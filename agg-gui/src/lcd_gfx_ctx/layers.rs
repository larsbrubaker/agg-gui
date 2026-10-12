//! Compositing layers, the rounded layer clip and the plane-fill
//! `clear_rect` for [`LcdGfxCtx`]. The `DrawCtx` impl in `lcd_gfx_ctx.rs`
//! delegates here.
//!
//! A layer is a fresh [`LcdBuffer`] **seeded with the parent's pixels** under
//! it (an LCD buffer composites per subpixel column, so there is no single
//! alpha that could tell "untouched" from "painted"). Popping it blends the
//! layer back by a per-pixel weight, `alpha × rounded-clip coverage`, which
//! for a seeded layer is exactly `(weight × content) over parent`
//! ([`LcdBuffer::blend_seeded_layer`]).
//!
//! The rounded clip ports agg-sharp's `RoundedClipCoverage` (agg/
//! `RoundedClipCoverage.cs`): 1 inside the rounded rectangle, 0 outside, a
//! linear ramp over the 1 pixel band centred on the edge, sampled at pixel
//! centres. agg-sharp scales a rounded panel's pixels by that coverage
//! before they land in an LCD buffer, whole pixels at a time, so the
//! panel's subpixel text keeps its chroma; the blend here does the same.
//! `GfxCtx` (`gfx_ctx/layers.rs`) is the RGBA counterpart.

use agg_rust::trans_affine::TransAffine;

use super::{LcdGfxCtx, LcdLayer};
use crate::color::Color;
use crate::lcd_coverage::{rect_to_pixel_clip, LcdBuffer};

/// A rounded clip in a layer buffer's own pixels (Y-up).
#[derive(Clone, Copy, Debug)]
pub(super) struct RoundedClip {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
    radius: f64,
}

impl RoundedClip {
    /// agg-sharp `RoundedClipCoverage.Coverage`: how much of the pixel
    /// centred on `(x, y)` the clip keeps.
    fn coverage(&self, x: f64, y: f64) -> f64 {
        let width = self.right - self.left;
        let height = self.top - self.bottom;
        let radius = clamp_radius(width, height, self.radius);
        // Signed distance to a rounded box: the distance from the
        // radius-shrunk box, less the radius.
        let dx = (x - (self.left + self.right) / 2.0).abs() - (width / 2.0 - radius);
        let dy = (y - (self.bottom + self.top) / 2.0).abs() - (height / 2.0 - radius);
        let ox = dx.max(0.0);
        let oy = dy.max(0.0);
        let signed_distance = (ox * ox + oy * oy).sqrt() + dx.max(dy).min(0.0) - radius;
        (0.5 - signed_distance).clamp(0.0, 1.0)
    }
}

/// agg-sharp `RoundedClipCoverage.ClampRadius`: never negative and never
/// more than half the shorter side.
fn clamp_radius(width: f64, height: f64, radius: f64) -> f64 {
    radius.min(width.min(height) / 2.0).max(0.0)
}

