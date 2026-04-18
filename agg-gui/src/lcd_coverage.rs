//! LCD subpixel text as a **per-channel coverage mask** that composites
//! onto arbitrary backgrounds — no bg pre-fill, no destination-color
//! knowledge required at rasterization time.
//!
//! # Why this replaces the pre-fill approach
//!
//! The older `PixfmtRgba32Lcd` path baked the caller's background colour
//! into the rasterised output via a per-channel src-over against the
//! pre-filled framebuffer.  That coupled the LCD glyphs to one specific
//! destination and forced us to know that destination everywhere text is
//! drawn — driving the walk / sample / push / pop complexity.
//!
//! Instead, we keep the **three subpixel coverage values independent**:
//! the output of the rasteriser is three 8-bit channels per pixel
//! `(cov_r, cov_g, cov_b)` describing how much of each subpixel the glyph
//! covered.  At composite time a per-channel Porter-Duff `over` blend
//! mixes the TEXT COLOUR into the live destination:
//!
//! ```text
//! dst.r = src.r * cov.r + dst.r * (1 - cov.r)
//! dst.g = src.g * cov.g + dst.g * (1 - cov.g)
//! dst.b = src.b * cov.b + dst.b * (1 - cov.b)
//! ```
//!
//! The coverage mask is the same regardless of where it lands; the blend
//! naturally produces the correct LCD chroma against any background.
//!
//! See `lcd-subpixel-compositing.md` at the repository root for the full
//! derivation.
//!
//! # Pipeline
//!
//! ```text
//! shape_text (rustybuzz kerning + fallback chain — unchanged)
//!   │
//! per-glyph PathStorage → ConvTransform(scale_x_3) → PixfmtGray8
//!   (8-bit grayscale coverage at 3× horizontal resolution)
//!   │
//! 5-tap low-pass filter per output channel
//!   │
//! packed (cov_r, cov_g, cov_b) 3-byte mask
//! ```

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use agg_rust::color::Gray8;
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_transform::ConvTransform;

// ---------------------------------------------------------------------------
// LcdBuffer — opaque 3-byte-per-pixel RGB render target
// ---------------------------------------------------------------------------
//
// Analogue of `Framebuffer` for widgets that opt into
// [`crate::widget::BackbufferMode::LcdCoverage`].  Every fill into an
// `LcdBuffer` goes through the 3× horizontal supersample + 5-tap filter
// pipeline and composites per-channel via Porter-Duff src-over.  The
// buffer has no alpha channel — it's intended to be fully covered by
// opaque fills and blitted as an opaque RGB texture.

/// RGB framebuffer, row 0 = bottom (matches `Framebuffer` convention).
/// 3 bytes per pixel: `(R, G, B)` composited result of every fill so far.
pub struct LcdBuffer {
    pixels: Vec<u8>,
    width:  u32,
    height: u32,
}

impl LcdBuffer {
    /// Allocate a zeroed buffer (all pixels black = `(0, 0, 0)`).
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixels: vec![0u8; (width as usize) * (height as usize) * 3],
            width,
            height,
        }
    }

    #[inline] pub fn width(&self)  -> u32     { self.width }
    #[inline] pub fn height(&self) -> u32     { self.height }
    #[inline] pub fn pixels(&self) -> &[u8]   { &self.pixels }
    #[inline] pub fn pixels_mut(&mut self) -> &mut [u8] { &mut self.pixels }

    /// Consume the buffer and hand ownership of the underlying
    /// `Vec<u8>` — used when moving the rendered pixels into an
    /// `Arc` for the widget's backbuffer cache.
    pub fn into_pixels(self) -> Vec<u8> { self.pixels }

    /// Top-row-first copy of the pixels — matches the convention used
    /// by `draw_image_rgba_arc` (images uploaded as textures expect
    /// row 0 at top).  One-time flip on cache build.
    pub fn pixels_flipped(&self) -> Vec<u8> {
        let row_bytes = (self.width * 3) as usize;
        let mut out = vec![0u8; self.pixels.len()];
        for y in 0..self.height as usize {
            let src = &self.pixels[y * row_bytes .. (y + 1) * row_bytes];
            let dst_y = self.height as usize - 1 - y;
            out[dst_y * row_bytes .. (dst_y + 1) * row_bytes].copy_from_slice(src);
        }
        out
    }
}
use agg_rust::pixfmt_gray::PixfmtGray8;
use agg_rust::rasterizer_scanline_aa::RasterizerScanlineAa;
use agg_rust::renderer_base::RendererBase;
use agg_rust::renderer_scanline::render_scanlines_aa_solid;
use agg_rust::rendering_buffer::RowAccessor;
use agg_rust::scanline_u::ScanlineU8;
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::text::{measure_text_metrics, shape_text, Font};

