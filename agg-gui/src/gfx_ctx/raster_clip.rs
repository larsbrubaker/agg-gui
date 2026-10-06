//! Rasterizer-level clipping for the software draw paths.
//!
//! The software paths in `draw_impl.rs` (`rasterize_fill`,
//! `rasterize_stroke`), `GfxCtx::fill_text_gsv` and the LCD coverage mask
//! (`lcd_coverage/mask.rs`) clip pixels at the `RendererBase` (see
//! `apply_clip`).  That alone keeps writes inside the scissor, but the AGG
//! cell rasterizer still walks every pixel cell along each edge — a line
//! whose endpoint is 200 000 px off screen builds hundreds of thousands of
//! cells that are then sorted and thrown away.  AGG's own answer (and what
//! C# agg's renderers do) is `RasterizerScanlineAa::clip_box`: edges are
//! clipped in 24.8 fixed point before any cell is generated.  Left/right
//! overhangs are replaced by vertical edges on the clip boundary so winding
//! (and therefore coverage) to the right of them is preserved; top/bottom
//! overhangs are dropped because those scanlines are never swept.
//!
//! The box is the active scissor intersected with the framebuffer, in device
//! pixels (all draw paths transform before rasterizing), grown by
//! [`RASTER_CLIP_MARGIN`] so the synthetic boundary edges land in a column
//! the renderer already discards.

use agg_rust::rasterizer_scanline_aa::RasterizerScanlineAa;

/// Extra device pixels kept around the clip box.  The renderer clip stays
/// exact, so this only moves the rasterizer's synthetic boundary edges out
/// of the visible pixels.
pub(crate) const RASTER_CLIP_MARGIN: f64 = 1.0;

/// The rasterizer clip box `(x1, y1, x2, y2)` in device pixels for a
/// `width × height` target and an optional Y-up scissor `(x, y, w, h)`.
///
/// The scissor is snapped outward to whole pixels exactly like
/// `apply_clip` does for the renderer, intersected with the target, then
/// grown by [`RASTER_CLIP_MARGIN`].  An empty intersection yields a
/// degenerate box (x2 == x1 or y2 == y1), which rasterizes nothing.
pub(crate) fn raster_clip_rect(
    clip: Option<(f64, f64, f64, f64)>,
    width: u32,
    height: u32,
) -> (f64, f64, f64, f64) {
    let (mut x1, mut y1, mut x2, mut y2) = (0.0, 0.0, width as f64, height as f64);
    if let Some((cx, cy, cw, ch)) = clip {
        x1 = cx.floor().max(x1);
        y1 = cy.floor().max(y1);
        x2 = (cx + cw).ceil().min(x2);
        y2 = (cy + ch).ceil().min(y2);
    }
    // NaN-safe: a NaN scissor compares false and leaves the target bounds.
    let x2 = x2.max(x1);
    let y2 = y2.max(y1);
    (
        x1 - RASTER_CLIP_MARGIN,
        y1 - RASTER_CLIP_MARGIN,
        x2 + RASTER_CLIP_MARGIN,
        y2 + RASTER_CLIP_MARGIN,
    )
}

/// Restrict `ras` to the visible area of a `width × height` target under
/// `clip`.  Must be called before any path is added: AGG's `clip_box`
/// resets the rasterizer.
pub(crate) fn clip_rasterizer(
    ras: &mut RasterizerScanlineAa,
    clip: Option<(f64, f64, f64, f64)>,
    width: u32,
    height: u32,
) {
    let (x1, y1, x2, y2) = raster_clip_rect(clip, width, height);
    ras.clip_box(x1, y1, x2, y2);
}
