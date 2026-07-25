//! Regression bound on how much **perceived ink** LCD subpixel text deposits
//! relative to grayscale text for the same string.
//!
//! This guards the "LCD text looks bolder than grayscale text" axis. It takes
//! the LCD and grayscale masks produced by `lcd_coverage::mask` and composites
//! both onto white through the shared, production `composite_lcd_mask`, so the
//! only variables under test are the 5-tap filter and the composite itself.
//!
//! The metric is **total linear-light ink**: `Σ (1 - Y)` over pixels, where `Y`
//! is Rec.709 luminance after decoding sRGB with a 2.2 power curve. It answers
//! "how much darkness landed on screen", which is what the eye integrates when
//! judging weight — unlike a raw coverage sum, which is blind to the blend
//! space.
//!
//! Split out of `lcd_coverage/tests.rs` so that file stays under the project's
//! 800-line cap.

use std::sync::Arc;

use super::*;
use crate::text::Font;

const CASCADIA_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const NUNITO_BYTES: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

fn cascadia_font() -> Arc<Font> {
    Arc::new(Font::from_slice(CASCADIA_BYTES).expect("cascadia font"))
}

/// Proportional sans with thinner stems than CascadiaCode — the face the
/// demo's body text actually uses, and where mid-coverage pixels dominate.
fn nunito_font() -> Arc<Font> {
    Arc::new(Font::from_slice(NUNITO_BYTES).expect("nunito font"))
}

const DIAG_TEXT: &str = "Renders text using per-channel R/G/B coverage";
const DIAG_SIZE: f64 = 14.0;

/// Restore every typography thread-local to its default so leakage from
/// another test that ran earlier on this thread can't skew the rasters.
fn reset_typography_defaults() {
    crate::font_settings::set_gamma(1.0);
    crate::font_settings::set_width(1.0);
    crate::font_settings::set_interval(0.0);
    crate::font_settings::set_faux_weight(0.0);
    crate::font_settings::set_faux_italic(0.0);
    crate::font_settings::set_primary_weight(1.0 / 3.0);
    crate::font_settings::set_hinting_enabled(true);
}

// ── metric ──────────────────────────────────────────────────────────────────

/// Approximate sRGB → linear decode (pure 2.2 power, no linear toe).
fn lin(c: u8) -> f64 {
    (c as f64 / 255.0).powf(2.2)
}

/// Rec.709 luminance of one RGBA8 pixel, in linear light.
fn luma(px: &[u8]) -> f64 {
    0.2126 * lin(px[0]) + 0.7152 * lin(px[1]) + 0.0722 * lin(px[2])
}

/// Total linear-light ink of an RGBA8 buffer against white: `Σ (1 - Y)`.
fn linear_ink(buf: &[u8]) -> f64 {
    buf.chunks_exact(4).map(|p| 1.0 - luma(p)).sum()
}

/// Composite a cached mask onto a fresh white RGBA8 buffer through the
/// **production** `composite_lcd_mask` (per-channel src-over in sRGB space).
fn composite_on_white(cached: &CachedLcdText, color: Color) -> Vec<u8> {
    let w = cached.width;
    let h = cached.height;
    let mask = LcdMask {
        data: (*cached.pixels).clone(),
        width: w,
        height: h,
    };
    let mut buf = vec![255u8; (w as usize) * (h as usize) * 4];
    composite_lcd_mask(&mut buf, w, h, &mask, color, 0, 0);
    buf
}

/// LCD text must not deposit more than 6% more linear-light ink than the
/// grayscale rendering of the same string.
///
/// There is an intrinsic surplus of roughly 3.5–4.5% (measured: ~4.5% for
/// Nunito, ~3.5% for CascadiaCode at 14px). It comes from compositing in sRGB
/// space: the 5-tap filter spreads a glyph edge across neighbouring subpixels,
/// producing many mid-range coverage values, and a mid-range coverage blended
/// without a linear-light decode lands darker than its linear equivalent. The
/// grayscale mask concentrates the same energy into fewer, more extreme
/// coverage values, so it suffers less from the same non-linearity. That
/// surplus is a property of the blend space, not a defect in the filter, and
/// the level was validated as acceptable against the demo sidebar by the user.
///
/// The 1.06 bound therefore does not demand parity — it pins the surplus near
/// where it was validated, so a future change to the 5-tap filter weights or to
/// `composite_lcd_mask` that makes LCD text visibly bolder than grayscale fails
/// here instead of shipping.
///
/// The companion coverage-sum assertion separates the two possible causes: the
/// raw mask sums must stay within 2% of each other, so if this test ever fails
/// the coverage check tells you whether the rasteriser changed how much ink it
/// emits (raster regression) or only how that ink is distributed and blended
/// (composite/filter regression).
#[test]
fn lcd_vs_gray_ink_parity_bound() {
    reset_typography_defaults();

    // Near-black text on white — the typical light-theme body-text case.
    let color = Color::rgba(0.1, 0.1, 0.1, 1.0);

    for (name, f) in [("Nunito", nunito_font()), ("CascadiaCode", cascadia_font())] {
        let lcd = rasterize_text_lcd_cached(&f, DIAG_TEXT, DIAG_SIZE);
        let gray = rasterize_text_gray_cached(&f, DIAG_TEXT, DIAG_SIZE);

        // Raster parity: both pipelines must emit essentially the same total
        // coverage. Only the DISTRIBUTION across subpixels should differ.
        let cov_lcd: u64 = lcd.pixels.iter().map(|&b| b as u64).sum();
        let cov_gray: u64 = gray.pixels.iter().map(|&b| b as u64).sum();
        assert!(cov_gray > 0, "{name}: gray mask produced no coverage");
        let cov_ratio = cov_lcd as f64 / cov_gray as f64;
        assert!(
            (cov_ratio - 1.0).abs() < 0.02,
            "{name}: raw mask coverage diverged; lcd={cov_lcd} gray={cov_gray} \
             (ratio {cov_ratio:.4}, want within 2% of 1.0) — the rasteriser \
             changed how much ink it emits, not just how it spreads it"
        );

        let ink_gray = linear_ink(&composite_on_white(&gray, color));
        let ink_lcd = linear_ink(&composite_on_white(&lcd, color));
        assert!(ink_gray > 0.0, "{name}: gray composite deposited no ink");

        let ink_ratio = ink_lcd / ink_gray;
        assert!(
            ink_ratio < 1.06,
            "{name}: LCD text is too bold relative to grayscale; \
             ink_lcd={ink_lcd:.3} ink_gray={ink_gray:.3} (ratio {ink_ratio:.4}, \
             want < 1.06). Coverage ratio is {cov_ratio:.4}, so this is a \
             filter/composite regression rather than a raster one."
        );
    }
}
