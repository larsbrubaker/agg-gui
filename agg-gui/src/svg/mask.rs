//! SVG `<mask>` support for the tree visitor in `render_tree.rs`.
//!
//! A masked group is an isolated compositing step: the group's content is
//! rendered into an offscreen software [`Framebuffer`], the mask's content is
//! rendered into a second buffer of the same size, the mask buffer is turned
//! into per-pixel coverage (luminance × alpha for `mask-type="luminance"`, the
//! SVG default; plain alpha for `mask-type="alpha"`), and the content is
//! multiplied by that coverage and by the group opacity.  The result is drawn
//! back into the caller's [`DrawCtx`] with `draw_image_rgba`, so masking works
//! identically on every backend (software RGBA, LCD, wgpu) without any backend
//! needing a mask primitive of its own.
//!
//! The offscreen buffers live in the caller's device space (the current CTM
//! with its translation snapped to whole pixels), so a masked icon drawn at
//! device scale is rasterised at physical resolution and blitted 1:1.
//!
//! Coverage math follows resvg (`resvg/src/mask.rs` + tiny-skia's
//! `Mask::from_pixmap`): luminance uses the sRGB/Rec.709 weights
//! `0.2126 R + 0.7152 G + 0.0722 B` on demultiplied colour, the mask content is
//! clipped to the mask's `x/y/width/height` rectangle, and a linked
//! `mask="..."` on the `<mask>` element multiplies in as a further mask.

use agg_rust::trans_affine::TransAffine;

use crate::draw_ctx::DrawCtx;
use crate::framebuffer::{unpremultiply_rgba_inplace, Framebuffer};
use crate::gfx_ctx::GfxCtx;

use super::render_tree::{render_children, render_group};
use super::{to_trans_affine, SvgRenderError, SvgRenderState};

/// Largest offscreen edge, in pixels, a mask composite allocates.  Larger
/// device-space regions are rendered at reduced resolution and stretched, so a
/// pathological document cannot request gigabytes of scratch memory.
const MAX_MASK_EDGE: f64 = 4096.0;

/// Device-space region a masked group is rendered into.
struct MaskRegion {
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    px_w: u32,
    px_h: u32,
    /// Device space → offscreen pixels.
    device_to_offscreen: TransAffine,
}

/// Render `group` (whose `mask()` is `Some`) through an offscreen mask
/// composite.  `parent_state.opacity` and the group opacity are folded into
/// the composite, after masking, as SVG specifies.
pub(super) fn render_masked_group(
    group: &usvg::Group,
    mask: &usvg::Mask,
    ctx: &mut dyn DrawCtx,
    parent_state: SvgRenderState,
) -> Result<(), SvgRenderError> {
    let alpha = parent_state.opacity * group.opacity().get();
    if alpha <= 0.0 || mask.root().children().is_empty() {
        // An empty mask masks the whole element out.
        return Ok(());
    }

    let canvas = ctx.transform();
    let Some(region) = mask_region(group, mask, &canvas) else {
        return Ok(());
    };

    let mut offscreen_canvas = canvas;
    offscreen_canvas.multiply(&region.device_to_offscreen);

    let offscreen_state = SvgRenderState {
        opacity: 1.0,
        layer_width: region.px_w as f64,
        layer_height: region.px_h as f64,
        source_cull: None,
    };

    let mut content = Framebuffer::new(region.px_w, region.px_h);
    {
        let mut octx = GfxCtx::new(&mut content);
        octx.set_transform(offscreen_canvas);
        render_children(group, &mut octx, offscreen_state)?;
    }

    // Mask content lives in the masked element's user space.
    let mut user_space = offscreen_canvas;
    user_space.premultiply(&to_trans_affine(group.abs_transform()));
    let coverage = mask_coverage(mask, &user_space, region.px_w, region.px_h)?;

    let pixels = content.pixels_mut();
    for (px, cov) in pixels.chunks_exact_mut(4).zip(coverage.iter()) {
        let k = *cov * alpha;
        // Premultiplied RGBA scales linearly in all four channels.
        for c in px.iter_mut() {
            *c = (*c as f32 * k).round().clamp(0.0, 255.0) as u8;
        }
    }

    let mut straight = content.pixels_flipped();
    unpremultiply_rgba_inplace(&mut straight);
    ctx.save();
    ctx.reset_transform();
    ctx.draw_image_rgba(
        &straight,
        region.px_w,
        region.px_h,
        region.x0,
        region.y0,
        region.w,
        region.h,
    );
    ctx.restore();
    Ok(())
}

