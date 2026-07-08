use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use agg_rust::basics::FillingRule;
use agg_rust::color::Gray8;
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_transform::ConvTransform;

use agg_rust::path_storage::PathStorage;
use agg_rust::pixfmt_gray::PixfmtGray8;
use agg_rust::rasterizer_scanline_aa::RasterizerScanlineAa;
use agg_rust::renderer_base::RendererBase;
use agg_rust::renderer_scanline::render_scanlines_aa_solid;
use agg_rust::rendering_buffer::RowAccessor;
use agg_rust::scanline_u::ScanlineU8;
use agg_rust::trans_affine::TransAffine;

use super::filter::{apply_5_tap_filter, apply_gray_collapse};
use crate::color::Color;
use crate::draw_ctx::FillRule;
use crate::text::{measure_text_metrics, shape_text, Font};

/// Identity transform — exposed so call sites that don't otherwise
/// depend on `agg_rust::trans_affine::TransAffine` can pass one.
pub fn identity_xform() -> TransAffine {
    TransAffine::new()
}

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
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// Mask-local x of the glyph origin (= padding inset).
    pub baseline_x_in_mask: f64,
    /// Mask-local Y-up y of the glyph baseline.
    pub baseline_y_in_mask: f64,
}

const MASK_PAD: f64 = 2.0;

#[derive(Clone, PartialEq, Eq, Hash)]
struct LcdMaskKey {
    text: String,
    font_ptr: usize,
    size_bits: u64,
    /// Typography-style fingerprint — every parameter that `shape_text`
    /// now applies must be part of the cache key, or a slider drag would
    /// keep serving stale masks rendered in the previous style.  Bits
    /// are read off the f64s so we inherit `Eq` / `Hash`.
    width_bits: u64,
    italic_bits: u64,
    interval_bits: u64,
    hint_y: bool,
    faux_weight_bits: u64,
    primary_weight_bits: u64,
    gamma_bits: u64,
    /// `true` for the grayscale (whole-pixel coverage) variant, `false`
    /// for LCD subpixel. Same raster, different finalize — so the two
    /// must not collide in the shared cache.
    gray: bool,
}

