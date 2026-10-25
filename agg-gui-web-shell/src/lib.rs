//! Turn-key browser shell for an [`agg_gui::App`] — the wasm counterpart of
//! [`agg-gui-shell`](https://docs.rs/agg-gui-shell).
//!
//! A wasm app calls [`start`] once from its `#[wasm_bindgen(start)]` and the
//! shell owns everything platform-generic:
//!
//! - a WebGPU or WebGL2 wgpu surface on a `<canvas>` — the consumer picks with
//!   [`Backend`] (WebGPU and WebGL2 need the default `webgpu` and `webgl`
//!   features; a WebGPU-only app turns `webgl` off), including a
//!   clean WebGPU → WebGL2 fallback probed before the canvas is bound, and
//!   the app's own check of the adapter
//!   ([`WebShellConfig::with_adapter_check`]) before the device is requested;
//! - the `requestAnimationFrame` loop with [`RedrawPolicy::Reactive`] /
//!   [`RedrawPolicy::Continuous`] run modes, layout caching, the
//!   **first-paint guarantee** (the first frame always paints, even when
//!   nothing requested a draw by the time the async GPU init resolved), and
//!   **idle sleep**: while nothing needs a frame the loop requests none, so
//!   a settled page costs no CPU (see "Asking for frames");
//! - **frame pacing**: two presented frames on the GPU at most, one on a GPU
//!   whose frames take longer than 50 ms, so a slow (software) GPU never
//!   builds a backlog that input and the page's own scrolling wait behind;
//!   ticks keep running, a skipped redraw paints as soon as the GPU catches
//!   up, and a frame whose completion never arrives stops being waited for
//!   after at most 10 s;
//! - canvas backing-store sizing (`clientSize × devicePixelRatio`, with the
//!   scale reduced uniformly when that would exceed the device's texture
//!   limit) and DPR tracking every tick, woken by a canvas `ResizeObserver`
//!   and a `devicePixelRatio` media query (browser zoom and a move to
//!   another monitor change DPR without a resize event); a full-window
//!   canvas is capped at the visible viewport's bottom (`innerHeight`), so a
//!   page sized with `100vh` keeps the app, and its on-screen keyboard, on
//!   screen while Android Chrome's address bar shows;
//! - DOM pointer (mouse / pen / multi-touch), wheel, context-menu, and
//!   pointer-leave listeners; physical keyboard **plus the copy/cut/paste
//!   clipboard bridge** via `agg_gui::web_adapter`;
//! - page lifecycle: a window-level `pointerup` resync so a release outside the
//!   canvas can't wedge the pointer-idle guard, and `visibilitychange` /
//!   `pagehide` flush hooks ([`WebShellHost::on_page_hide`]);
//! - surface-acquire recovery and WebGPU **device-loss** rebuild (backed
//!   off, then the fatal panel); a lost WebGL2 context is reported as fatal
//!   rather than rebuilt;
//! - `agg_gui::fullscreen`, `agg_gui::tilt` and `agg_gui::gamepad` plumbing;
//! - optional `localStorage` settings ([`LocalStorageSettings`] +
//!   [`SettingsAutoSave`]).
//!
//! ```ignore
//! use agg_gui_web_shell::{start, NoHost, WebShellConfig};
//! use wasm_bindgen::prelude::*;
//!
//! #[wasm_bindgen(start)]
//! pub fn main() {
//!     // Errors are already on the console and in the fatal panel; the
//!     // Result is for apps that want to react (or fail the wasm start).
//!     start(WebShellConfig::new("canvas"), |_init| Ok((build_my_app(), NoHost)))
//!         .expect("agg-gui-web-shell start");
//! }
//! ```
//!
//! The app side mirrors `agg_gui_shell::ShellHost` method for method
//! ([`WebShellHost`]), so native and web glue can share one host type.
//!
//! # Asking for frames
//!
//! The loop runs a tick for every animation frame only while something needs
//! one; otherwise it sleeps until a wake, or until a single timer for the
//! earliest scheduled draw. Nothing an app already does needs to change:
//!
//! - **Input** (pointer, wheel, keys, touch, IME composition, paste, file
//!   drop, window focus and blur) wakes it; the tick paints if the event
//!   changed something (`App::wants_draw`).
//! - **Widgets and app code** ask through agg-gui: `animation::request_draw`
//!   (and its tagged / no-invalidation forms), `request_layout`,
//!   `request_draw_after`, `schedule_draw_after` (whose handle withdraws
//!   it again: the loop then re-arms its timer for the next deadline or
//!   sleeps) and per-widget draw deadlines (caret blink, tooltips),
//!   `ui_thread::run_on_idle` / `run_on_idle_after` /
//!   `set_interval`, `animation::signal_async_state_change`, `tilt` and
//!   `fullscreen` requests. Made outside a tick (a fetch completion, a
//!   promise, a `setTimeout`) they wake the loop through
//!   `agg_gui::frame_demand`; made inside one, the end-of-tick decision sees
//!   them. A running tween or glide re-requests a draw each frame, which keeps
//!   the loop running until it settles.
//! - **App exports** call [`mark_dirty`] (repaint) or [`request_tick`] (one
//!   tick, no forced paint).
//! - **A host that polls** something without an event in
//!   [`WebShellHost::on_tick`] / `on_idle` returns `true` from
//!   [`WebShellHost::wants_tick`] while it polls.
//!
//! The shell keeps the loop running itself while a mouse button or touch is
//! held, a gamepad is connected, input waits for an app that was busy, a
//! frame has yet to be presented, a redraw waits for the GPU to finish
//! earlier frames, or the policy is [`RedrawPolicy::Continuous`]; resize,
//! DPR, visibility, fullscreen, gamepad-connect, device-loss, a failed
//! rebuild and backoff timers wake it.
//!
//! # Targets
//!
//! The browser runtime ([`start`], [`mark_dirty`], [`with_app`], …) exists only
//! on `wasm32`. On native targets the crate compiles to its platform-neutral
//! half — configuration, the host trait, the paint-decision policy, DOM math
//! and the settings store — so `cargo build --workspace` / `cargo test` work
//! everywhere and those decisions are unit tested natively.
//!
//! # Public API surface
//!
//! `wgpu` types appear in this crate's API; its major version is part of this
//! crate's public API. Reach wgpu through [`wgpu`] (re-exported) rather than a
//! separate dependency; likewise [`agg_gui`], [`agg_gui_wgpu`] and (wasm)
//! `web_sys` are re-exported so an app's versions always match the shell's.