/// Identity transform — exposed so call sites that don't otherwise
/// depend on `agg_rust::trans_affine::TransAffine` can pass one.
pub fn identity_xform() -> TransAffine { TransAffine::new() }

// ---------------------------------------------------------------------------
// Cached LCD text raster
// ---------------------------------------------------------------------------
//
// The mask is fully determined by `(text, font_ptr, font_size)` — colour is
// applied at composite time, and placement coordinates are just translations
// the caller handles.  Caching keeps `fill_text` roughly as fast as the old
// grayscale path: AGG rasterisation runs once per unique text string, and
// GL backends can further cache the uploaded texture keyed on the returned
// `Arc`'s pointer identity (see `demo-gl`'s `arc_texture_cache` pattern).

/// Result of [`rasterize_text_lcd_cached`].  Callers composite the mask
/// at `(x - baseline_x_in_mask, y - baseline_y_in_mask)` where `(x, y)`
/// is the target baseline position in local / screen coordinates.
pub struct CachedLcdText {
    /// 3-byte-per-pixel coverage mask, Y-up (row 0 = bottom).  Shared
    /// `Arc` so GL backends can key a texture cache on its pointer
    /// identity — one upload per unique raster result.
    pub pixels:              Arc<Vec<u8>>,
    pub width:               u32,
    pub height:              u32,
    /// Mask-local x of the glyph origin (= padding inset).
    pub baseline_x_in_mask:  f64,
    /// Mask-local Y-up y of the glyph baseline.
    pub baseline_y_in_mask:  f64,
}

const MASK_PAD: f64 = 2.0;

#[derive(Clone, PartialEq, Eq, Hash)]
struct LcdMaskKey {
    text:      String,
    font_ptr:  usize,
    size_bits: u64,
}

struct LcdMaskEntry {
    pixels:              Arc<Vec<u8>>,
    width:               u32,
    height:              u32,
    baseline_x_in_mask:  f64,
    baseline_y_in_mask:  f64,
}

thread_local! {
    static MASK_CACHE: RefCell<HashMap<LcdMaskKey, LcdMaskEntry>>
        = RefCell::new(HashMap::new());
    static MASK_LRU: RefCell<VecDeque<LcdMaskKey>>
        = RefCell::new(VecDeque::new());
}

const MASK_CACHE_MAX: usize = 1024;

/// Rasterise `text` in `font` at `size` into a 3-channel LCD coverage mask,
/// caching the result so subsequent calls with the same `(text, font, size)`
/// return the shared `Arc` without re-running AGG.
pub fn rasterize_text_lcd_cached(
    font: &Arc<Font>,
    text: &str,
    size: f64,
) -> CachedLcdText {
    let key = LcdMaskKey {
        text:      text.to_string(),
        font_ptr:  Arc::as_ptr(font) as *const () as usize,
        size_bits: size.to_bits(),
    };
    // Cache hit path — bump LRU, return shared Arc.
    let hit = MASK_CACHE.with(|m| {
        m.borrow().get(&key).map(|e| CachedLcdText {
            pixels:             Arc::clone(&e.pixels),
            width:              e.width,
            height:             e.height,
            baseline_x_in_mask: e.baseline_x_in_mask,
            baseline_y_in_mask: e.baseline_y_in_mask,
        })
    });
    if let Some(got) = hit {
        MASK_LRU.with(|lru| {
            let mut lru = lru.borrow_mut();
            // Move key to back (most recently used).
            if let Some(pos) = lru.iter().position(|k| k == &key) {
                lru.remove(pos);
            }
            lru.push_back(key);
        });
        return got;
    }

    // Cache miss — run the rasteriser.
    let m   = measure_text_metrics(font, text, size);
    let bw  = (m.width  + MASK_PAD * 2.0).ceil().max(1.0) as u32;
    let bh  = (m.ascent + m.descent + MASK_PAD * 2.0).ceil().max(1.0) as u32;
    let bx  = MASK_PAD;
    let by  = MASK_PAD + m.descent;
    let mask = rasterize_lcd_mask(
        font, text, size, bx, by, bw, bh, &TransAffine::new(),
    );
    let pixels = Arc::new(mask.data);
    let entry = LcdMaskEntry {
        pixels:              Arc::clone(&pixels),
        width:               bw,
        height:              bh,
        baseline_x_in_mask:  bx,
        baseline_y_in_mask:  by,
    };

    MASK_CACHE.with(|m| m.borrow_mut().insert(key.clone(), entry));
    MASK_LRU.with(|lru| {
        let mut lru = lru.borrow_mut();
        lru.push_back(key.clone());
        // LRU evict to cap — drop the oldest Arc strong refs so GL
        // texture caches holding a Weak will see them expire and
        // release their textures.
        while lru.len() > MASK_CACHE_MAX {
            if let Some(old) = lru.pop_front() {
                MASK_CACHE.with(|m| m.borrow_mut().remove(&old));
            }
        }
    });

    CachedLcdText {
        pixels,
        width:              bw,
        height:             bh,
        baseline_x_in_mask: bx,
        baseline_y_in_mask: by,
    }
}