/// Per-pixel coverage in `0.0..=1.0` for `mask` (and any linked mask), laid
/// out like a [`Framebuffer`] of `px_w × px_h` (bottom row first).
/// `user_space` maps the masked element's user space to offscreen pixels.
fn mask_coverage(
    mask: &usvg::Mask,
    user_space: &TransAffine,
    px_w: u32,
    px_h: u32,
) -> Result<Vec<f32>, SvgRenderError> {
    let mut buffer = Framebuffer::new(px_w, px_h);
    {
        let mut mctx = GfxCtx::new(&mut buffer);
        mctx.set_transform(*user_space);
        // The mask content is clipped to the mask rectangle.  `GfxCtx`
        // resolves a path clip at the matching `restore`, so bracket it.
        let r = mask.rect();
        mctx.save();
        mctx.begin_path();
        mctx.rect(
            r.x() as f64,
            r.y() as f64,
            r.width() as f64,
            r.height() as f64,
        );
        mctx.clip_path();
        render_group(
            mask.root(),
            &mut mctx,
            SvgRenderState {
                opacity: 1.0,
                layer_width: px_w as f64,
                layer_height: px_h as f64,
                source_cull: None,
            },
        )?;
        mctx.restore();
    }

    let kind = mask.kind();
    let mut coverage: Vec<f32> = buffer
        .pixels()
        .chunks_exact(4)
        .map(|p| pixel_coverage(p, kind))
        .collect();

    if let Some(linked) = mask.mask() {
        let linked = mask_coverage(linked, user_space, px_w, px_h)?;
        for (c, l) in coverage.iter_mut().zip(linked) {
            *c *= l;
        }
    }
    Ok(coverage)
}

/// Coverage of one premultiplied RGBA8 mask pixel.
fn pixel_coverage(p: &[u8], kind: usvg::MaskType) -> f32 {
    let a = p[3] as f32 / 255.0;
    match kind {
        usvg::MaskType::Alpha => a,
        usvg::MaskType::Luminance => {
            if p[3] == 0 {
                return 0.0;
            }
            // Demultiply, weight, then re-apply alpha (tiny-skia's order).
            let r = p[0] as f32 / 255.0 / a;
            let g = p[1] as f32 / 255.0 / a;
            let b = p[2] as f32 / 255.0 / a;
            let luma = r * 0.2126 + g * 0.7152 + b * 0.0722;
            (luma * a).clamp(0.0, 1.0)
        }
    }
}

/// Device-space rectangle covering the group's content intersected with the
/// mask rectangle (nothing outside the mask rectangle can show), snapped
/// outward to whole pixels.
fn mask_region(group: &usvg::Group, mask: &usvg::Mask, canvas: &TransAffine) -> Option<MaskRegion> {
    let content = device_bounds(canvas, group.abs_layer_bounding_box().to_rect())?;

    let mut user_space = *canvas;
    user_space.premultiply(&to_trans_affine(group.abs_transform()));
    let mask_rect = device_bounds(&user_space, mask.rect().to_rect())?;

    let x0 = content.0.max(mask_rect.0).floor();
    let y0 = content.1.max(mask_rect.1).floor();
    let x1 = content.2.min(mask_rect.2).ceil();
    let y1 = content.3.min(mask_rect.3).ceil();
    if !(x1 > x0 && y1 > y0) {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let scale = (MAX_MASK_EDGE / w.max(h)).min(1.0);
    let px_w = (w * scale).ceil().max(1.0) as u32;
    let px_h = (h * scale).ceil().max(1.0) as u32;

    let mut device_to_offscreen = TransAffine::new_translation(-x0, -y0);
    device_to_offscreen.multiply(&TransAffine::new_scaling(px_w as f64 / w, px_h as f64 / h));
    Some(MaskRegion {
        x0,
        y0,
        w,
        h,
        px_w,
        px_h,
        device_to_offscreen,
    })
}

/// Axis-aligned `(min_x, min_y, max_x, max_y)` of `rect` under `t`.
fn device_bounds(t: &TransAffine, rect: usvg::Rect) -> Option<(f64, f64, f64, f64)> {
    let (x, y) = (rect.x() as f64, rect.y() as f64);
    let (w, h) = (rect.width() as f64, rect.height() as f64);
    let mut b = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for (mut cx, mut cy) in [(x, y), (x + w, y), (x + w, y + h), (x, y + h)] {
        t.transform(&mut cx, &mut cy);
        b = (b.0.min(cx), b.1.min(cy), b.2.max(cx), b.3.max(cy));
    }
    (b.0.is_finite() && b.1.is_finite() && b.2.is_finite() && b.3.is_finite()).then_some(b)
}
