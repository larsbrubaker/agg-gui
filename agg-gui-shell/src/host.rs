//! The app-side seam: [`ShellHost`], the per-frame/per-idle information the
//! shell hands it, and the [`ShellControl`] handle it steers the loop with.
//!
//! [`crate::run`] owns the window, the GPU and the event loop; everything an
//! app wants to do *around* those (custom frame rendering, state persistence,
//! diagnostics, exit/relaunch) goes through this trait. Every method has a
//! default, so a host with nothing to add is an empty impl.

use std::time::Duration;

use agg_gui::{App, Size};
use agg_gui_wgpu::{Gpu, WgpuGfxCtx};

/// What the shell knows about the frame it is about to paint.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Frame {
    /// Surface size in physical pixels — already clamped to the GPU limit, so
    /// this is what layout and the renderer must use, not `Window::inner_size`.
    pub width: u32,
    pub height: u32,
    /// `agg_gui::device_scale()` for this frame.
    pub device_scale: f64,
    /// Wall time the *previous* painted frame took (paint plus present).
    /// Zero for the first frame. This is the number a frame-time graph plots.
    pub duration: Duration,
    /// Count of frames painted so far, this one included (1-based).
    pub index: u64,
    /// Whether anything that feeds layout changed since the last painted
    /// frame — surface size, device scale, or the invalidation epoch. A host
    /// that overrides [`ShellHost::paint`] should honour it rather than laying
    /// out unconditionally.
    pub needs_layout: bool,
    /// Whether any input event arrived since the last painted frame. A
    /// runaway-repaint detector uses this to tell "the user is doing
    /// something" from "the app will not quiesce".
    pub input_since_last_frame: bool,
}

impl Frame {
    /// Surface size as the `Size` `App::layout` wants.
    pub fn viewport(&self) -> Size {
        Size::new(self.width as f64, self.height as f64)
    }
}

/// Window geometry as the OS last reported it. Sizes are physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct WindowGeometry {
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    pub fullscreen: bool,
    pub scale_factor: f64,
}

/// How the event loop should end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExitAction {
    /// Close normally.
    Exit,
    /// Close, then start a fresh copy of this executable. The shell spawns it
    /// only after the loop has exited and every store has been flushed, so the
    /// child reads the state this run just wrote.
    Relaunch,
}

/// Steering handle passed to [`ShellHost::on_idle`].
pub struct ShellControl<'a> {
    pub(crate) policy: &'a mut crate::RedrawPolicy,
    pub(crate) exit: &'a mut Option<ExitAction>,
    pub(crate) painted: bool,
    pub(crate) pointer_idle: bool,
}

impl ShellControl<'_> {
    /// Current redraw policy.
    pub fn redraw_policy(&self) -> crate::RedrawPolicy {
        *self.policy
    }

    /// Switch between reactive and continuous redraw at runtime.
    pub fn set_redraw_policy(&mut self, policy: crate::RedrawPolicy) {
        *self.policy = policy;
    }

    /// Whether a frame was painted in this loop iteration. `false` means the
    /// loop is idling — nothing asked for a frame.
    pub fn painted(&self) -> bool {
        self.painted
    }

    /// Whether no mouse button is currently held. The gate an app's own
    /// auto-save should use so a drag/resize doesn't thrash the disk; the
    /// shell applies the same gate to its [`crate::WindowBoundsStore`].
    pub fn pointer_idle(&self) -> bool {
        self.pointer_idle
    }

    /// Close the window and leave the event loop after this iteration.
    pub fn request_exit(&mut self) {
        // A pending relaunch is the stronger request — do not downgrade it.
        if self.exit.is_none() {
            *self.exit = Some(ExitAction::Exit);
        }
    }

    /// Leave the event loop and start a fresh copy of this executable.
    ///
    /// Used for settings that can only be applied at startup (MSAA sample
    /// count, backend selection). The spawn happens after the loop exits and
    /// after [`ShellHost::on_exit`] and the bounds store have run, so the
    /// child sees the state this run just saved.
    pub fn request_relaunch(&mut self) {
        *self.exit = Some(ExitAction::Relaunch);
    }
}

/// The app side of the shell.
///
/// The host owns whatever the app needs that is *not* the widget tree —
/// persistence handles, diagnostic state, GPU-readback plumbing — and the
/// shell calls into it around each frame and each idle iteration.
pub trait ShellHost {
    /// Runs at the start of every painted frame, before layout and paint.
    /// The hook for per-frame app state: advancing a wall-clock cell, pushing
    /// `frame.duration` into a history buffer, feeding a runaway detector.
    fn on_frame(&mut self, _app: &mut App, _frame: &Frame) {}

    /// Render the frame's contents.
    ///
    /// The default resets the context, lays out when `frame.needs_layout`, and
    /// paints — which is all most apps want. Override to wrap layout/paint in
    /// app-specific plumbing (a live inspector's edit queues, for example);
    /// the shell has already begun the frame and will call `end_frame` and
    /// `present` afterwards.
    fn paint(&mut self, app: &mut App, ctx: &mut WgpuGfxCtx, frame: &Frame) {
        default_paint(app, ctx, frame);
    }

    /// Runs after `WgpuGfxCtx::end_frame` and before `present`.
    ///
    /// The only place a GPU copy off the surface texture can happen: wgpu's
    /// swap chain takes ownership of that texture at `present`. `frame` is the
    /// same descriptor [`ShellHost::paint`] just saw — a read-back needs the
    /// surface size, and a capture that only fires on some frames needs the
    /// frame index.
    fn after_paint(&mut self, _ctx: &mut WgpuGfxCtx, _frame: &Frame) {}

    /// The OS window was resized, maximized, restored, or changed scale.
    fn on_geometry_changed(&mut self, _geometry: WindowGeometry) {}

    /// Runs once per idle iteration of the event loop, after any frame this
    /// iteration painted. The hook for app-owned auto-save, deferred work that
    /// must not run inside event dispatch, and exit/relaunch requests.
    fn on_idle(&mut self, _app: &mut App, _control: &mut ShellControl<'_>) {}

    /// The wgpu device was lost (TDR, driver reset, GPU removal, RDP session
    /// change) and the shell has rebuilt it. **Every GPU resource created from
    /// the old device is dead** — textures, buffers, pipelines cached by
    /// custom-render widgets — so drop them here and let them be recreated
    /// from `gpu` on the next frame.
    fn on_gpu_rebuilt(&mut self, _app: &mut App, _gpu: &Gpu) {}

    /// The loop is about to exit (window closed, exit requested, or a
    /// screenshot run finished). Last chance to flush state.
    fn on_exit(&mut self, _app: &mut App) {}
}

/// A host with nothing to add — `run(config, |_| Ok((app, NoHost)))`.
pub struct NoHost;

impl ShellHost for NoHost {}

/// The default frame body: reset, lay out if anything that feeds layout
/// changed, paint. Exposed so a host that overrides [`ShellHost::paint`] to
/// add work *around* the frame can still delegate the frame itself.
pub fn default_paint(app: &mut App, ctx: &mut WgpuGfxCtx, frame: &Frame) {
    ctx.reset(frame.width as f32, frame.height as f32);
    ctx.set_lcd_mode(agg_gui::font_settings::lcd_enabled());
    if frame.needs_layout {
        app.layout(frame.viewport());
    }
    app.paint(ctx);
}
