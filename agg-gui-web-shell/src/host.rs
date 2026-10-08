//! The app-side seam: [`WebShellHost`], the per-frame information the shell
//! hands it, and the [`WebShellControl`] handle it steers the loop with.
//!
//! Deliberately shaped like `agg_gui_shell::ShellHost` (same method names and
//! call order: `on_frame` → `paint` → `after_paint` → `on_idle`) so an app's
//! native and web glue are near-identical. The web-only additions are
//! [`WebShellHost::on_tick`] (every `requestAnimationFrame`, painted or not)
//! and [`WebShellHost::on_page_hide`] (the browser's last chance to write,
//! standing in for native `on_exit`). Platform-neutral: compiles natively.

use std::time::Duration;

use agg_gui::{App, Size};
use agg_gui_wgpu::WgpuGfxCtx;

use crate::config::RedrawPolicy;

/// What the shell knows about the frame it is about to paint. Same fields as
/// `agg_gui_shell::Frame`.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Frame {
    /// Surface size in physical pixels (canvas backing store, clamped to the
    /// device's texture limit) — what layout and the renderer must use.
    pub width: u32,
    pub height: u32,
    /// `agg_gui::device_scale()` for this frame.
    pub device_scale: f64,
    /// Wall time the *previous* painted frame took (paint plus present).
    /// Zero for the first frame.
    pub duration: Duration,
    /// Count of frames painted so far, this one included (1-based).
    pub index: u64,
    /// Whether anything that feeds layout changed since the last laid-out
    /// frame, or a widget called `agg_gui::animation::request_layout`. A host
    /// overriding [`WebShellHost::paint`] should honour it.
    pub needs_layout: bool,
    /// Whether any input event arrived since the last painted frame.
    pub input_since_last_frame: bool,
}

impl Frame {
    /// Surface size as the `Size` `App::layout` wants.
    pub fn viewport(&self) -> Size {
        Size::new(self.width as f64, self.height as f64)
    }
}

/// Canvas geometry as last observed. Sizes are physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct CanvasGeometry {
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub scale_factor: f64,
}

/// Steering handle passed to [`WebShellHost::on_idle`].
pub struct WebShellControl<'a> {
    pub(crate) policy: &'a mut RedrawPolicy,
    pub(crate) painted: bool,
    pub(crate) pointer_idle: bool,
}

impl<'a> WebShellControl<'a> {
    /// Build a control handle. Public so host logic can be unit tested
    /// natively without the browser runtime.
    pub fn new(policy: &'a mut RedrawPolicy, painted: bool, pointer_idle: bool) -> Self {
        Self {
            policy,
            painted,
            pointer_idle,
        }
    }

    pub fn redraw_policy(&self) -> RedrawPolicy {
        *self.policy
    }

    /// Switch between reactive and continuous redraw at runtime.
    pub fn set_redraw_policy(&mut self, policy: RedrawPolicy) {
        *self.policy = policy;
    }

    /// Whether a frame was painted this tick.
    pub fn painted(&self) -> bool {
        self.painted
    }

    /// Whether no mouse button is held — the gate an auto-save should use so a
    /// drag doesn't thrash storage. Kept honest by a window-level `pointerup`
    /// listener and a `buttons` resync on every move, so a release outside the
    /// canvas can't wedge it closed.
    pub fn pointer_idle(&self) -> bool {
        self.pointer_idle
    }
}

/// The app side of the web shell. Every method has a default, so [`NoHost`]
/// is a complete implementation.
///
/// # Borrow rules inside callbacks
///
/// The shell holds the app and the host mutably borrowed while it calls a
/// host method. From inside any callback:
///
/// - use the `&mut App` you were handed — the global `with_app` (wasm)
///   returns `None` there (the app is already borrowed) and the closure does
///   not run;
/// - `with_canvas`, `mark_dirty`, `set_redraw_policy`, `redraw_policy` and
///   `has_presented` are safe (separate cells, never held across a callback);
/// - a `#[wasm_bindgen]` export the page calls *during* a callback cannot
///   happen (the browser main thread is busy), so exports may use `with_app`
///   freely.
pub trait WebShellHost {
    /// Runs on **every** `requestAnimationFrame` tick, before the shell
    /// drains `agg_gui::ui_thread`'s queued work and decides whether to
    /// paint. The hook for polling app state that has no
    /// event (a storage job pump, a wall clock); call [`crate::mark_dirty`]
    /// (or `animation::request_draw`) to make the tick paint.
    fn on_tick(&mut self, _app: &mut App) {}

    /// Runs at the start of every painted frame, before layout and paint.
    fn on_frame(&mut self, _app: &mut App, _frame: &Frame) {}

    /// Render the frame's contents. The shell has already begun the frame and
    /// will call `end_frame` and `present` afterwards.
    fn paint(&mut self, app: &mut App, ctx: &mut WgpuGfxCtx, frame: &Frame) {
        default_paint(app, ctx, frame);
    }

    /// Runs after `WgpuGfxCtx::end_frame`, before `present`. With
    /// [`crate::WebShellConfig::offscreen_scene`] the context's surface texture
    /// is the copyable scene texture, so `capture_screenshot` works here.
    fn after_paint(&mut self, _ctx: &mut WgpuGfxCtx, _frame: &Frame) {}

    /// Runs right after the frame was presented to the canvas — the point at
    /// which it is actually on screen. The hook for "first frame visible"
    /// signals (a page-level ready flag for an end-to-end test) and present
    /// timing. Not called for a tick that bailed before presenting.
    /// [`crate::has_presented`] (wasm) answers the same question globally.
    fn after_present(&mut self, _app: &mut App, _frame: &Frame) {}

    /// The canvas backing size, DPR, or fullscreen state changed. Also called
    /// once right after boot, with the geometry the app was built at.
    fn on_geometry_changed(&mut self, _geometry: CanvasGeometry) {}

    /// Runs once per tick after any painted frame — auto-save, deferred work,
    /// run-mode changes.
    fn on_idle(&mut self, _app: &mut App, _control: &mut WebShellControl<'_>) {}

    /// The GPU device was lost and rebuilt. Every resource from the old device
    /// is dead — drop cached textures/pipelines so they are recreated.
    fn on_gpu_rebuilt(&mut self, _app: &mut App, _gpu: &crate::GpuInfo) {}

    /// The page was hidden (`visibilitychange` → hidden) or is being torn down
    /// (`pagehide`) — the browser's closest thing to a reliable last chance to
    /// write. Flush settings here, bypassing any pointer-idle guard. May fire
    /// more than once per page (every tab switch).
    fn on_page_hide(&mut self, _app: &mut App) {}
}

/// A host with nothing to add.
pub struct NoHost;

impl WebShellHost for NoHost {}

/// The default frame body: reset, lay out if anything that feeds layout
/// changed, paint. Identical to `agg_gui_shell::default_paint`.
pub fn default_paint(app: &mut App, ctx: &mut WgpuGfxCtx, frame: &Frame) {
    ctx.reset(frame.width as f32, frame.height as f32);
    ctx.set_lcd_mode(agg_gui::font_settings::lcd_enabled());
    if frame.needs_layout {
        app.layout(frame.viewport());
    }
    app.paint(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_switches_policy() {
        let mut policy = RedrawPolicy::Reactive;
        let mut c = WebShellControl::new(&mut policy, true, false);
        assert!(c.painted());
        assert!(!c.pointer_idle());
        c.set_redraw_policy(RedrawPolicy::Continuous);
        assert_eq!(c.redraw_policy(), RedrawPolicy::Continuous);
        assert_eq!(policy, RedrawPolicy::Continuous);
    }
}
