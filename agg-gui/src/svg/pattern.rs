//! SVG pattern paint-server rendering.

use super::*;
use std::sync::Arc;

use crate::draw_ctx::PatternPaint;
use crate::framebuffer::{unpremultiply_rgba_inplace, Framebuffer};
use crate::gfx_ctx::GfxCtx;

pub(super) fn render_pattern_paint(
    pattern: &Arc<usvg::Pattern>,
    opacity: f32,
    object_bbox: Option<usvg::Rect>,
) -> Option<PatternPaint> {
    let rect = pattern.rect();
    let (x, y, width, height) = pattern_rect(rect, object_bbox);
    if width <= f64::EPSILON || height <= f64::EPSILON {
        return None;
    }
    let object_bbox_units = rect.width() <= 1.0 && rect.height() <= 1.0 && object_bbox.is_some();
    let render_x = if object_bbox_units { 0.0 } else { x };
    let render_y = if object_bbox_units { 0.0 } else { y };

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
            -render_x * pixel_width as f64 / width,
            (render_y + height) * pixel_height as f64 / height,
        ));
        render_group(pattern.root(), &mut ctx, SvgRenderState { opacity }).ok()?;
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

fn pattern_rect(rect: usvg::NonZeroRect, object_bbox: Option<usvg::Rect>) -> (f64, f64, f64, f64) {
    let (x, y, w, h) = (
        rect.x() as f64,
        rect.y() as f64,
        rect.width() as f64,
        rect.height() as f64,
    );
    if w <= 1.0 && h <= 1.0 {
        if let Some(bbox) = object_bbox {
            return (
                bbox.x() as f64 + x * bbox.width() as f64,
                bbox.y() as f64 + y * bbox.height() as f64,
                w * bbox.width() as f64,
                h * bbox.height() as f64,
            );
        }
    }
    (x, y, w, h)
}
