//! One painted frame: surface acquire, the host's frame body, the deferred
//! screenshot read-back, present — and the retry wake for a frame the surface
//! refused ([`schedule_skipped_frame`]).
//!
//! Split out of `run.rs` so the event loop file is about events. The state
//! that only the paint path cares about — the render context, the layout-skip
//! key, the frame counter — lives in [`Painter`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui::App;
use agg_gui_wgpu::{FrameAcquire, Gpu, RetryWake, WgpuGfxCtx};
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
    /// The surface refused the frame because a failed configure is backing
    /// off, and the retry is due at this time. The loop waits for it instead
    /// of polling — see `redraw_schedule::next_control_flow`.
    pub(crate) retry_at: Option<Instant>,
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
    /// A frame the surface refuses is skipped with its retry arranged by
    /// [`schedule_skipped_frame`]. A surface that stays unusable past the
    /// retry budget is [`ShellError::Surface`], which ends the loop.
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
        // Never zero: `Gpu` clamps the configured size to at least 1x1 and
        // ignores zero-sized resizes. (A minimized window with an
        // unconfigured surface is not painted at all — `ShellLoop::paint`
        // stops before this, see `redraw_schedule::paint_blocked`.)
        let (win_w, win_h) = (gpu.config().width, gpu.config().height);

        let started = Instant::now();
        let surface_frame = match gpu.try_acquire_frame().map_err(ShellError::Surface)? {
            FrameAcquire::Frame(frame) => frame,
            FrameAcquire::Skip(wake) => {
                schedule_skipped_frame(wake, || window.request_redraw());
                return Ok(PaintOutcome {
                    retry_at: retry_deadline(wake, Instant::now()),
                    ..PaintOutcome::default()
                });
            }
            // A variant added by a later agg-gui-wgpu: skip, retry next frame.
            _ => {
                schedule_skipped_frame(RetryWake::Now, || window.request_redraw());
                return Ok(PaintOutcome::default());
            }
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
                self.ctx.present(surface_frame);
                result.map_err(ShellError::Screenshot)?;
                self.last_duration = started.elapsed();
                return Ok(PaintOutcome {
                    painted: true,
                    captured: true,
                    retry_at: None,
                });
            }
        }

        // Drops the ctx's back-buffer clone before presenting (DX12 resize
        // fails otherwise) — see `WgpuGfxCtx::present`.
        self.ctx.present(surface_frame);
        self.last_duration = started.elapsed();
        Ok(PaintOutcome {
            painted: true,
            captured: false,
            retry_at: None,
        })
    }
}

/// Arrange the retry for a frame the surface refused, then drop the
/// immediate draw request.
///
/// `Gpu::try_acquire_frame` says when to come back ([`RetryWake`]); the
/// shell owns the loop, so it turns that into a wake: `Now` → a window
/// redraw, `After(d)` → an agg-gui draw deadline (which `about_to_wait`
/// turns into `ControlFlow::WaitUntil`), `OnEvent` → nothing (an occluded
/// window is woken by `WindowEvent::Occluded(false)`).
///
/// The immediate request is cleared because only `App::paint` clears it and
/// a skipped frame never gets there: left set, `wants_draw()` stays true and
/// the loop sits in `ControlFlow::Poll`, spinning while a failed surface
/// configure backs off. The narrow clear leaves scheduled deadlines and
/// pending cross-thread wakeups alone.
pub(crate) fn schedule_skipped_frame(wake: RetryWake, request_redraw: impl FnOnce()) {
    match wake {
        RetryWake::Now => request_redraw(),
        RetryWake::After(delay) => agg_gui::animation::request_draw_after(delay),
        RetryWake::OnEvent => {}
        // A wake kind added by a later agg-gui-wgpu: retrying on the next
        // frame can cost a few wasted frames but never strands the window.
        _ => request_redraw(),
    }
    agg_gui::animation::clear_immediate_draw_request();
}

/// When a skipped frame's retry is due, for a backed-off surface configure
/// (`RetryWake::After`); `None` for the wakes that do not hold the loop.
pub(crate) fn retry_deadline(wake: RetryWake, now: Instant) -> Option<Instant> {
    match wake {
        RetryWake::After(delay) => Some(now + delay),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{retry_deadline, schedule_skipped_frame};
    use agg_gui::animation;
    use agg_gui_wgpu::RetryWake;
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    /// Animation state is thread-local; start each test from a clean slate
    /// with a pending immediate request, as a frame the app asked for.
    fn app_wants_a_frame() {
        animation::clear_draw_request();
        animation::request_draw();
    }

    #[test]
    fn retry_now_requests_a_redraw() {
        app_wants_a_frame();
        let redraws = Cell::new(0);
        schedule_skipped_frame(RetryWake::Now, || redraws.set(redraws.get() + 1));
        assert_eq!(redraws.get(), 1, "an immediate retry needs a redraw");
        assert!(
            !animation::peek_draw_signals().0,
            "the immediate request must be dropped, or the loop polls"
        );
        assert_eq!(animation::peek_next_draw_deadline(), None);
    }

    #[test]
    fn retry_after_schedules_a_deadline_instead_of_polling() {
        app_wants_a_frame();
        let delay = Duration::from_millis(400);
        let before = Instant::now();
        let redraws = Cell::new(0);
        schedule_skipped_frame(RetryWake::After(delay), || redraws.set(redraws.get() + 1));
        let after = Instant::now();
        assert_eq!(redraws.get(), 0, "a backed-off retry must not redraw now");
        assert!(
            !animation::peek_draw_signals().0,
            "the immediate request must be dropped, or the loop polls"
        );
        let deadline = animation::peek_next_draw_deadline().expect("retry deadline armed");
        assert!(deadline >= before + delay && deadline <= after + delay);
    }

    #[test]
    fn retry_on_event_arranges_nothing() {
        app_wants_a_frame();
        let redraws = Cell::new(0);
        schedule_skipped_frame(RetryWake::OnEvent, || redraws.set(redraws.get() + 1));
        assert_eq!(redraws.get(), 0);
        assert!(!animation::peek_draw_signals().0);
        assert_eq!(animation::peek_next_draw_deadline(), None);
    }

    #[test]
    fn only_a_backed_off_retry_reports_a_deadline() {
        let now = Instant::now();
        let d = Duration::from_millis(200);
        assert_eq!(retry_deadline(RetryWake::After(d), now), Some(now + d));
        assert_eq!(retry_deadline(RetryWake::Now, now), None);
        assert_eq!(retry_deadline(RetryWake::OnEvent, now), None);
    }

    #[test]
    fn skipped_frame_keeps_an_animation_deadline() {
        // A running animation's next tick must survive a refused frame.
        app_wants_a_frame();
        animation::request_draw_after(Duration::from_secs(60));
        let deadline = animation::peek_next_draw_deadline();
        schedule_skipped_frame(RetryWake::OnEvent, || {});
        assert_eq!(animation::peek_next_draw_deadline(), deadline);
    }
}
