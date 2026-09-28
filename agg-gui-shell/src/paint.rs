//! One painted frame: surface acquire, the host's frame body, the deferred
//! screenshot read-back, present — and the draw-request bookkeeping for a
//! frame the surface refused.
//!
//! Split out of `run.rs` so the event loop file is about events. The state
//! that only the paint path cares about — the render context, the layout-skip
//! key, the frame counter — lives in [`Painter`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui::App;
use agg_gui_wgpu::{Gpu, WgpuGfxCtx};
use winit::window::Window;

use crate::config::ScreenshotConfig;
use crate::host::{Frame, ShellHost};
use crate::screenshot::{should_capture, write_png};
use crate::ShellError;

/// What one call to [`Painter::paint`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct PaintOutcome {
    /// A frame reached the compositor. `false` means the surface would not
    /// hand out a texture (minimized, occluded, lost) — nothing was drawn.
    pub(crate) painted: bool,
    /// A pending deterministic capture fired this frame.
    pub(crate) captured: bool,
}

/// The per-call inputs to [`Painter::paint`], bundled so the signature stays
/// readable.
pub(crate) struct PaintRequest<'a> {
    /// Coalesced window resize to apply before acquiring the surface.
    pub(crate) pending_resize: &'a mut Option<(u32, u32)>,
    pub(crate) input_since_last_frame: bool,
    /// A still-pending deterministic capture, if one is configured.
    pub(crate) capture: Option<&'a ScreenshotConfig>,
}

/// Owns the render context and the per-frame bookkeeping.
pub(crate) struct Painter {
    ctx: WgpuGfxCtx,
    /// Surface size + device scale + invalidation epoch of the last laid-out
    /// frame. Layout is skipped while it is unchanged.
    layout_key: Option<(u32, u32, u64, u64)>,
    frames_painted: u64,
    last_duration: Duration,
}

impl Painter {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        Self {
            ctx: Self::make_ctx(gpu),
            layout_key: None,
            frames_painted: 0,
            last_duration: Duration::ZERO,
        }
    }

    fn make_ctx(gpu: &Gpu) -> WgpuGfxCtx {
        WgpuGfxCtx::new(
            Arc::clone(gpu.device()),
            Arc::clone(gpu.queue()),
            gpu.surface_format(),
            gpu.config().width as f32,
            gpu.config().height as f32,
        )
    }

    /// Rebuild the render context against a fresh device after device loss.
    /// Every cached GPU resource in the old context died with the old device,
    /// so the context is replaced wholesale; the layout key is dropped too so
    /// the next frame lays out from scratch.
    pub(crate) fn rebuild(&mut self, gpu: &Gpu) {
        self.ctx = Self::make_ctx(gpu);
        self.layout_key = None;
    }

    /// Paint one frame.
    ///
    /// `req.pending_resize` is applied here rather than in the `Resized` event
    /// arm: winit can deliver `Resized` from a modal drag-resize loop, and
    /// reconfiguring the surface between `get_current_texture` and `present`
    /// is a validation error. Applying the coalesced size at the top of the
    /// frame means a reconfigure can never land inside one.
    ///
    /// When the surface hands out no texture the frame is skipped and the
    /// immediate draw request is dropped ([`frame_skipped`]): the skip has
    /// already arranged its own wake, and a request left set would keep the
    /// loop polling.
    pub(crate) fn paint<H: ShellHost>(
        &mut self,
        gpu: &mut Gpu,
        window: &Window,
        app: &mut App,
        host: &mut H,
        req: PaintRequest<'_>,
    ) -> Result<PaintOutcome, ShellError> {
        let PaintRequest {
            pending_resize,
            input_since_last_frame,
            capture,
        } = req;
        if let Some((w, h)) = pending_resize.take() {
            gpu.resize(w, h);
        }
        let (win_w, win_h) = (gpu.config().width, gpu.config().height);
        // A zero-sized (minimized) window has no presentable surface, and
        // `Gpu::resize` refuses to configure one — same guard as there.
        if win_w == 0 || win_h == 0 {
            return Ok(PaintOutcome::default());
        }

        let started = Instant::now();
        let Some(surface_frame) = gpu.acquire_frame(|| window.request_redraw()) else {
            frame_skipped();
            return Ok(PaintOutcome::default());
        };
        let view = surface_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Skip layout when nothing that feeds it changed: same surface size,
        // same DPI, same invalidation epoch.
        let next_layout_key = (
            win_w,
            win_h,
            agg_gui::device_scale().to_bits(),
            agg_gui::animation::invalidation_epoch(),
        );
        self.frames_painted += 1;
        let frame = Frame {
            width: win_w,
            height: win_h,
            device_scale: agg_gui::device_scale(),
            duration: self.last_duration,
            index: self.frames_painted,
            needs_layout: self.layout_key != Some(next_layout_key),
            input_since_last_frame,
        };

        host.on_frame(app, &frame);

        self.ctx.set_surface_texture(surface_frame.texture.clone());
        self.ctx.begin_frame(view);
        host.paint(app, &mut self.ctx, &frame);
        self.layout_key = Some(next_layout_key);
        self.ctx.end_frame();

        // After the render is submitted, before the surface texture goes back
        // to the compositor: the only window in which the frame can be copied
        // or read back.
        host.after_paint(&mut self.ctx, &frame);

        if let Some(cfg) = capture {
            if should_capture(self.frames_painted, cfg.settle_frames) {
                let (rgba, w, h) = self.ctx.read_screenshot();
                let result = write_png(&cfg.path, &rgba, w, h);
                surface_frame.present();
                result.map_err(ShellError::Screenshot)?;
                self.last_duration = started.elapsed();
                return Ok(PaintOutcome {
                    painted: true,
                    captured: true,
                });
            }
        }

        surface_frame.present();
        self.last_duration = started.elapsed();
        Ok(PaintOutcome {
            painted: true,
            captured: false,
        })
    }
}

