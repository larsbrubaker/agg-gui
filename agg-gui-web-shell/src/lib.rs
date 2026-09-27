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
//!   **first-paint guarantee** (see [`FirstPaintGate`]);
//! - canvas backing-store sizing (`clientSize × devicePixelRatio`) and DPR
//!   tracking every tick (browser zoom changes DPR without a resize event);
//! - DOM pointer (mouse / pen / multi-touch), wheel, context-menu, and
//!   pointer-leave listeners; physical keyboard **plus the copy/cut/paste
//!   clipboard bridge** via `agg_gui::web_adapter`;
//! - page lifecycle: a window-level `pointerup` resync so a release outside the
//!   canvas can't wedge the pointer-idle guard, and `visibilitychange` /
//!   `pagehide` flush hooks ([`WebShellHost::on_page_hide`]);
//! - surface-acquire recovery and GPU **device-loss** rebuild;
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
//!     let _ = start(WebShellConfig::new("canvas"), |_init| Ok((build_my_app(), NoHost)));
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
//! separate dependency.

mod config;
pub mod dom_math;
mod error;
mod host;
mod policy;
mod settings;

pub use config::{Backend, RedrawPolicy, WebShellConfig};
pub use error::{device_limits, pick_surface_format, GpuInfo, WebShellError};
pub use host::{default_paint, CanvasGeometry, Frame, NoHost, WebShellControl, WebShellHost};
pub use policy::{layout_key, wants_paint, FirstPaintGate, LayoutKey};
pub use settings::{
    LocalStorageSettings, MemorySettings, SettingsAutoSave, SettingsError, SettingsStore,
};

pub use agg_gui_wgpu::WgpuGfxCtx;
/// The `wgpu` this shell was built against — see "Public API surface".
pub use wgpu;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{
    mark_dirty, redraw_policy, set_redraw_policy, start, with_app, with_canvas, WebShellInit,
};
