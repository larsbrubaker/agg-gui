//! One painted frame: surface acquire, the host's frame body, the deferred
//! screenshot read-back, present.
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