impl LcdGfxCtx<'_> {
    /// Begin a layer of `width × height` logical units at the CTM's
    /// translation. The CTM's scale is kept, so a HiDPI subtree rasters at
    /// physical resolution inside the layer (as `GfxCtx::push_layer_with_alpha`).
    pub(super) fn push_layer_impl(&mut self, width: f64, height: f64, alpha: f64) {
        let origin_x = self.state.transform.tx;
        let origin_y = self.state.transform.ty;
        let (scale_x, scale_y) = self.state.transform.scaling_abs();
        let scale_x = scale_x.max(1e-6);
        let scale_y = scale_y.max(1e-6);
        let lw = (width * scale_x).ceil().max(1.0) as u32;
        let lh = (height * scale_y).ceil().max(1.0) as u32;
        let mut layer_buffer = LcdBuffer::new(lw, lh);

        // Seed the layer with the parent's pixels at the layer's bounds, so
        // the pop's blend can treat the layer as "content over parent".
        let dx = -(origin_x.round() as i32);
        let dy = -(origin_y.round() as i32);
        let parent_ref: &LcdBuffer = if let Some(layer) = self.layer_stack.last() {
            &layer.buffer
        } else {
            &*self.base_buffer
        };
        layer_buffer.blend_seeded_layer(parent_ref, dx, dy, None, |_, _| 1.0);

        let saved_state = self.state.clone();
        let saved_stack = std::mem::take(&mut self.state_stack);
        self.layer_stack.push(LcdLayer {
            buffer: layer_buffer,
            saved_state,
            saved_stack,
            origin_x,
            origin_y,
            alpha: alpha.clamp(0.0, 1.0),
            rounded_clip: None,
        });
        // Drawing inside the layer starts at the layer's bottom-left, at the
        // parent's device scale, with no clip.
        self.state.transform = TransAffine::new_scaling(scale_x, scale_y);
        self.state.clip = None;
    }

    /// Record the rounded rectangle `(x, y, w, h)` with corner radius `r`,
    /// in the current (layer-local) coordinates, as the current layer's
    /// clip; `pop_layer` keeps only what lies inside it. Outside a layer
    /// there is nothing to clip.
    pub(super) fn set_layer_rounded_clip_impl(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let ctm = self.state.transform;
        let Some(layer) = self.layer_stack.last_mut() else {
            return;
        };
        let (mut x1, mut y1) = (x, y);
        let (mut x2, mut y2) = (x + w, y + h);
        ctm.transform(&mut x1, &mut y1);
        ctm.transform(&mut x2, &mut y2);
        let (scale_x, scale_y) = ctm.scaling_abs();
        layer.rounded_clip = Some(RoundedClip {
            left: x1.min(x2),
            bottom: y1.min(y2),
            right: x1.max(x2),
            top: y1.max(y2),
            radius: r.max(0.0) * scale_x.min(scale_y),
        });
    }

    /// Blend the current layer back into the previous target by its alpha
    /// and rounded clip, inside the parent clip, then restore the state
    /// that was active at the matching push.
    pub(super) fn pop_layer_impl(&mut self) {
        let Some(layer) = self.layer_stack.pop() else {
            return;
        };
        self.state = layer.saved_state;
        self.state_stack = layer.saved_stack;
        // The origin is in the parent's pixels; round so the layer lands on
        // the integer grid (same reason `draw_lcd_mask` rounds).
        let dst_x = layer.origin_x.round() as i32;
        let dst_y = layer.origin_y.round() as i32;
        let clip_i = self.state.clip.map(rect_to_pixel_clip);
        let alpha = layer.alpha as f32;
        let rounded = layer.rounded_clip;
        let buffer = layer.buffer;
        self.active_buffer()
            .blend_seeded_layer(&buffer, dst_x, dst_y, clip_i, |sx, sy| {
                let cov = rounded.map_or(1.0, |c| c.coverage(sx as f64 + 0.5, sy as f64 + 0.5));
                alpha * cov as f32
            });
    }

    /// `DrawCtx::clear_rect` as a direct plane fill: the rectangle (in the
    /// active buffer's own pixels) and the current clip, when both sit on
    /// whole pixels, are written straight into the planes. A fractional
    /// edge takes the trait's default route (a clipped full-coverage fill),
    /// whose edge pixels are partly covered.
    pub(super) fn clear_rect_impl(&mut self, x: f64, y: f64, w: f64, h: f64, color: Color) {
        let mut x1 = x;
        let mut y1 = y;
        let mut x2 = x + w;
        let mut y2 = y + h;
        if let Some((cx, cy, cw, ch)) = self.state.clip {
            x1 = x1.max(cx);
            y1 = y1.max(cy);
            x2 = x2.min(cx + cw);
            y2 = y2.min(cy + ch);
        }
        if !(x2 > x1 && y2 > y1) {
            return;
        }
        let whole = [x1, y1, x2, y2].iter().all(|v| v.fract() == 0.0);
        if whole {
            self.active_buffer()
                .fill_rect_src((x1 as i32, y1 as i32, x2 as i32, y2 as i32), color);
            // The trait's contract: the current path is discarded.
            self.path = agg_rust::path_storage::PathStorage::new();
            return;
        }
        crate::draw_ctx::DrawCtx::save(self);
        crate::draw_ctx::DrawCtx::reset_transform(self);
        crate::draw_ctx::DrawCtx::clip_rect(self, x, y, w, h);
        crate::draw_ctx::defaults::clear_clip(self, color);
        crate::draw_ctx::DrawCtx::restore(self);
    }
}
