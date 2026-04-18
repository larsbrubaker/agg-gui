//! LCD subpixel text rasterization — **opaque, bitmap-only** flavour.
//!
//! The `blend_text_lcd` primitive renders text through `PixfmtRgba32Lcd`
//! straight onto the contents of a caller-supplied `Framebuffer`, forcing
//! alpha = 255 on every written pixel.  Callers pre-fill the fb with the
//! target background colour, call this, and get an opaque RGBA swatch with
//! per-channel coverage visible in the RGB channels — the LCD look.
//!
//! # Why opaque-only
//!
//! The alternative two-buffer approach (LCD RGB + separate grayscale
//! alpha) looked tempting but produces a double-multiplied result at edge
//! pixels that dims text incorrectly (see the explanation we walked
//! through earlier).  This module intentionally only offers the opaque
//! path, which is the only formulation that yields correct per-channel
//! LCD chroma blending against a known destination.
//!
//! # Caller contract
//!
//! - `out` **must** already contain the intended background colour.  LCD's
//!   per-channel blend reads from `out`, so if you hand it a zeroed fb the
//!   RGB output is `text_color × cov` — which collapses to black for
//!   dark-on-light (the most common UI case).
//! - This path is for **backbuffers only**.  Direct-to-screen text doesn't
//!   know its own destination bg (it IS the destination), so LCD there
//!   requires dual-source-blend shader work we're deferring.
//!
//! # Pipeline
//!
//! ```text
//! shape_text (rustybuzz kerning + fallback chain — unchanged)
//!   │
//! per-glyph PathStorage → ConvTransform(scale_x_3) → PixfmtRgba32Lcd
//!   │
//! 5-tap LCD distribution kernel writes per-channel coverage × text_color
//! into the pre-filled bg, forces α = 255
//! ```

use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_transform::ConvTransform;
use agg_rust::pixfmt_lcd::{LcdDistributionLut, PixfmtRgba32Lcd};
use agg_rust::rasterizer_scanline_aa::RasterizerScanlineAa;
use agg_rust::renderer_base::RendererBase;
use agg_rust::renderer_scanline::render_scanlines_aa_solid;
use agg_rust::rendering_buffer::RowAccessor;
use agg_rust::scanline_u::ScanlineU8;
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::text::{shape_text, Font};

/// Default weight distribution for the LCD 5-tap kernel.  Matches the
/// agg-rust TrueType-LCD demo and most FreeType defaults:
/// primary 1/3, secondary 2/9, tertiary 1/9.
pub fn default_lut() -> LcdDistributionLut {
    LcdDistributionLut::new(1.0 / 3.0, 2.0 / 9.0, 1.0 / 9.0)
}

/// Build the combined transform used by the LCD raster pass: the caller's
/// CTM followed by a 3× X scale so the path lands in `PixfmtRgba32Lcd`'s
/// 3×-wide subpixel space.
///
/// Column-vector convention: `combined = Scale_3x1 · user`.
fn lcd_transform(user: &TransAffine) -> TransAffine {
    let mut m = *user;
    m.sx  = user.sx  * 3.0;
    m.shx = user.shx * 3.0;
    m.tx  = user.tx  * 3.0;
    // shy, sy, ty pass through unchanged.
    m
}

/// Render `text` through `PixfmtRgba32Lcd` straight onto `out`'s current
/// contents.  Caller **must** pre-fill `out` with the destination
/// background colour — see module docs for why.
///
/// Alpha is forced to 255 on every written pixel (standard LCD pixfmt
/// behaviour).  `out` is returned as an opaque swatch you can blit 1:1.
pub fn blend_text_lcd(
    out:       &mut Framebuffer,
    font:      &Font,
    text:      &str,
    size:      f64,
    x:         f64,
    y:         f64,
    color:     Color,
    transform: &TransAffine,
) {
    let w = out.width();
    let h = out.height();
    if w == 0 || h == 0 { return; }
    let stride = (w * 4) as i32;

    let mut ra = RowAccessor::new();
    unsafe { ra.attach(out.pixels_mut().as_mut_ptr(), w, h, stride) };
    let lut = default_lut();
    let pf  = PixfmtRgba32Lcd::new(&mut ra, &lut);
    let mut rb = RendererBase::new(pf);

    let mut ras = RasterizerScanlineAa::new();
    let mut sl  = ScanlineU8::new();

    let rgba  = color.to_rgba8();
    let xform = lcd_transform(transform);
    let (mut paths, _) = shape_text(font, text, size, x, y);

    for path in paths.iter_mut() {
        let mut curves = ConvCurve::new(path);
        let mut tx     = ConvTransform::new(&mut curves, xform);
        ras.reset();
        ras.add_path(&mut tx, 0);
        render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &rgba);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const FONT_BYTES: &[u8] =
        include_bytes!("../../demo/assets/CascadiaCode.ttf");

    fn font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    /// LCD must blend text colour into a pre-filled white bg and leave
    /// every written pixel opaque (alpha = 255).
    #[test]
    fn test_lcd_blends_onto_prefilled_bg() {
        // Pre-fill white; render BLACK text — the "dark-on-light" case
        // that fails without pre-fill.
        let mut fb = Framebuffer::new(200, 40);
        for px in fb.pixels_mut().chunks_exact_mut(4) {
            px[0] = 255; px[1] = 255; px[2] = 255; px[3] = 255;
        }
        blend_text_lcd(
            &mut fb, &font(), "Hello", 16.0, 4.0, 12.0,
            Color::black(), &TransAffine::new(),
        );

        // Some pixel RGB must be darker than pure white (text blended in).
        let has_dark = fb.pixels().chunks_exact(4)
            .any(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) < 600);
        assert!(has_dark, "LCD render on white bg produced no dark pixels");

        // Every pixel alpha must be 255 (opaque swatch).
        for (i, p) in fb.pixels().chunks_exact(4).enumerate() {
            assert_eq!(p[3], 255, "pixel {i} alpha = {} (must be 255)", p[3]);
        }
    }

    /// AA edge pixels must show per-channel variation — the defining
    /// property of LCD rendering.
    #[test]
    fn test_lcd_has_channel_variation_at_edges() {
        let mut fb = Framebuffer::new(400, 40);
        for px in fb.pixels_mut().chunks_exact_mut(4) {
            px[0] = 255; px[1] = 255; px[2] = 255; px[3] = 255;
        }
        blend_text_lcd(
            &mut fb, &font(), "Wing", 24.0, 4.0, 16.0,
            Color::black(), &TransAffine::new(),
        );

        let mut saw = false;
        for p in fb.pixels().chunks_exact(4) {
            // Darker-than-white edge pixel?
            let sum = p[0] as u32 + p[1] as u32 + p[2] as u32;
            if sum > 60 && sum < 720 { // not pure white, not pure black
                let max = *p[..3].iter().max().unwrap();
                let min = *p[..3].iter().min().unwrap();
                if max - min > 10 { saw = true; break; }
            }
        }
        assert!(saw, "LCD raster produced no per-channel variation at edges");
    }
}
