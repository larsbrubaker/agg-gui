//! SVG pattern paint-server rendering.

use super::*;
use std::sync::Arc;

use crate::draw_ctx::PatternPaint;
use crate::framebuffer::{unpremultiply_rgba_inplace, Framebuffer};
use crate::gfx_ctx::GfxCtx;

/// Render `pattern`'s tile and wrap it as a [`PatternPaint`]. usvg has already
/// resolved objectBoundingBox `patternUnits`/`patternContentUnits` and the
/// viewBox into user space, so `pattern.rect()` is the tile in user space and
/// the content is drawn in the tile's own space, whose origin is the tile's
/// (x, y) corner (SVG's pattern coordinate system).
pub(super) fn render_pattern_paint(
    pattern: &Arc<usvg::Pattern>,
    opacity: f32,
) -> Option<PatternPaint> {
    let rect = pattern.rect();
    let (x, y, width, height) = (
        rect.x() as f64,
        rect.y() as f64,
        rect.width() as f64,
        rect.height() as f64,
    );
    if width <= f64::EPSILON || height <= f64::EPSILON {
        return None;
    }

    let pixel_width = width.ceil().clamp(1.0, 4096.0) as u32;
    let pixel_height = height.ceil().clamp(1.0, 4096.0) as u32;
    let mut tile = Framebuffer::new(pixel_width, pixel_height);
    {
        let mut ctx = GfxCtx::new(&mut tile);
        ctx.set_transform(TransAffine::new_custom(
            pixel_width as f64 / width,
            0.0,
            0.0,
            -(pixel_height as f64 / height),
            0.0,
            pixel_height as f64,
        ));
        render_group(
            pattern.root(),
            &mut ctx,
            SvgRenderState {
                opacity,
                layer_width: pixel_width as f64,
                layer_height: pixel_height as f64,
                source_cull: None,
            },
        )
        .ok()?;
    }

    let mut pixels = tile.into_pixels();
    unpremultiply_rgba_inplace(&mut pixels);
    Some(PatternPaint {
        x,
        y,
        width,
        height,
        transform: to_trans_affine(pattern.transform()),
        pixels: Arc::new(pixels),
        pixel_width,
        pixel_height,
    })
}
