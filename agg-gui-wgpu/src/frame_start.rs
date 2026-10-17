//! Starting a frame on [`WgpuGfxCtx`]: [`WgpuGfxCtx::reset`] (drawing state
//! back to defaults), [`WgpuGfxCtx::begin_frame`] (take the render target and
//! queue the theme's clear) and [`WgpuGfxCtx::begin_overlay_frame`] (take the
//! render target and keep its pixels).
//!
//! The frame's clear is the first entry of the deferred command list, which
//! `end_frame.rs` turns into the first render pass's `LoadOp::Clear`; a frame
//! whose first command is not a clear loads whatever the target held. Shells
//! call `reset` either before `begin_frame` or after it (the shells'
//! `default_paint` resets inside the frame body), so `reset` re-queues the
//! open frame's clear rather than dropping it with the rest of the commands.
//! Otherwise a window would start every frame from the swap-chain image's old
//! contents: transparent black in a browser, trails of earlier frames under
//! anything animated.

use super::*;

impl WgpuGfxCtx {
    /// Reset drawing state for a new frame.  Preserves GPU resources.
    ///
    /// Drops every queued draw command except the open frame's clear: called
    /// after [`Self::begin_frame`], the frame still starts from the
    /// background; called after [`Self::begin_overlay_frame`] (or before any
    /// frame is begun), nothing is queued.
    pub fn reset(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.fill_color = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.fill_linear_gradient = None;
        self.fill_radial_gradient = None;
        self.stroke_color = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.stroke_linear_gradient = None;
        self.stroke_radial_gradient = None;
        self.line_width = 1.0;
        self.fill_rule = FillRule::NonZero;
        self.miter_limit = 4.0;
        self.line_dash.clear();
        self.dash_offset = 0.0;
        self.global_alpha = 1.0;
        self.blend_mode = CompOp::SrcOver;
        self.paint_state_stack.clear();
        self.coverage_pass = false;
        self.state_stack = vec![(TransAffine::new(), None)];
        self.path = PathStorage::new();
        self.font = None;
        self.font_size = 16.0;
        self.commands.clear();
        if let Some(clear) = self.frame_clear {
            self.commands.push(DrawCommand::Clear(clear));
        }
        self.layer_stack.clear();
    }

    /// Start a frame: take ownership of this frame's render target and queue
    /// the clear.
    ///
    /// `view` is the `wgpu::TextureView` for the surface texture (or any other
    /// target) being painted this frame.  It is stashed on the context so that
    /// mid-frame GPU work — custom render passes, layer compositing — targets
    /// the same attachment as the deferred 2-D pipeline without the caller
    /// plumbing it through every method; [`Self::end_frame`] consumes it.
    ///
    /// The clear itself happens inside `end_frame`, when the leading
    /// `DrawCommand::Clear` is flushed into the first render pass; this
    /// method only puts the current theme's background colour at the head of
    /// the command list so the frame starts from a clean framebuffer. The
    /// clear survives a [`Self::reset`] made inside the frame.
    pub fn begin_frame(&mut self, view: wgpu::TextureView) {
        let bg = agg_gui::current_visuals().bg_color;
        self.begin_frame_cleared(view, bg);
    }

    /// [`Self::begin_frame`] with the clear colour stated instead of taken
    /// from the theme — an offscreen frame ([`headless::HeadlessFrame`])
    /// whose starting pixels a test pins.
    pub(crate) fn begin_frame_cleared(&mut self, view: wgpu::TextureView, clear: Color) {
        self.open_frame(view);
        self.frame_clear = Some(clear);
        self.queue_leading_clear(clear);
    }

    /// Start a frame that paints over the target's current pixels: the
    /// target is taken as [`Self::begin_frame`] takes it, but no clear is
    /// queued and [`Self::reset`] adds none, so the first render pass loads
    /// what the target holds. For a pass drawn on top of a finished frame
    /// before it is presented (a simulated mouse pointer, a debug overlay).
    /// A clear queued before this call (by a `reset` of a frame never ended)
    /// is dropped, so the target's pixels are always kept.
    pub fn begin_overlay_frame(&mut self, view: wgpu::TextureView) {
        self.open_frame(view);
        self.frame_clear = None;
        if self.only_clears_queued() {
            self.commands.clear();
        }
    }

    /// Queue `color` as the frame's clear: replaces the clears queued so far
    /// when nothing has been drawn yet (only a leading clear is honoured, so
    /// the newest one before any drawing is the one that should win), and is
    /// otherwise appended, where the flush ignores it.
    pub(crate) fn queue_leading_clear(&mut self, color: Color) {
        if self.only_clears_queued() {
            self.commands.clear();
        }
        self.commands.push(DrawCommand::Clear(color));
    }

    /// No draw command queued yet: the list is empty or holds only clears.
    fn only_clears_queued(&self) -> bool {
        self.commands
            .iter()
            .all(|c| matches!(c, DrawCommand::Clear(_)))
    }

    /// The part of beginning a frame both kinds share: stash the target,
    /// start the texture caches' sweep frame, and take the LCD text setting.
    fn open_frame(&mut self, view: wgpu::TextureView) {
        self.surface_view = Some(view);
        self.dead_sweeps.begin_frame();
        // Sync the per-ctx LCD flag from the global typography setting so
        // any subsequent `fill_text` routes through the cached subpixel mask
        // path when LCD is enabled (default at scale ≤ 1.25; an explicit choice at any scale).  Without this
        // shells that drive the loop via `begin_frame` instead of a helper
        // that sets it silently fall through to the aliased tessellated-
        // outline path even when the global says LCD is on.
        self.set_lcd_mode(agg_gui::font_settings::lcd_enabled());
    }
}
