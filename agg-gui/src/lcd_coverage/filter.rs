//! Gray-buffer → packed-mask downsampling filters.
//!
//! The rasterisation stage (in [`super::mask`]) writes AGG coverage into a
//! 3×-horizontally-supersampled gray buffer.  This module owns the second
//! stage: collapsing that buffer into the packed 3-byte-per-pixel mask that
//! composites onto the framebuffer.  Two flavours:
//!
//! - [`apply_5_tap_filter`] — the LCD subpixel path: a phase-shifted 5-tap
//!   low-pass per channel, so R/G/B carry independent coverage aligned to
//!   the panel's physical subpixels.
//! - [`apply_gray_collapse`] — the grayscale path: box-average each triple
//!   into one whole-pixel coverage value replicated across the channels,
//!   giving anti-aliased edges with no subpixel chroma.

/// FreeType-default 5-tap weights; sum = 9.  Heavier filter weights reduce
/// colour fringing at the cost of sharpness; tuning against this table is
/// the standard knob for "darker / lighter" LCD text.  These are the
/// legacy baked-in weights — still used as the fallback when the
/// Primary Weight global sits at its default `1/3` (at which point
/// `lcd_filter_weights()` below reproduces `[1, 2, 3, 2, 1] / 9`).
const FILTER_WEIGHTS: [u32; 5] = [1, 2, 3, 2, 1];
const FILTER_SUM: u32 = 9;

/// Per-frame tap weights for the 5-tap LCD filter, as f64 pre-normalised
/// so the five samples always sum to 1.0.  Parameterised on the Primary
/// Weight global (`font_settings::current_primary_weight`): the middle
/// tap carries `p * 9` units, the two shoulder taps 2 each, the two
/// outer taps 1 each — a direct analogue of the agg-rust
/// `LcdDistributionLut::new(primary, 2/9, 1/9)` construction.
///
/// Called once per mask rasterisation; the inner loop multiplies each
/// sample by the corresponding weight.  At the default `primary = 1/3`
/// the output is identical (up to rounding) to the legacy integer
/// `[1, 2, 3, 2, 1] / 9` filter.
fn lcd_filter_weights() -> [f64; 5] {
    let p_units = crate::font_settings::current_primary_weight() * 9.0;
    let weights = [1.0, 2.0, p_units, 2.0, 1.0];
    let sum = weights.iter().sum::<f64>().max(1e-9);
    [
        weights[0] / sum,
        weights[1] / sum,
        weights[2] / sum,
        weights[3] / sum,
        weights[4] / sum,
    ]
}

/// Run the 5-tap low-pass filter over `gray` and produce the packed
/// `(R,G,B)` mask.  See the `lcd_coverage` module docs for the per-channel
/// formula and phase shift.
pub(super) fn apply_5_tap_filter(gray: &[u8], gray_w: u32, mask_w: u32, mask_h: u32) -> Vec<u8> {
    // Decide once whether the current parameters reproduce the legacy
    // integer filter exactly.  When they do (primary = 1/3, gamma = 1),
    // run the original byte-for-byte path so every label cached before
    // any slider-driven raster produces the EXACT same bytes it did
    // pre-phase-3.  This is a correctness fast path, not just a
    // performance one — f64 arithmetic on e.g. (128+256+384+256+128)/9
    // rounds to 127.999… which truncates to 127, where the integer
    // version gives a clean 128.  Sub-u8 drift on cached masks is
    // invisible in isolation but accumulates into a faint "fade"
    // across a paragraph of text, so we keep the old path exact.
    let primary = crate::font_settings::current_primary_weight();
    let gamma = crate::font_settings::current_gamma();
    let is_default_primary = ((primary - 1.0 / 3.0).abs()) < 1e-6;
    let is_default_gamma = ((gamma - 1.0).abs()) < 1e-6;
    if is_default_primary && is_default_gamma {
        return apply_5_tap_filter_legacy(gray, gray_w, mask_w, mask_h);
    }

    let mut data = vec![0u8; (mask_w as usize) * (mask_h as usize) * 3];
    let gw = gray_w as i32;
    // Parameterised path — f64 weights driven by Primary Weight, plus
    // a gamma curve applied to the per-channel coverage AFTER the
    // filter sum so light AA edges strengthen or weaken uniformly.
    let w = lcd_filter_weights();
    let inv_g = 1.0 / gamma.max(1e-3);
    let need_gamma = !is_default_gamma;
    let apply_gamma = |c: f64| -> f64 {
        if !need_gamma {
            return c;
        }
        let t = (c / 255.0).clamp(0.0, 1.0);
        t.powf(inv_g) * 255.0
    };
    for py in 0..mask_h {
        let row_start = (py as usize) * (gray_w as usize);
        let row = &gray[row_start..row_start + gray_w as usize];
        for px in 0..mask_w {
            let base = (px as i32) * 3;
            let sample = |off: i32| -> f64 {
                let pos = base + off;
                if pos < 0 || pos >= gw {
                    0.0
                } else {
                    row[pos as usize] as f64
                }
            };
            // R samples [-2..=2], G shifts +1, B shifts +2 (phase offsets
            // between the three physical subpixels of the output pixel).
            let cov_r = w[0] * sample(-2)
                + w[1] * sample(-1)
                + w[2] * sample(0)
                + w[3] * sample(1)
                + w[4] * sample(2);
            let cov_g = w[0] * sample(-1)
                + w[1] * sample(0)
                + w[2] * sample(1)
                + w[3] * sample(2)
                + w[4] * sample(3);
            let cov_b = w[0] * sample(0)
                + w[1] * sample(1)
                + w[2] * sample(2)
                + w[3] * sample(3)
                + w[4] * sample(4);
            let mi = ((py as usize) * (mask_w as usize) + (px as usize)) * 3;
            // `.round()` here matches the classic integer filter's
            // rounding semantics more closely than bare `as u8` (which
            // truncates) — minor but measurable difference near mid-gray.
            data[mi] = apply_gamma(cov_r).round().clamp(0.0, 255.0) as u8;
            data[mi + 1] = apply_gamma(cov_g).round().clamp(0.0, 255.0) as u8;
            data[mi + 2] = apply_gamma(cov_b).round().clamp(0.0, 255.0) as u8;
        }
    }
    data
}