/// 3-byte-per-pixel LCD coverage mask.  Callers composite via
/// [`composite_lcd_mask`].  The distinction from a normal RGBA image is
/// crucial: the three channels are **independent coverage values**, not
/// an RGB colour — they drive a per-channel blend where each subpixel
/// mixes the source colour with the destination colour by its own amount.
pub struct LcdMask {
    pub data:   Vec<u8>, // len = width * height * 3, stride = width * 3
    pub width:  u32,
    pub height: u32,
}

/// FreeType-default 5-tap weights; sum = 9.  Heavier filter weights reduce
/// colour fringing at the cost of sharpness; tuning against this table is
/// the standard knob for "darker / lighter" LCD text.
const FILTER_WEIGHTS: [u32; 5] = [1, 2, 3, 2, 1];
const FILTER_SUM:     u32       = 9;

/// Rasterize `text` at baseline `(x, y)` into a 3-channel coverage mask
/// of size `mask_w × mask_h`.  `transform` is applied before the 3× X
/// scale that puts the path into the high-resolution grayscale buffer.
///
/// The returned mask has **no colour**; at composite time `composite_lcd_mask`
/// mixes the caller's desired text colour into the destination through the
/// per-channel coverage.
pub fn rasterize_lcd_mask(
    font:      &Font,
    text:      &str,
    size:      f64,
    x:         f64,
    y:         f64,
    mask_w:    u32,
    mask_h:    u32,
    transform: &TransAffine,
) -> LcdMask {
    rasterize_lcd_mask_multi(font, &[(text, x, y)], size, mask_w, mask_h, transform)
}

