//! Turn-key browser shell for an [`agg_gui::App`] — the wasm counterpart of
//! [`agg-gui-shell`](https://docs.rs/agg-gui-shell).
//!
//! A wasm app calls [`start`] once from its `#[wasm_bindgen(start)]` and the
//! shell owns everything platform-generic:
//!
//! - a WebGPU or WebGL2 wgpu surface on a `<canvas>` — the consumer picks with
//!   [`Backend`] (WebGPU needs the default `webgpu` feature), including a
//!   clean WebGPU → WebGL2 fallback probed before the canvas is bound;
//! - the `requestAnimationFrame` loop with [`RedrawPolicy::Reactive`] /
//!   [`RedrawPolicy::Continuous`] run modes, layout caching, and the
//!   **first-paint guarantee** (the first frame always paints, even when
//!   nothing requested a draw by the time the async GPU init resolved);
//! - canvas backing-store sizing (`clientSize × devicePixelRatio`, with the
//!   scale reduced uniformly when that would exceed the device's texture
//!   limit) and DPR tracking every tick (browser zoom changes DPR without a
//!   resize event);
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
// most of policy) are consumed only by the wasm runtime; natively they are
// exercised by the unit tests alone.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod config;
mod dom_math;
mod error;
mod fatal;
mod host;
mod pointer;
mod policy;
mod recovery;
mod settings;

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
    has_presented, mark_dirty, redraw_policy, set_redraw_policy, start, with_app, with_canvas,
    WebShellInit,
};
