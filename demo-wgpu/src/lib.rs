//! Platform shells + wgpu-using demo widgets, on top of the `agg-gui-wgpu`
//! renderer.
//!
//! # What lives where
//!
//! The renderer itself — [`WgpuGfxCtx`], its pipelines and shaders, SSAA
//! framebuffers, screenshot capture, the [`custom_render`] hook and the
//! [`gpu::Gpu`] surface bundle — was extracted into the publishable
//! [`agg_gui_wgpu`] crate. This crate is now:
//!
//! - **Platform shells**: [`native_shell`] (winit event loop + wgpu present)
//!   and [`web_shell`] (canvas + rAF loop + DOM input), the turn-key harnesses
//!   a platform shim reduces to.
//! - **Inspector plumbing**: [`render_app_frame`], which drains the live
//!   inspector edit queues around layout + paint.
//! - **Demo widgets that need the GPU**: the 3-D bar-grid cube
//!   ([`WgpuCubeWidget`]), which rides the generic custom-render hook.
//!
//! Everything in `agg-gui-wgpu` is re-exported below, so existing consumers
//! (`demo-native`, `demo-wasm`, and apps that depend on this crate by path)
//! keep compiling against `demo_wgpu::` paths unchanged. New code should
//! depend on `agg-gui-wgpu` directly for the renderer and reach for this crate
//! only when it wants a shell or the demo widgets.
//!
//! # Platform-split policy (mirrors `demo-gl`)
//!
//! Platform shells (`demo-native`, `demo-wasm`) are pure OS shims; all
//! rendering code lives below this crate so both targets execute identical
//! compiled bytes.
//!
//! - Generic widget / layout code (no GPU dependency) → `demo-ui`
//! - wgpu-using demo widgets (bar grid, etc.) → here, in dedicated modules
//! - Platform shell (OS window/canvas, event loop, persistence) → `demo-native` / `demo-wasm`

// The whole renderer surface, under the paths consumers already use.
pub use agg_gui_wgpu::*;

pub mod frame;
pub use frame::{begin_frame, render_app_frame};

/// Deprecated winit + wgpu shell — a thin wrapper over the `agg-gui-shell`
/// crate, kept so external path-dependency consumers keep compiling.
#[cfg(not(target_arch = "wasm32"))]
pub mod native_shell;
#[cfg(not(target_arch = "wasm32"))]
#[allow(deprecated)]
pub use native_shell::NativeShellConfig;

/// Turn-key canvas + rAF + DOM-input shell for wasm platform shims.
#[cfg(target_arch = "wasm32")]
pub mod web_shell;

pub mod bar_grid;
mod bar_grid_math;
pub mod bar_grid_render;
pub use bar_grid::{BarGridCustomRenderer, BarGridWgpuRenderer, WgpuCubeWidget, CUBE_SCREEN_RECT};