/// Multi-span variant: raster several `(text, x, y)` tuples into a
/// single mask.  Used by wrapped-text `Label` so every line shares one
/// 3×-wide gray buffer and one filter pass.  The gray buffer is written
/// cumulatively by AGG (glyphs in different pixels don't interact, so
/// non-overlapping lines just occupy disjoint rows).
pub fn rasterize_lcd_mask_multi(
    font:      &Font,
    spans:     &[(&str, f64, f64)],
    size:      f64,
    mask_w:    u32,
    mask_h:    u32,
    transform: &TransAffine,
) -> LcdMask {
    if mask_w == 0 || mask_h == 0 {
        return LcdMask { data: Vec::new(), width: mask_w, height: mask_h };
    }

    // High-resolution intermediate: one byte per subpixel.
    let gray_w = mask_w * 3;
    let gray_h = mask_h;
    let mut gray = vec![0u8; (gray_w as usize) * (gray_h as usize)];

    // Rasterize every span into the same gray buffer with 3× X scale.
    {
        let stride = gray_w as i32;
        let mut ra = RowAccessor::new();
        unsafe { ra.attach(gray.as_mut_ptr(), gray_w, gray_h, stride); }
        let pf = PixfmtGray8::new(&mut ra);
        let mut rb = RendererBase::new(pf);

        let mut ras = RasterizerScanlineAa::new();
        let mut sl  = ScanlineU8::new();

        // Full coverage = 255.  AGG writes `gray_value * alpha / 255` per
        // pixel; with value = 255 the output byte equals the AGG coverage
        // at that pixel — exactly what we need for the filter input.
        let cov_color = Gray8::new_opaque(255);

        let mut xform = *transform;
        xform.sx  *= 3.0;
        xform.shx *= 3.0;
        xform.tx  *= 3.0;
        // shy, sy, ty unchanged — only X is supersampled.

        for (text, x, y) in spans {
            if text.is_empty() { continue; }
            let (mut paths, _) = shape_text(font, text, size, *x, *y);
            for path in paths.iter_mut() {
                let mut curves = ConvCurve::new(path);
                let mut tx     = ConvTransform::new(&mut curves, xform);
                ras.reset();
                ras.add_path(&mut tx, 0);
                render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &cov_color);
            }
        }
    }

    // Post-process: 5-tap low-pass filter per channel.  See module docs
    // for the formula; the phase shift between R/G/B (± 1 subpixel) is
    // what produces the horizontal colour separation that characterises
    // LCD subpixel rendering.
    let mut data = vec![0u8; (mask_w as usize) * (mask_h as usize) * 3];
    let gw = gray_w as i32;
    for py in 0..mask_h {
        let row_start = (py as usize) * (gray_w as usize);
        let row = &gray[row_start .. row_start + gray_w as usize];
        for px in 0..mask_w {
            let base = (px as i32) * 3;
            let sample = |off: i32| -> u32 {
                let pos = base + off;
                if pos < 0 || pos >= gw { 0 } else { row[pos as usize] as u32 }
            };
            // R samples [-2..=2], G shifts +1, B shifts +2 (phase offsets
            // between the three physical subpixels of the output pixel).
            let cov_r = (FILTER_WEIGHTS[0] * sample(-2)
                       + FILTER_WEIGHTS[1] * sample(-1)
                       + FILTER_WEIGHTS[2] * sample(0)
                       + FILTER_WEIGHTS[3] * sample(1)
                       + FILTER_WEIGHTS[4] * sample(2)) / FILTER_SUM;
            let cov_g = (FILTER_WEIGHTS[0] * sample(-1)
                       + FILTER_WEIGHTS[1] * sample(0)
                       + FILTER_WEIGHTS[2] * sample(1)
                       + FILTER_WEIGHTS[3] * sample(2)
                       + FILTER_WEIGHTS[4] * sample(3)) / FILTER_SUM;
            let cov_b = (FILTER_WEIGHTS[0] * sample(0)
                       + FILTER_WEIGHTS[1] * sample(1)
                       + FILTER_WEIGHTS[2] * sample(2)
                       + FILTER_WEIGHTS[3] * sample(3)
                       + FILTER_WEIGHTS[4] * sample(4)) / FILTER_SUM;
            let mi = ((py as usize) * (mask_w as usize) + (px as usize)) * 3;
            data[mi]     = cov_r.min(255) as u8;
            data[mi + 1] = cov_g.min(255) as u8;
            data[mi + 2] = cov_b.min(255) as u8;
        }
    }

    LcdMask { data, width: mask_w, height: mask_h }
}

