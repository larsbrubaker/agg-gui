//! Solid scanline rendering with anti-aliasing switched off, for the free
//! rasterization helpers in `draw_impl.rs`.
//!
//! AGG draws aliased shapes by giving the rasterizer a `gamma_threshold(0.5)`
//! table: a pixel whose area coverage is at least half is drawn fully, any
//! other not at all. agg-rust's rasterizer is the "nogamma" variant, so the
//! threshold is applied here to each span's covers before they are blended.
//! SVG's `shape-rendering="crispEdges"`/`"optimizeSpeed"` reach this through
//! `DrawCtx::set_anti_alias`.

use agg_rust::pixfmt_rgba::PixelFormat;
use agg_rust::rasterizer_scanline_aa::{RasterizerScanlineAa, Scanline};
use agg_rust::renderer_base::RendererBase;
use agg_rust::renderer_scanline::render_scanlines_aa_solid;
use agg_rust::scanline_u::ScanlineU8;

/// Coverage at or above this is drawn fully when anti-aliasing is off
/// (AGG's `gamma_threshold(0.5)`: `cover / 255 < 0.5` draws nothing).
const ALIASED_THRESHOLD: u8 = 128;

/// Render the rasterizer's scanlines in `color`, anti-aliased or not.
pub(super) fn render_scanlines_solid<PF: PixelFormat>(
    ras: &mut RasterizerScanlineAa,
    sl: &mut ScanlineU8,
    ren: &mut RendererBase<PF>,
    color: &PF::ColorType,
    anti_alias: bool,
) {
    if anti_alias {
        render_scanlines_aa_solid(ras, sl, ren, color);
        return;
    }
    if !ras.rewind_scanlines() {
        return;
    }
    sl.reset(ras.min_x(), ras.max_x());
    let mut covers = Vec::new();
    while ras.sweep_scanline(sl) {
        let y = sl.y();
        for span in sl.begin() {
            if span.len <= 0 {
                continue;
            }
            let len = span.len as usize;
            covers.clear();
            covers.extend(
                sl.covers()[span.cover_offset..span.cover_offset + len]
                    .iter()
                    .map(|&c| if c >= ALIASED_THRESHOLD { 255 } else { 0 }),
            );
            ren.blend_solid_hspan(span.x, y, span.len, color, &covers);
        }
    }
}