/// Byte-exact legacy 5-tap filter — preserved for the
/// primary-weight = 1/3, gamma = 1 default path so cached text
/// rasterised before phase 3 matches what we produce now.
fn apply_5_tap_filter_legacy(gray: &[u8], gray_w: u32, mask_w: u32, mask_h: u32) -> Vec<u8> {
    let mut data = vec![0u8; (mask_w as usize) * (mask_h as usize) * 3];
    let gw = gray_w as i32;
    for py in 0..mask_h {
        let row_start = (py as usize) * (gray_w as usize);
        let row = &gray[row_start..row_start + gray_w as usize];
        for px in 0..mask_w {
            let base = (px as i32) * 3;
            let sample = |off: i32| -> u32 {
                let pos = base + off;
                if pos < 0 || pos >= gw {
                    0
                } else {
                    row[pos as usize] as u32
                }
            };
            let cov_r = (FILTER_WEIGHTS[0] * sample(-2)
                + FILTER_WEIGHTS[1] * sample(-1)
                + FILTER_WEIGHTS[2] * sample(0)
                + FILTER_WEIGHTS[3] * sample(1)
                + FILTER_WEIGHTS[4] * sample(2))
                / FILTER_SUM;
            let cov_g = (FILTER_WEIGHTS[0] * sample(-1)
                + FILTER_WEIGHTS[1] * sample(0)
                + FILTER_WEIGHTS[2] * sample(1)
                + FILTER_WEIGHTS[3] * sample(2)
                + FILTER_WEIGHTS[4] * sample(3))
                / FILTER_SUM;
            let cov_b = (FILTER_WEIGHTS[0] * sample(0)
                + FILTER_WEIGHTS[1] * sample(1)
                + FILTER_WEIGHTS[2] * sample(2)
                + FILTER_WEIGHTS[3] * sample(3)
                + FILTER_WEIGHTS[4] * sample(4))
                / FILTER_SUM;
            let mi = ((py as usize) * (mask_w as usize) + (px as usize)) * 3;
            data[mi] = cov_r.min(255) as u8;
            data[mi + 1] = cov_g.min(255) as u8;
            data[mi + 2] = cov_b.min(255) as u8;
        }
    }
    data
}

/// Box-average each triple of subpixels in the 3×-wide gray buffer into one
/// coverage value and write it into all three channels of the packed mask.
/// The 3× horizontal supersampling gives real horizontal AA; AGG's scanline
/// coverage already supplies vertical AA — so the plain average is the
/// correct downsample, no low-pass phase filter needed.
pub(super) fn apply_gray_collapse(gray: &[u8], gray_w: u32, mask_w: u32, mask_h: u32) -> Vec<u8> {
    let mut data = vec![0u8; (mask_w as usize) * (mask_h as usize) * 3];
    for py in 0..mask_h as usize {
        let row_start = py * gray_w as usize;
        let out_row = py * mask_w as usize * 3;
        for px in 0..mask_w as usize {
            let g = row_start + px * 3;
            // gray_w == mask_w * 3, so all three subpixels are in-bounds.
            let cov = ((gray[g] as u32 + gray[g + 1] as u32 + gray[g + 2] as u32 + 1) / 3) as u8;
            let o = out_row + px * 3;
            data[o] = cov;
            data[o + 1] = cov;
            data[o + 2] = cov;
        }
    }
    data
}
