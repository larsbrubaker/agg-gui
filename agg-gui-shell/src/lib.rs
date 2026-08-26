//! Turn-key native shell for an [`agg_gui::App`] — winit window, wgpu present,
//! input forwarding, redraw scheduling, window-bounds persistence, and
//! device-loss recovery.
//!
//! Every native agg-gui app used to hand-roll the same several hundred lines
//! of event loop, and every copy drifted: one had key-up dispatch and surface
//! recovery, another had touch input and the anti-flash first-paint ordering,
//! a third had the DPI-safe window restore. This crate is the union of them.
//!
//! ```no_run
//! use agg_gui_shell::{run, NoHost, ShellConfig};
//!
//! # fn build_my_app() -> agg_gui::App { unimplemented!() }
//! fn main() -> Result<(), agg_gui_shell::ShellError> {
//!     run(
//!         ShellConfig::new("My App").with_logical_size(1280.0, 720.0),
//!         |_init| Ok((build_my_app(), NoHost)),
//!     )
//! }
//! ```
//!
//! # What the shell owns
//!
//! - Window creation with DPI-safe restore of a saved size (see
//!   [`WindowBoundsStore`]) and the paint-first-then-show ordering that avoids
//!   a white flash on start-up.
//! - Mouse, wheel (including the shift→horizontal remap), keyboard **down and
//!   up**, touch, cursor-leave, modifiers, and the OS cursor icon.
//! - Reactive redraw scheduling: `Poll` while the app wants frames,
//!   `WaitUntil` for a scheduled deadline, `Wait` otherwise — or
//!   [`RedrawPolicy::Continuous`] when the app wants every frame.
//! - The agg-gui host waker, so a background thread's
//!   `animation::signal_async_state_change` wakes a parked loop; installed
//!   only after the window and GPU came up and cleared on every exit path.
//! - Surface-acquire recovery, swap-chain resize coalescing, and rebuilding
//!   the device after a **device loss** (TDR, driver reset, GPU removal, RDP
//!   session change) — see [`ShellHost::on_gpu_rebuilt`].
//! - Optional deterministic screenshot capture, and fullscreen toggles
//!   requested through `agg_gui::fullscreen`.
//!
//! # What the app owns
//!
//! Everything else, through [`ShellHost`]: per-frame state ticks, a custom
//! frame body, GPU read-back after the render is submitted, geometry change
//! notifications, idle-time work, exit and relaunch. Every method has a
//! default, so [`NoHost`] is a complete implementation for an app that just
//! wants a window.
//!
//! # Public API surface
//!
//! `winit` and `wgpu` types appear in this crate's API ([`ShellConfig`]'s
//! present mode, [`ShellHost::paint`]'s render context, the window handed to a
//! builder closure). Their major versions are therefore part of *this* crate's
//! public API: a `winit` 0.31 or `wgpu` 30 will be a breaking release here,
//! and an app must depend on the same majors this crate does.
//!
//! The browser equivalent of this crate does not live here — a wasm app drives
//! its own `requestAnimationFrame` loop against `agg-gui-wgpu`.

mod bounds;
mod config;
mod host;
mod input;
mod paint;
mod run;
mod screenshot;
mod shell_loop;
mod tooltip;
mod waker;

pub use bounds::{sanitize_restored_window_size, SavedBounds, WindowBoundsStore};
pub use config::{RedrawPolicy, ScreenshotConfig, ShellConfig, WindowIcon, WindowSize};
pub use host::{default_paint, Frame, NoHost, ShellControl, ShellHost, WindowGeometry};
pub use run::run;
pub use shell_loop::ShellInit;

// Re-exported so a consumer can build a `ShellHost` without naming the
// renderer crate, and so the versions can never disagree.
pub use agg_gui_wgpu::{CopySrc, Gpu, WgpuGfxCtx};

/// The `wgpu` this shell was built against.
///
/// Its major version is part of this crate's public API (present modes, the
/// render context, the device behind [`Gpu`]), so an app that names `wgpu`
/// types itself should reach them through here rather than adding its own
/// `wgpu` dependency, where a mismatched major would silently produce two
/// incompatible sets of types.
pub use wgpu;

/// The `winit` this shell was built against.
///
/// Its major version is part of this crate's public API (the window handed to
/// the builder closure, fullscreen and monitor types), so an app that names
/// `winit` types itself should reach them through here rather than adding its
/// own `winit` dependency.
pub use winit;

/// Why the shell could not start, or could not finish.
#[derive(Debug)]
#[non_exhaustive]
pub enum ShellError {
    /// Creating or running the winit event loop failed.
    EventLoop(winit::error::EventLoopError),
    /// The OS refused to create the window.
    CreateWindow(winit::error::OsError),
    /// wgpu could not produce a usable device or surface — at start-up, or
    /// when rebuilding after a device loss.
    Gpu(agg_gui_wgpu::GpuInitError),
    /// A configured deterministic capture could not be produced.
    Screenshot(String),
    /// [`ShellControl::request_relaunch`] could not start the new process.
    Relaunch(std::io::Error),
    /// The app's builder closure failed. Build one with [`ShellError::app`].
    App(Box<dyn std::error::Error + Send + Sync>),
}

impl ShellError {
    /// Wrap an app-side start-up failure so a builder closure can use `?`.
    pub fn app(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::App(error.into())
    }
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventLoop(e) => write!(f, "winit event loop: {e}"),
            Self::CreateWindow(e) => write!(f, "create window: {e}"),
            Self::Gpu(e) => write!(f, "wgpu init: {e}"),
            Self::Screenshot(msg) => write!(f, "screenshot: {msg}"),
            Self::Relaunch(e) => write!(f, "relaunch: {e}"),
            Self::App(e) => write!(f, "app start-up: {e}"),
        }
    }
}

impl std::error::Error for ShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EventLoop(e) => Some(e),
            Self::CreateWindow(e) => Some(e),
            Self::Gpu(e) => Some(e),
            Self::Relaunch(e) => Some(e),
            Self::App(e) => Some(e.as_ref()),
            Self::Screenshot(_) => None,
        }
    }
}
