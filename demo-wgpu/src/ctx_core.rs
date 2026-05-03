//! Core helper methods on [`WgpuGfxCtx`] that are used throughout the `DrawCtx`
//! implementation but do not belong to the public drawing API.
//!
//! Mirrors the role of `ctx_core.rs` in `demo-gl`, providing accessors for the
//! current transform, scissor rect, and point transformation.  Unlike the GL
//! version, scissor application is deferred — there is no `apply_scissor` that
//! pokes GPU state.  The current scissor rect is carried in
//! `state_stack.last().1` and applied per-draw-command inside `end_frame`.

use super::*;

impl WgpuGfxCtx {
    /// Return a shared reference to the current transform (CTM).
    #[inline]
    pub(crate) fn ctm(&self) -> &TransAffine {
        &self.state_stack.last().unwrap().0
    }

    /// Return a mutable reference to the current transform.
    #[inline]
    pub(crate) fn ctm_mut(&mut self) -> &mut TransAffine {
        &mut self.state_stack.last_mut().unwrap().0
    }

    /// Return the current scissor rect in Y-up screen-space coordinates
    /// `[x, y_bottom, w, h]`, or `None` when no clip is active.
    #[inline]
    pub(crate) fn current_clip(&self) -> Option<[i32; 4]> {
        self.state_stack.last().unwrap().1
    }

    /// No-op stub: in the wgpu backend, scissor state is deferred to
    /// `end_frame` rather than applied immediately.  Kept so call sites
    /// ported from the GL backend compile without changes; will be
    /// replaced by a proper deferred-command emission in Phase 4.
    #[inline]
    pub(crate) fn apply_scissor(&mut self) {}

    /// Transform a point `(x, y)` through the current CTM, returning the
    /// result as `[f32; 2]` for direct use in vertex buffers.
    #[inline]
    pub(crate) fn transform_pt(&self, x: f64, y: f64) -> [f32; 2] {
        let (mut px, mut py) = (x, y);
        self.ctm().transform(&mut px, &mut py);
        [px as f32, py as f32]
    }

    /// Convert a Y-up screen-space bounding box to a stored scissor rect
    /// `[x, y_bottom, w, h]` in integer screen pixels.
    ///
    /// The wgpu backend uses Y-down framebuffer coordinates for the actual
    /// scissor call; the conversion happens in `end_frame` when the viewport
    /// height is known.  Storing in Y-up form here is consistent with the
    /// `state_stack` convention and with the GL backend.
    pub(crate) fn compute_scissor(lx: f64, by: f64, rx: f64, ty: f64) -> [i32; 4] {
        // Clamp before cast: layout can produce f64::MAX/2 for unbounded sizes.
        const LO: f64 = i32::MIN as f64;
        const HI: f64 = i32::MAX as f64;
        let x = lx.floor().clamp(LO, HI) as i32;
        let y = by.floor().clamp(LO, HI) as i32;
        let w = (rx - lx).ceil().clamp(0.0, HI) as i32;
        let h = (ty - by).ceil().clamp(0.0, HI) as i32;
        [x, y, w, h]
    }

    /// Convert a Y-up scissor rect `[x, y_bottom, w, h]` (as stored in
    /// `state_stack`) to wgpu's Y-down framebuffer convention
    /// `(x, y_top, w, h)` given the current viewport height.
    ///
    /// Called in `end_frame` when emitting each scissored draw command.
    #[inline]
    pub(crate) fn yup_to_ydown_scissor(
        scissor: [i32; 4],
        viewport_h: u32,
    ) -> (u32, u32, u32, u32) {
        let [x, y_bottom, w, h] = scissor;
        let y_top = (viewport_h as i32) - (y_bottom + h);
        (
            x.max(0) as u32,
            y_top.max(0) as u32,
            w.max(0) as u32,
            h.max(0) as u32,
        )
    }
}