/// Composite an [`LcdMask`] onto `dst_rgba` using per-channel Porter-Duff
/// "over": each subpixel mixes `src_color` into the live destination by
/// its own coverage.  The destination colour is whatever pixels are
/// currently at the target rect — so this works over any background.
///
/// Both the mask and `dst_rgba` are **Y-up** (row 0 = bottom), matching
/// `agg-gui`'s `Framebuffer` convention.  `(dst_x, dst_y)` is the mask's
/// bottom-left in the destination's Y-up pixel grid; mask row `my` is
/// written to destination row `dst_y + my`.
pub fn composite_lcd_mask(
    dst_rgba: &mut [u8],
    dst_w:    u32,
    dst_h:    u32,
    mask:     &LcdMask,
    src:      Color,
    dst_x:    i32,
    dst_y:    i32,
) {
    if mask.width == 0 || mask.height == 0 { return; }
    let sr = src.r.clamp(0.0, 1.0);
    let sg = src.g.clamp(0.0, 1.0);
    let sb = src.b.clamp(0.0, 1.0);
    let dst_w_i = dst_w as i32;
    let dst_h_i = dst_h as i32;
    let mw = mask.width  as i32;
    let mh = mask.height as i32;

    for my in 0..mh {
        // Both buffers Y-up: mask row my → dst row dst_y + my.
        let dy = dst_y + my;
        if dy < 0 || dy >= dst_h_i { continue; }
        for mx in 0..mw {
            let dx = dst_x + mx;
            if dx < 0 || dx >= dst_w_i { continue; }
            let mi = ((my * mw + mx) * 3) as usize;
            let cr = mask.data[mi]     as f32 / 255.0;
            let cg = mask.data[mi + 1] as f32 / 255.0;
            let cb = mask.data[mi + 2] as f32 / 255.0;
            if cr == 0.0 && cg == 0.0 && cb == 0.0 { continue; }

            let di = ((dy * dst_w_i + dx) * 4) as usize;
            let dr = dst_rgba[di]     as f32 / 255.0;
            let dg = dst_rgba[di + 1] as f32 / 255.0;
            let db = dst_rgba[di + 2] as f32 / 255.0;

            // Per-channel source-over in sRGB space.  Gamma-aware
            // linearization is the correct next step (see the design
            // doc); sRGB-direct is adequate for first-cut validation
            // and matches what FreeType does in its non-linear mode.
            let rr = sr * cr + dr * (1.0 - cr);
            let rg = sg * cg + dg * (1.0 - cg);
            let rbb = sb * cb + db * (1.0 - cb);

            dst_rgba[di]     = (rr  * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst_rgba[di + 1] = (rg  * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst_rgba[di + 2] = (rbb * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            // Alpha unchanged — mask composites onto the existing dst
            // without introducing transparency.
        }
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

    /// The rasteriser must produce some non-zero coverage for ordinary
    /// text — sanity check that the pipeline wires up at all.
    #[test]
    fn test_lcd_mask_has_coverage() {
        let mask = rasterize_lcd_mask(
            &font(), "Hello", 16.0, 4.0, 12.0,
            200, 40, &TransAffine::new(),
        );
        let total: u64 = mask.data.iter().map(|&b| b as u64).sum();
        assert!(total > 0, "rasterize_lcd_mask produced all-zero coverage");
    }

    /// Edge pixels must exhibit **per-channel variation** — the
    /// defining property of LCD subpixel rendering.  Without the 5-tap
    /// filter's phase shift between R/G/B, the three channels would be
    /// identical at every pixel.
    #[test]
    fn test_lcd_mask_has_channel_variation() {
        let mask = rasterize_lcd_mask(
            &font(), "Wing", 24.0, 4.0, 16.0,
            400, 40, &TransAffine::new(),
        );
        let mut saw = false;
        for px in mask.data.chunks_exact(3) {
            let r = px[0];
            let g = px[1];
            let b = px[2];
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            if mx > 20 && (mx - mn) > 10 {
                saw = true;
                break;
            }
        }
        assert!(saw, "no per-channel variation at edges");
    }

    /// Compositing the mask must mix text into any destination bg and
    /// produce plausibly darker pixels for dark-on-light, and plausibly
    /// lighter pixels for light-on-dark, regardless of which bg the mask
    /// was rastered against (it wasn't rastered against any).
    #[test]
    fn test_composite_dark_on_light_and_light_on_dark() {
        let mask = rasterize_lcd_mask(
            &font(), "Hi", 20.0, 2.0, 14.0,
            80, 24, &TransAffine::new(),
        );

        // Dark text on white.
        let mut fb_white = vec![255u8; 80 * 24 * 4];
        composite_lcd_mask(&mut fb_white, 80, 24, &mask, Color::black(), 0, 0);
        let sum_white: u64 = fb_white.chunks_exact(4)
            .map(|p| (p[0] as u64 + p[1] as u64 + p[2] as u64))
            .sum();
        assert!(sum_white < 80 * 24 * 3 * 255,
                "dark-on-white composite left every pixel white");

        // Light text on black.
        let mut fb_black = vec![0u8; 80 * 24 * 4];
        for chunk in fb_black.chunks_exact_mut(4) { chunk[3] = 255; }
        composite_lcd_mask(&mut fb_black, 80, 24, &mask, Color::white(), 0, 0);
        let sum_black: u64 = fb_black.chunks_exact(4)
            .map(|p| (p[0] as u64 + p[1] as u64 + p[2] as u64))
            .sum();
        assert!(sum_black > 0,
                "light-on-black composite left every pixel black");
    }
}
