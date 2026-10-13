//! Rotated or sheared text for `LcdGfxCtx`: grayscale outline fill.
//!
//! An LCD mask is an axis-aligned bitmap blitted at the transformed origin, so
//! it cannot follow a turn or a shear.  Such text is shaped into glyph
//! outlines instead and rasterised through the CTM into a grayscale coverage
//! mask (the three subpixels of each pixel averaged, no chroma), which
//! composites into the buffer like any other mask.  `GfxCtx::fill_text` takes
//! the same route for the same transforms, and so does the wgpu backend; the
//! predicate is `crate::text::ctm_needs_outline_text`.

use std::sync::Arc;

use agg_rust::path_storage::PathStorage;

use super::LcdGfxCtx;
use crate::color::Color;
use crate::lcd_coverage::{build_bounded_gray_mask, rect_to_pixel_clip};
use crate::text::{device_baseline_snap, shape_text_snapped, Font};

/// Draw `text` (baseline-left at `(x, y)`) as grayscale outlines in `color`,
/// which already carries the global alpha.
pub(super) fn fill_text_outline(
    ctx: &mut LcdGfxCtx<'_>,
    font: &Arc<Font>,
    text: &str,
    x: f64,
    y: f64,
    color: Color,
) {
    let transform = ctx.state.transform;
    let clip = ctx.state.clip;
    let fill_rule = ctx.state.fill_rule;
    // The baseline snap `GfxCtx` uses, so both contexts place a rotated run
    // on the same baseline.
    let (glyph_paths, _) = shape_text_snapped(font, text, ctx.state.font_size, x, y, |gy| {
        device_baseline_snap(&transform, gy)
    });
    // One mask for the whole run: concatenate the glyphs into a single path.
    let mut run = PathStorage::new();
    for mut glyph in glyph_paths {
        run.concat_path(&mut glyph, 0);
    }
    let buffer = ctx.active_buffer();
    let Some((mask, bbox_x, bbox_y)) = build_bounded_gray_mask(
        buffer.width(),
        buffer.height(),
        &mut run,
        &transform,
        clip,
        fill_rule,
    ) else {
        return;
    };
    let clip_i = clip.map(rect_to_pixel_clip);
    buffer.composite_mask(&mask, color, bbox_x, bbox_y, clip_i);
}
