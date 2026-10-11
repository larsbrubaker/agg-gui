//! SVG `clip-path` coverage for the offscreen composite in `mask.rs`.
//!
//! usvg hands a `<clipPath>` over with its children already reduced to filled
//! paths (black, the `clip-rule` as their fill rule, no stroke) and with
//! `clipPathUnits="objectBoundingBox"` folded into the clip's transform. The
//! clip is rendered in the clipped element's user space, followed by its
//! `transform`, into an offscreen buffer; its alpha is the coverage. A
//! `clip-path` on the `<clipPath>` element itself intersects, as a further
//! coverage multiplied in (resvg's `clip.rs` does the same with tiny-skia
//! masks). Children with their own `clip-path` arrive as groups and are
//! clipped by the ordinary tree walk.

use agg_rust::trans_affine::TransAffine;

use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;

use super::render_tree::render_group;
use super::{to_trans_affine, SvgRenderError, SvgRenderState};

/// Per-pixel coverage in `0.0..=1.0` for `clip` (and any clip linked from it),
/// laid out like a [`Framebuffer`] of `px_w × px_h` (bottom row first).
/// `user_space` maps the clipped element's user space to offscreen pixels.
pub(super) fn clip_coverage(
    clip: &usvg::ClipPath,
    user_space: &TransAffine,
    px_w: u32,
    px_h: u32,
) -> Result<Vec<f32>, SvgRenderError> {
    let mut clip_space = *user_space;
    clip_space.premultiply(&to_trans_affine(clip.transform()));

    let mut buffer = Framebuffer::new(px_w, px_h);
    {
        let mut cctx = GfxCtx::new(&mut buffer);
        cctx.set_transform(clip_space);
        render_group(
            clip.root(),
            &mut cctx,
            SvgRenderState {
                opacity: 1.0,
                layer_width: px_w as f64,
                layer_height: px_h as f64,
                source_cull: None,
                node_to_root: usvg::Transform::identity(),
            },
        )?;
    }

    let mut coverage: Vec<f32> = buffer
        .pixels()
        .chunks_exact(4)
        .map(|p| p[3] as f32 / 255.0)
        .collect();

    if let Some(linked) = clip.clip_path() {
        let linked = clip_coverage(linked, user_space, px_w, px_h)?;
        for (c, l) in coverage.iter_mut().zip(linked) {
            *c *= l;
        }
    }
    Ok(coverage)
}