/// Drop the immediate draw request after a frame the surface refused.
///
/// Only `App::paint` clears that request, and a skipped frame never reaches
/// it — left set, `wants_draw()` stays true and the shell sits in
/// `ControlFlow::Poll`, spinning while a failed surface configure backs off.
/// Every skip has already arranged its own wake: `Gpu::acquire_frame` calls
/// `request_redraw` for an immediate retry or a `Timeout`, and
/// `request_draw_after` for a backed-off configure; an `Occluded` (macOS)
/// window is woken by `WindowEvent::Occluded(false)` in the shell loop.
///
/// `clear_draw_request` is not a narrow clear, so two things it would lose are
/// preserved:
/// - pending cross-thread async wakeups are pumped first (through
///   `async_state_epoch`), because the clear marks them seen without bumping
///   the async-state epoch, and the next real paint would skip its dirty walk;
/// - the scheduled deadline is re-armed, because the clear drops it, and it
///   is what wakes a backed-off retry (or an animation) later. Re-arming goes
///   through `request_draw_after`, which re-reads the clock, so the deadline
///   can slip by the few microseconds this function takes.
pub(crate) fn frame_skipped() {
    use agg_gui::animation;
    let _ = animation::async_state_epoch();
    let deadline = animation::peek_next_draw_deadline();
    animation::clear_draw_request();
    if let Some(when) = deadline {
        animation::request_draw_after(when.saturating_duration_since(Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::frame_skipped;
    use agg_gui::animation;
    use std::time::Duration;

    // Animation state is thread-local and the test harness runs each test on
    // its own thread, so these start from a clean slate.

    #[test]
    fn skipped_frame_drops_the_immediate_request_but_keeps_the_deadline() {
        // A skipped frame never reaches `App::paint`, the only other place
        // the immediate request is cleared. Left set, it keeps the shell in
        // `ControlFlow::Poll` — a busy loop while the surface backs off.
        animation::request_draw();
        animation::request_draw_after(Duration::from_secs(60));
        let deadline = animation::peek_next_draw_deadline();
        assert!(deadline.is_some());

        frame_skipped();

        // Read the flag directly: `wants_draw()` also folds in a process-wide
        // async counter that a sibling test's thread can bump at any time.
        assert!(
            !animation::peek_draw_signals().0,
            "immediate request must be cleared"
        );
        // The deadline is how a backed-off surface retry wakes the loop. It
        // is re-armed from a fresh clock read, so allow a small slip — but
        // never earlier than asked.
        let (before, after) = (deadline.unwrap(), animation::peek_next_draw_deadline());
        let after = after.expect("scheduled deadline must survive the skip");
        assert!(after >= before);
        assert!(after - before < Duration::from_millis(50));
    }

    #[test]
    fn skipped_frame_keeps_a_pending_async_state_bump() {
        // A background load that finished before the skip must still force
        // the dirty walk on the next real paint. Signalled from another
        // thread: a same-thread signal also bumps the local epoch directly,
        // which would hide a clear that swallows the cross-thread counter.
        let before = animation::async_state_epoch();
        std::thread::spawn(animation::signal_async_state_change)
            .join()
            .expect("signal thread");

        frame_skipped();

        assert_ne!(animation::async_state_epoch(), before);
    }
}