// The crate-internal decision modules (dom_math, pointer, recovery, fatal,
// frames_in_flight, held_input, most of policy) are consumed only by the wasm runtime; natively they are
// exercised by the unit tests alone.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod adapter_check;
mod config;
mod cursor;
mod dom_math;
mod error;
mod fatal;
// Native only: it paints on a headless GPU frame.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod frame_clear_tests;
mod frame_schedule;
mod frames_in_flight;
mod held_input;
mod host;
mod pointer;
mod policy;
mod recovery;
mod settings;

pub use adapter_check::{AdapterCheck, AdapterCheckFn};
pub use config::{Backend, RedrawPolicy, WebShellConfig};
pub use error::{GpuInfo, WebShellError};
pub use fatal::{FatalHook, FatalMessageFn};
pub use host::{default_paint, CanvasGeometry, Frame, NoHost, WebShellControl, WebShellHost};
pub use settings::{
    LocalStorageSettings, MemorySettings, SettingsAutoSave, SettingsError, SettingsStore,
};

/// The `agg-gui` this shell was built against.
pub use agg_gui;
/// The `agg-gui-wgpu` renderer this shell was built against.
pub use agg_gui_wgpu;
pub use agg_gui_wgpu::WgpuGfxCtx;
/// The `web-sys` this shell was built against (its canvas type appears in
/// [`WebShellInit`] and [`with_canvas`]).
#[cfg(target_arch = "wasm32")]
pub use web_sys;
/// The `wgpu` this shell was built against — see "Public API surface".
pub use wgpu;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{
    has_presented, mark_dirty, redraw_policy, request_tick, set_redraw_policy, start, with_app,
    with_canvas, WebShellInit,
};
