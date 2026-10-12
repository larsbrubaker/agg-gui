//! Gradient fill helpers for `LcdGfxCtx`.
//!
//! This module keeps gradient-specific mask construction out of
//! `lcd_gfx_ctx.rs` while still routing fills through the same LCD coverage
//! mask and per-channel compositing pipeline as solid fills, in source-over
//! or, when the blend mode is `CompOp::Src`, copy terms ([`FillSpec::src`]).

use agg_rust::path_storage::PathStorage;

use crate::color::Color;
use crate::draw_ctx::{LinearGradientPaint, PatternPaint, RadialGradientPaint};
use crate::lcd_coverage::{build_bounded_mask, rect_to_pixel_clip, LcdBuffer};

use super::fill::FillSpec;

pub(super) fn fill_linear_gradient(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    gradient: &LinearGradientPaint,
    spec: &FillSpec,
) {
    fill_sampled_gradient(buffer, path, |x, y| gradient.sample(x, y), spec);
}

pub(super) fn fill_radial_gradient(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    gradient: &RadialGradientPaint,
    spec: &FillSpec,
) {
    fill_sampled_gradient(buffer, path, |x, y| gradient.sample(x, y), spec);
}

pub(super) fn fill_pattern(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    pattern: &PatternPaint,
    spec: &FillSpec,
) {
    fill_sampled_gradient(buffer, path, |x, y| pattern.sample(x, y), spec);
}

fn fill_sampled_gradient<F>(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    mut sample: F,
    spec: &FillSpec,
) where
    F: FnMut(f64, f64) -> Color,
{
    // Bound the coverage mask to the transformed path's bbox instead of the
    // whole buffer — same O(bbox) win and pixel-identical output as the
    // solid `fill_path` (see `build_bounded_mask`).  The colour callback
    // receives buffer-space `(dx, dy)`, so the gradient sampling below is
    // unchanged; only the mask's size and composite origin move.
    let transform = spec.transform;
    let Some((mask, bbox_x, bbox_y)) = build_bounded_mask(
        buffer.width(),
        buffer.height(),
        path,
        &transform,
        spec.clip,
        spec.fill_rule,
    ) else {
        return;
    };
    let clip_i = spec.clip.map(rect_to_pixel_clip);
    let global_alpha = spec.global_alpha;
    let color_at = |dx: i32, dy: i32| {
        let mut lx = dx as f64 + 0.5;
        let mut ly = dy as f64 + 0.5;
        transform.inverse_transform(&mut lx, &mut ly);
        let mut color = sample(lx, ly);
        color.a *= global_alpha;
        color
    };
    if spec.src {
        buffer.composite_mask_src_with_color(&mask, bbox_x, bbox_y, clip_i, color_at);
    } else {
        buffer.composite_mask_with_color(&mask, bbox_x, bbox_y, clip_i, color_at);
    }
}
