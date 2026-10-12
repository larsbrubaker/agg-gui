//! Path fills for `LcdGfxCtx`: solid colours, gradients and patterns, all
//! through the LCD coverage mask (3× supersample, 5-tap filter).
//!
//! The blend mode decides how the mask lands.  `CompOp::Src` copies: each
//! subpixel column moves from the destination to the source by coverage
//! alone ([`LcdBuffer::composite_mask_src_with_color`]), so a translucent
//! colour *replaces* what it covers — what `DrawCtx::clear_clip` and the
//! fractional-edged route of `clear_rect` (`layers.rs`) rely on.  Every
//! other mode composites source-over, as the LCD pipeline always has.
//! Gradient and pattern sampling lives in `gradient.rs`; strokes in
//! `stroke.rs` materialise an outline and fill it the same way.

use agg_rust::comp_op::CompOp;
use agg_rust::path_storage::PathStorage;
use agg_rust::trans_affine::TransAffine;

use super::{gradient, LcdGfxCtx};
use crate::draw_ctx::FillRule;
use crate::lcd_coverage::{build_bounded_mask, rect_to_pixel_clip, LcdBuffer};

/// What every fill of one `fill` call shares: placement, clip, rule, the
/// global alpha the source is scaled by, and whether it copies (`Src`).
pub(super) struct FillSpec {
    pub(super) transform: TransAffine,
    pub(super) clip: Option<(f64, f64, f64, f64)>,
    pub(super) fill_rule: FillRule,
    pub(super) global_alpha: f32,
    /// `CompOp::Src`: replace by coverage instead of blending over.
    pub(super) src: bool,
}

pub(super) fn fill(ctx: &mut LcdGfxCtx<'_>) {
    let spec = FillSpec {
        transform: ctx.state.transform,
        clip: ctx.state.clip,
        fill_rule: ctx.state.fill_rule,
        global_alpha: ctx.state.global_alpha as f32,
        src: ctx.state.blend_mode == CompOp::Src,
    };
    // Borrow gymnastics: a fill needs `&mut path` AND `&mut buffer`, both
    // fields of the ctx.  Take the path out, fill into the active buffer,
    // then put the path back — preserves the "path persists across fill
    // calls" GfxCtx contract.
    let mut path = std::mem::replace(&mut ctx.path, PathStorage::new());
    if let Some(gradient) = ctx.state.fill_linear_gradient.clone() {
        gradient::fill_linear_gradient(ctx.active_buffer(), &mut path, &gradient, &spec);
    } else if let Some(gradient) = ctx.state.fill_radial_gradient.clone() {
        gradient::fill_radial_gradient(ctx.active_buffer(), &mut path, &gradient, &spec);
    } else if let Some(pattern) = ctx.state.fill_pattern.clone() {
        gradient::fill_pattern(ctx.active_buffer(), &mut path, &pattern, &spec);
    } else {
        let mut color = ctx.state.fill_color;
        color.a *= spec.global_alpha;
        fill_solid(ctx.active_buffer(), &mut path, color, &spec);
    }
    ctx.path = path;
}

/// A solid-colour fill of `path`, source-over or (`spec.src`) copy.
pub(super) fn fill_solid(
    buffer: &mut LcdBuffer,
    path: &mut PathStorage,
    color: crate::color::Color,
    spec: &FillSpec,
) {
    if !spec.src {
        buffer.fill_path(path, color, &spec.transform, spec.clip, spec.fill_rule);
        return;
    }
    let Some((mask, bbox_x, bbox_y)) = build_bounded_mask(
        buffer.width(),
        buffer.height(),
        path,
        &spec.transform,
        spec.clip,
        spec.fill_rule,
    ) else {
        return;
    };
    let clip_i = spec.clip.map(rect_to_pixel_clip);
    buffer.composite_mask_src_with_color(&mask, bbox_x, bbox_y, clip_i, |_, _| color);
}
