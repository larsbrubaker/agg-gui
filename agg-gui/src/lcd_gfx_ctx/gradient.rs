//! Gradient fill helpers for `LcdGfxCtx`.
//!
//! This module keeps gradient-specific mask construction out of
//! `lcd_gfx_ctx.rs` while still routing fills through the same LCD coverage
//! mask and per-channel compositing pipeline as solid fills.

use agg_rust::path_storage::PathStorage;
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::draw_ctx::{FillRule, LinearGradientPaint, PatternPaint, RadialGradientPaint};
use crate::lcd_coverage::{rect_to_pixel_clip, LcdBuffer, LcdMaskBuilder};

pub(super) fn fill_linear_gradient(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    gradient: &LinearGradientPaint,
    global_alpha: f32,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
) {
    fill_sampled_gradient(
        buffer,
        path,
        |x, y| gradient.sample(x, y),
        global_alpha,
        transform,
        clip,
        fill_rule,
    );
}

pub(super) fn fill_radial_gradient(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    gradient: &RadialGradientPaint,
    global_alpha: f32,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
) {
    fill_sampled_gradient(
        buffer,
        path,
        |x, y| gradient.sample(x, y),
        global_alpha,
        transform,
        clip,
        fill_rule,
    );
}

pub(super) fn fill_pattern(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    pattern: &PatternPaint,
    global_alpha: f32,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
) {
    fill_sampled_gradient(
        buffer,
        path,
        |x, y| pattern.sample(x, y),
        global_alpha,
        transform,
        clip,
        fill_rule,
    );
}

fn fill_sampled_gradient<F>(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    mut sample: F,
    global_alpha: f32,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
) where
    F: FnMut(f64, f64) -> Color,
{
    let mut builder = LcdMaskBuilder::new(buffer.width(), buffer.height())
        .with_clip(clip)
        .with_fill_rule(fill_rule);
    builder.with_paths(transform, |add| add(path));
    let mask = builder.finalize();
    let clip_i = clip.map(rect_to_pixel_clip);

    buffer.composite_mask_with_color(&mask, 0, 0, clip_i, |dx, dy| {
        let mut lx = dx as f64 + 0.5;
        let mut ly = dy as f64 + 0.5;
        transform.inverse_transform(&mut lx, &mut ly);
        let mut color = sample(lx, ly);
        color.a *= global_alpha;
        color
    });
}
