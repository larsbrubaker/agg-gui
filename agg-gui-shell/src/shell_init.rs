//! [`ShellInit`]: what the shell hands an app's builder closure once the
//! window and GPU exist but before the app is built.
//!
//! Split out of `shell_loop.rs` (the running event loop): this is the small
//! public view [`crate::run`] constructs for the builder and then drops.

use std::sync::Arc;

use agg_gui_wgpu::Gpu;
use winit::window::Window;

use crate::bounds::SavedBounds;
use crate::position::SavedPosition;

/// Everything the shell knows once the window and GPU exist but before the app
/// is built — what a builder closure needs to size its first layout or share
/// the device with an app-owned renderer.
pub struct ShellInit<'a> {
    pub(crate) window: &'a Arc<Window>,
    pub(crate) gpu: &'a Gpu,
    pub(crate) restored_bounds: Option<SavedBounds>,
    pub(crate) restored_position: Option<SavedPosition>,
}

impl ShellInit<'_> {
    /// The OS window. Handy for a title the app computes, or for platform
    /// integration the shell doesn't wrap.
    pub fn window(&self) -> &Arc<Window> {
        self.window
    }

    /// The live device + surface bundle. An app with its own wgpu renderer
    /// builds it from this device.
    pub fn gpu(&self) -> &Gpu {
        self.gpu
    }

    /// Surface size in physical pixels — already clamped to the GPU limit.
    pub fn size(&self) -> (u32, u32) {
        (self.gpu.config().width, self.gpu.config().height)
    }

    /// What the [`WindowBoundsStore`] returned, if anything. An app that
    /// keeps other window state alongside the bounds can use this to tell a
    /// restored launch from a first one.
    pub fn restored_bounds(&self) -> Option<SavedBounds> {
        self.restored_bounds
    }

    /// What the [`WindowBoundsStore`] returned for the position, before the
    /// shell moved it onto a connected monitor. Read the window's
    /// `outer_position()` for where the window actually opened.
    pub fn restored_position(&self) -> Option<SavedPosition> {
        self.restored_position
    }

    /// `agg_gui::device_scale()`, already applied from the window.
    pub fn device_scale(&self) -> f64 {
        agg_gui::device_scale()
    }
}