struct LcdMaskEntry {
    pixels: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    baseline_x_in_mask: f64,
    baseline_y_in_mask: f64,
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
pub fn rasterize_text_lcd_cached(font: &Arc<Font>, text: &str, size: f64) -> CachedLcdText {
    rasterize_text_mask_cached(font, text, size, false)
}

/// Grayscale (whole-pixel coverage) sibling of [`rasterize_text_lcd_cached`].
///
/// Runs the identical AGG rasterization and caching, but box-averages the
/// 3× horizontal gray buffer down to one coverage value per pixel and
/// replicates it into all three channels (`R = G = B = coverage`). The
/// result blits through the very same per-channel composite the LCD mask
/// uses, so the GPU path is shared — but with equal channels there is no
/// subpixel colour fringing. This is the anti-aliased text path for
/// hi-DPI / touch displays where LCD subpixel order is unknown or the
/// per-logical-pixel fringe would be visible (e.g. scaled desktops).
pub fn rasterize_text_gray_cached(font: &Arc<Font>, text: &str, size: f64) -> CachedLcdText {
    rasterize_text_mask_cached(font, text, size, true)
}

fn rasterize_text_mask_cached(
    font: &Arc<Font>,
    text: &str,
    size: f64,
    gray: bool,
) -> CachedLcdText {
    // Snapshot the current typography style once so the same values
    // used for the cache key are also used to size the mask below.
    let width_now = crate::font_settings::current_width();
    let italic_now = crate::font_settings::current_faux_italic();
    let interval_now = crate::font_settings::current_interval();
    let hint_y_now = crate::font_settings::hinting_enabled();
    let fweight_now = crate::font_settings::current_faux_weight();
    let pweight_now = crate::font_settings::current_primary_weight();
    let gamma_now = crate::font_settings::current_gamma();

    let key = LcdMaskKey {
        text: text.to_string(),
        font_ptr: Arc::as_ptr(font) as *const () as usize,
        size_bits: size.to_bits(),
        width_bits: width_now.to_bits(),
        italic_bits: italic_now.to_bits(),
        interval_bits: interval_now.to_bits(),
        hint_y: hint_y_now,
        faux_weight_bits: fweight_now.to_bits(),
        primary_weight_bits: pweight_now.to_bits(),
        gamma_bits: gamma_now.to_bits(),
        gray,
    };
    // Cache hit path — bump LRU, return shared Arc.
    let hit = MASK_CACHE.with(|m| {
        m.borrow().get(&key).map(|e| CachedLcdText {
            pixels: Arc::clone(&e.pixels),
            width: e.width,
            height: e.height,
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
    let m = measure_text_metrics(font, text, size);
    // Extra horizontal slack when Width != 1.0 (last glyph outline is
    // scaled beyond its advance) or Faux Italic != 0 (shear lifts the
    // top-right of each glyph past the advance column).  Without this
    // a slider drag past 1.0/0 would crop glyph stems at the mask
    // edges.
    let width_slack = (width_now - 1.0).abs() * size;
    let italic_slack = (italic_now.abs() / 3.0) * (m.ascent + m.descent);
    let extra_pad = (width_slack + italic_slack).ceil();
    let pad_x = MASK_PAD + extra_pad;
    // Music and icon fonts deliberately overflow their ascender/descender
    // (a SMuFL treble clef spans staff spaces far past the em box), so a
    // mask sized from the font-wide metrics would crop such glyphs flat.
    // Grow to the actual outline extents of the glyphs being drawn.
    let mut ink_ascent = m.ascent;
    let mut ink_descent = m.descent;
    for ch in text.chars() {
        if let Some((y_min, y_max)) = font.glyph_visual_bounds(ch, size) {
            ink_ascent = ink_ascent.max(y_max);
            ink_descent = ink_descent.max(-y_min);
        }
    }
    let bw = (m.width + pad_x * 2.0).ceil().max(1.0) as u32;
    let bh = (ink_ascent + ink_descent + MASK_PAD * 2.0).ceil().max(1.0) as u32;
    let bx = pad_x;
    // Snap the mask's internal baseline Y to a whole pixel **only when
    // the user has hinting enabled** — the same checkbox that drives
    // the per-glyph `gy` snap inside `shape_text`.  This keeps the
    // two renderers aligned at integer pixels when the user opted in
    // to hinting, and leaves both at their natural sub-pixel positions
    // when they opted out (the small residual LCD/RGBA Y mismatch when
    // hinting is OFF is intrinsic to LCD's composite-row-alignment
    // requirement, not something we can paper over without forcing a
    // permanent snap that the user explicitly rejected).
    let by_unhinted = MASK_PAD + ink_descent;
    let by = if hint_y_now {
        by_unhinted.round()
    } else {
        by_unhinted
    };
    let mask = if gray {
        rasterize_gray_mask(font, text, size, bx, by, bw, bh, &TransAffine::new())
    } else {
        rasterize_lcd_mask(font, text, size, bx, by, bw, bh, &TransAffine::new())
    };
    let pixels = Arc::new(mask.data);
    let entry = LcdMaskEntry {
        pixels: Arc::clone(&pixels),
        width: bw,
        height: bh,
        baseline_x_in_mask: bx,
        baseline_y_in_mask: by,
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
        width: bw,
        height: bh,
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
    pub data: Vec<u8>, // len = width * height * 3, stride = width * 3
    pub width: u32,
    pub height: u32,
}

/// Rasterize `text` at baseline `(x, y)` into a 3-channel coverage mask
/// of size `mask_w × mask_h`.  `transform` is applied before the 3× X
/// scale that puts the path into the high-resolution grayscale buffer.
///
/// The returned mask has **no colour**; at composite time `composite_lcd_mask`
/// mixes the caller's desired text colour into the destination through the
/// per-channel coverage.
pub fn rasterize_lcd_mask(
    font: &Font,
    text: &str,
    size: f64,
    x: f64,
    y: f64,
    mask_w: u32,
    mask_h: u32,
    transform: &TransAffine,
) -> LcdMask {
    rasterize_lcd_mask_multi(font, &[(text, x, y)], size, mask_w, mask_h, transform)
}

/// Multi-span variant: raster several `(text, x, y)` tuples into a
/// single mask.  Used by wrapped-text `Label` so every line shares one
/// 3×-wide gray buffer and one filter pass.  The gray buffer is written
/// cumulatively by AGG (glyphs in different pixels don't interact, so
/// non-overlapping lines just occupy disjoint rows).
///
/// Now a thin wrapper over [`LcdMaskBuilder`] — kept as a free function
/// because the cached text path keys on `(text, font, size)` and never
/// needs to interleave non-text paths.  Generic callers should reach
/// for the builder directly.
pub fn rasterize_lcd_mask_multi(
    font: &Font,
    spans: &[(&str, f64, f64)],
    size: f64,
    mask_w: u32,
    mask_h: u32,
    transform: &TransAffine,
) -> LcdMask {
    let mut builder = LcdMaskBuilder::new(mask_w, mask_h);
    builder.with_paths(transform, |add| {
        for (text, x, y) in spans {
            if text.is_empty() {
                continue;
            }
            let (mut paths, _) = shape_text(font, text, size, *x, *y);
            for path in paths.iter_mut() {
                add(path);
            }
        }
    });
    builder.finalize()
}

/// Grayscale sibling of [`rasterize_lcd_mask`]: same 3× AGG rasterization,
/// but the gray buffer is box-collapsed 3→1 per pixel and the coverage is
/// replicated into all three channels. The returned mask has the identical
/// 3-byte layout, so it composites through the same path as an LCD mask
/// with no subpixel fringing.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_gray_mask(
    font: &Font,
    text: &str,
    size: f64,
    x: f64,
    y: f64,
    mask_w: u32,
    mask_h: u32,
    transform: &TransAffine,
) -> LcdMask {
    let mut builder = LcdMaskBuilder::new(mask_w, mask_h);
    builder.with_paths(transform, |add| {
        if !text.is_empty() {
            let (mut paths, _) = shape_text(font, text, size, x, y);
            for path in paths.iter_mut() {
                add(path);
            }
        }
    });
    builder.finalize_gray()
}

/// Convert a screen-space float clip rect `(x, y, w, h)` to the
/// integer pixel clip box `(x1, y1, x2, y2)` (half-open) used by
/// [`LcdBuffer::composite_mask`].  Floor on the left/bottom and ceil on
/// the right/top so any pixel touched by the clip rect (even partially)
/// is included — matches the AGG raster-clip convention.
pub fn rect_to_pixel_clip(rect: (f64, f64, f64, f64)) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = rect;
    (
        x.floor() as i32,
        y.floor() as i32,
        (x + w).ceil() as i32,
        (y + h).ceil() as i32,
    )
}

// ── LcdMaskBuilder ──────────────────────────────────────────────────────────
//
// Lifts the inner "rasterize one or more AGG paths at 3× X resolution →
// 5-tap low-pass filter → packed 3-byte LCD coverage mask" pipeline out
// of the text-only entry points so any path source can drive it.  This
// is the seam any new caller (rect fill, stroke, future widget paint)
// hooks into when it needs LCD-aware coverage output.

/// Accumulator for an [`LcdMask`].  Build the gray buffer with one or
/// more `with_paths` calls (each opens an AGG rasterizer scope), then
/// `finalize` to apply the 5-tap filter and produce the packed mask.
pub struct LcdMaskBuilder {
    gray: Vec<u8>,
    gray_w: u32,
    gray_h: u32,
    mask_w: u32,
    mask_h: u32,
    /// Optional screen-space clip rect (in mask pixel coords, post-CTM).
    /// Applied to the AGG renderer as a `clip_box_i` with X scaled by 3
    /// before any path is added, so any rasterised coverage outside the
    /// clip gets dropped at raster time (no need to also clip during
    /// the filter pass — zero gray = zero mask).
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
}

impl LcdMaskBuilder {
    /// Allocate a zeroed builder for an `mask_w × mask_h` output mask.
    /// The internal gray buffer is `(3 × mask_w) × mask_h` bytes.
    pub fn new(mask_w: u32, mask_h: u32) -> Self {
        let gray_w = mask_w.saturating_mul(3);
        let gray_h = mask_h;
        let gray = vec![0u8; (gray_w as usize) * (gray_h as usize)];
        Self {
            gray,
            gray_w,
            gray_h,
            mask_w,
            mask_h,
            clip: None,
            fill_rule: FillRule::NonZero,
        }
    }

    /// Set a clip rectangle in screen-space (mask pixel coords).  All
    /// subsequent `with_paths` calls render only inside the clip;
    /// pixels outside it stay zero in the gray buffer (and therefore
    /// produce zero coverage in the final filtered mask).  Builder-style;
    /// chain after `new`.
    pub fn with_clip(mut self, clip: Option<(f64, f64, f64, f64)>) -> Self {
        self.clip = clip;
        self
    }

    /// Set the fill rule used by subsequent path rasterization.
    pub fn with_fill_rule(mut self, fill_rule: FillRule) -> Self {
        self.fill_rule = fill_rule;
        self
    }

    /// Open an AGG rasterizer scope and let `f` add as many paths as
    /// it likes via the supplied `&mut FnMut(&mut PathStorage)`.  All
    /// paths share `transform`, with X supersampled by 3 inside the
    /// scope.  Lifetimes prevent us from keeping the renderer alive
    /// across separate method calls (it borrows `self.gray`), so the
    /// closure pattern scopes the borrow precisely.
    pub fn with_paths<F>(&mut self, transform: &TransAffine, f: F)
    where
        F: FnOnce(&mut dyn FnMut(&mut PathStorage)),
    {
        rasterize_paths_into_gray(
            &mut self.gray,
            self.gray_w,
            self.gray_h,
            transform,
            self.clip,
            self.fill_rule,
            f,
        );
    }

    /// Apply the 5-tap low-pass filter to the gray buffer and return
    /// the packed mask.  Consumes the builder; callers usually composite
    /// the result via [`LcdBuffer::composite_mask`] or
    /// [`composite_lcd_mask`].
    pub fn finalize(self) -> LcdMask {
        if self.mask_w == 0 || self.mask_h == 0 {
            return LcdMask {
                data: Vec::new(),
                width: self.mask_w,
                height: self.mask_h,
            };
        }
        let data = apply_5_tap_filter(&self.gray, self.gray_w, self.mask_w, self.mask_h);
        LcdMask {
            data,
            width: self.mask_w,
            height: self.mask_h,
        }
    }

    /// Collapse the 3× gray buffer to a whole-pixel coverage mask: box-average
    /// each triple of subpixels into one value and replicate it across R/G/B.
    /// Produces the same packed 3-byte layout as [`finalize`], so the result
    /// composites identically — but with equal channels, i.e. no chroma.
    pub fn finalize_gray(self) -> LcdMask {
        if self.mask_w == 0 || self.mask_h == 0 {
            return LcdMask {
                data: Vec::new(),
                width: self.mask_w,
                height: self.mask_h,
            };
        }
        let data = apply_gray_collapse(&self.gray, self.gray_w, self.mask_w, self.mask_h);
        LcdMask {
            data,
            width: self.mask_w,
            height: self.mask_h,
        }
    }
}

/// Internal: run one AGG rasterizer scope writing into `gray` at 3× X
/// scale.  The closure receives an `add` function that takes a mutable
/// `PathStorage` and renders it with curve flattening + the X-scaled
/// transform applied.  Optional `clip` (in mask pixel coords) is
/// applied to the renderer with X scaled by 3 to match the gray
/// buffer; rasterised coverage outside the clip is dropped at raster
/// time.
fn rasterize_paths_into_gray<F>(
    gray: &mut [u8],
    gray_w: u32,
    gray_h: u32,
    transform: &TransAffine,
    clip: Option<(f64, f64, f64, f64)>,
    fill_rule: FillRule,
    f: F,
) where
    F: FnOnce(&mut dyn FnMut(&mut PathStorage)),
{
    if gray_w == 0 || gray_h == 0 {
        return;
    }
    let stride = gray_w as i32;
    let mut ra = RowAccessor::new();
    unsafe {
        ra.attach(gray.as_mut_ptr(), gray_w, gray_h, stride);
    }
    let pf = PixfmtGray8::new(&mut ra);
    let mut rb = RendererBase::new(pf);
    if let Some((cx, cy, cw, ch)) = clip {
        // Clip box is in mask pixel coords.  The gray buffer is 3× X,
        // so multiply X bounds by 3 to land on the right subpixels.
        // `clip_box_i` is inclusive on both ends, so the right/top
        // edges use `-1` after the ceil.
        let x1 = (cx.floor() as i32).saturating_mul(3);
        let y1 = cy.floor() as i32;
        let x2 = ((cx + cw).ceil() as i32).saturating_mul(3) - 1;
        let y2 = (cy + ch).ceil() as i32 - 1;
        rb.clip_box_i(x1, y1, x2, y2);
    }
    let mut ras = RasterizerScanlineAa::new();
    ras.filling_rule(to_agg_fill_rule(fill_rule));
    let mut sl = ScanlineU8::new();

    // Full coverage = 255.  AGG writes `gray_value * alpha / 255` per
    // pixel; with value = 255 the output byte equals AGG's coverage
    // estimate at that pixel — exactly what the 5-tap filter expects
    // as input.
    let cov_color = Gray8::new_opaque(255);

    let mut xform = *transform;
    xform.sx *= 3.0;
    xform.shx *= 3.0;
    xform.tx *= 3.0;
    // shy, sy, ty unchanged — only X is supersampled.

    let mut add = |path: &mut PathStorage| {
        let mut curves = ConvCurve::new(path);
        let mut tx = ConvTransform::new(&mut curves, xform);
        ras.reset();
        ras.add_path(&mut tx, 0);
        render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &cov_color);
    };
    f(&mut add);
}

fn to_agg_fill_rule(rule: FillRule) -> FillingRule {
    match rule {
        FillRule::NonZero => FillingRule::NonZero,
        FillRule::EvenOdd => FillingRule::EvenOdd,
    }
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
    dst_w: u32,
    dst_h: u32,
    mask: &LcdMask,
    src: Color,
    dst_x: i32,
    dst_y: i32,
) {
    if mask.width == 0 || mask.height == 0 {
        return;
    }
    let sa = src.a.clamp(0.0, 1.0);
    let sr = src.r.clamp(0.0, 1.0);
    let sg = src.g.clamp(0.0, 1.0);
    let sb = src.b.clamp(0.0, 1.0);
    let dst_w_i = dst_w as i32;
    let dst_h_i = dst_h as i32;
    let mw = mask.width as i32;
    let mh = mask.height as i32;

    for my in 0..mh {
        // Both buffers Y-up: mask row my → dst row dst_y + my.
        let dy = dst_y + my;
        if dy < 0 || dy >= dst_h_i {
            continue;
        }
        for mx in 0..mw {
            let dx = dst_x + mx;
            if dx < 0 || dx >= dst_w_i {
                continue;
            }
            let mi = ((my * mw + mx) * 3) as usize;
            // Effective per-channel src-over weight is `mask_cov × src.a`.
            // Callers using a Color with alpha < 1 (e.g. placeholder text
            // painted in a half-opacity "dim" colour) depend on this to
            // get a partially-faded blit; without the alpha modulation
            // the blit is full-opacity regardless of src.a.
            let cr = (mask.data[mi] as f32 / 255.0) * sa;
            let cg = (mask.data[mi + 1] as f32 / 255.0) * sa;
            let cb = (mask.data[mi + 2] as f32 / 255.0) * sa;
            if cr == 0.0 && cg == 0.0 && cb == 0.0 {
                continue;
            }

            let di = ((dy * dst_w_i + dx) * 4) as usize;
            let dr = dst_rgba[di] as f32 / 255.0;
            let dg = dst_rgba[di + 1] as f32 / 255.0;
            let db = dst_rgba[di + 2] as f32 / 255.0;

            // Per-channel source-over in sRGB space.  Gamma-aware
            // linearization is the correct next step (see the design
            // doc); sRGB-direct is adequate for first-cut validation
            // and matches what FreeType does in its non-linear mode.
            let rr = sr * cr + dr * (1.0 - cr);
            let rg = sg * cg + dg * (1.0 - cg);
            let rbb = sb * cb + db * (1.0 - cb);

            dst_rgba[di] = (rr * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst_rgba[di + 1] = (rg * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dst_rgba[di + 2] = (rbb * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            // Alpha unchanged — mask composites onto the existing dst
            // without introducing transparency.
        }
    }
}
