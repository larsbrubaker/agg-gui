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

/// LCD coverage buffer, row 0 = bottom (matches `Framebuffer` convention).
///
/// **Two planes, 3 bytes per pixel each:**
///
/// - `color`: per-channel **premultiplied** RGB colour accumulated from
///   every paint so far.  `(R_color, G_color, B_color)` where each byte
///   is `channel_color * channel_alpha`.
/// - `alpha`: per-channel alpha/coverage accumulated from every paint so
///   far.  `(R_alpha, G_alpha, B_alpha)` where each byte is the combined
///   opacity of that subpixel column (0 = untouched, 255 = fully opaque).
///
/// **Why per-channel alpha?**  LCD subpixel rendering produces a distinct
/// coverage value per R/G/B channel, so a single per-pixel alpha can't
/// represent the output correctly at glyph edges and fractional image
/// boundaries.  Splitting alpha per-channel gives each subpixel its own
/// Porter-Duff state: paints accumulate independently through the same
/// premultiplied src-over math you'd use for a normal RGBA surface, just
/// three streams instead of one.  A cached `LcdBuffer` with partial
/// coverage can be composited onto any destination without the "black
/// rect where unpainted" failure mode that killed the first-cut design.
pub struct LcdBuffer {
    color:  Vec<u8>,
    alpha:  Vec<u8>,
    width:  u32,
    height: u32,
}

impl LcdBuffer {
    /// Allocate a fully-transparent buffer (color zero, alpha zero
    /// everywhere).  "Transparent" here means the per-channel alpha is
    /// 0, so composite-onto-destination leaves the destination
    /// unchanged wherever no paint has landed yet.
    pub fn new(width: u32, height: u32) -> Self {
        let bytes = (width as usize) * (height as usize) * 3;
        Self {
            color: vec![0u8; bytes],
            alpha: vec![0u8; bytes],
            width,
            height,
        }
    }

    #[inline] pub fn width(&self)  -> u32 { self.width }
    #[inline] pub fn height(&self) -> u32 { self.height }

    #[inline] pub fn color_plane(&self)     -> &[u8]     { &self.color }
    #[inline] pub fn alpha_plane(&self)     -> &[u8]     { &self.alpha }
    #[inline] pub fn color_plane_mut(&mut self) -> &mut [u8] { &mut self.color }
    #[inline] pub fn alpha_plane_mut(&mut self) -> &mut [u8] { &mut self.alpha }

    /// Both planes mutably in one borrow — for inner loops that update
    /// a pixel's colour and alpha together (image blit, manual composite).
    #[inline]
    pub fn planes_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        (&mut self.color, &mut self.alpha)
    }

    /// Consume the buffer, returning the owned `(color, alpha)` planes
    /// as a pair — used when moving the painted pixels into `Arc`s for
    /// a widget's backbuffer cache or for GPU texture upload.
    pub fn into_planes(self) -> (Vec<u8>, Vec<u8>) { (self.color, self.alpha) }

    /// Top-row-first copy of the colour plane, suitable for a plain
    /// RGB8 upload or CPU blit.  Row 0 of the output is the VISUAL
    /// top of the buffer (Y-up → Y-down flip).
    pub fn color_plane_flipped(&self) -> Vec<u8> {
        flip_plane(&self.color, self.width, self.height)
    }

    /// Top-row-first copy of the alpha plane.
    pub fn alpha_plane_flipped(&self) -> Vec<u8> {
        flip_plane(&self.alpha, self.width, self.height)
    }

    /// Collapse both planes into a single top-row-first straight-alpha
    /// RGBA8 image suitable for the existing blit pipeline (one texture,
    /// standard `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` blend).
    ///
    /// The per-channel alphas get collapsed to a single per-pixel alpha
    /// via `max(R_alpha, G_alpha, B_alpha)`; RGB is recovered by dividing
    /// the premult colour by that max alpha (straight-alpha form).  This
    /// conversion is **lossy** when the three subpixel alphas diverge
    /// (the whole point of the per-channel representation is lost under
    /// collapse).  It's correct for typical monochrome-text cases where
    /// all three alphas agree, and degrades gracefully otherwise —
    /// Phase 5.2's two-plane blit path preserves the full per-channel
    /// information through upload and shader.
    pub fn to_rgba8_top_down_collapsed(&self) -> Vec<u8> {
        let w = self.width  as usize;
        let h = self.height as usize;
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            let src_y = h - 1 - y;
            for x in 0..w {
                let si = (src_y * w + x) * 3;
                let di = (y * w + x) * 4;
                let ra = self.alpha[si];
                let ga = self.alpha[si + 1];
                let ba = self.alpha[si + 2];
                let a  = ra.max(ga).max(ba);
                if a == 0 { continue; } // fully transparent → keep RGBA zero
                let af = a as f32 / 255.0;
                let rc = self.color[si]     as f32 / 255.0;
                let gc = self.color[si + 1] as f32 / 255.0;
                let bc = self.color[si + 2] as f32 / 255.0;
                out[di]     = ((rc / af) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                out[di + 1] = ((gc / af) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                out[di + 2] = ((bc / af) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                out[di + 3] = a;
            }
        }
        out
    }

    // ── Paint primitives ────────────────────────────────────────────────────
    //
    // These are the foundation operations every higher layer (LcdGfxCtx,
    // path-fill helpers, image blit) eventually composes into.  They write
    // directly into the 3-byte-per-pixel coverage store with no intermediate
    // allocation.

    /// Fill the entire buffer with a solid colour.  Every subpixel gets
    /// the same premultiplied colour contribution and the same alpha —
    /// a flat clear has no per-subpixel differentiation, so the three
    /// alpha channels are all set to `color.a` and the three colour
    /// channels to `color.rgb * color.a`.
    pub fn clear(&mut self, color: Color) {
        let a  = color.a.clamp(0.0, 1.0);
        let r_c = ((color.r.clamp(0.0, 1.0) * a) * 255.0 + 0.5) as u8;
        let g_c = ((color.g.clamp(0.0, 1.0) * a) * 255.0 + 0.5) as u8;
        let b_c = ((color.b.clamp(0.0, 1.0) * a) * 255.0 + 0.5) as u8;
        let a_byte = (a * 255.0 + 0.5) as u8;
        for px in self.color.chunks_exact_mut(3) {
            px[0] = r_c;
            px[1] = g_c;
            px[2] = b_c;
        }
        for px in self.alpha.chunks_exact_mut(3) {
            px[0] = a_byte;
            px[1] = a_byte;
            px[2] = a_byte;
        }
    }

    /// Fill an AGG path through the LCD pipeline: rasterize at 3× X
    /// resolution → 5-tap filter → per-channel src-over composite into
    /// this buffer.  `transform` is applied to `path` before the 3× X
    /// scale (typically the caller's CTM); the path's coordinates are
    /// in the buffer's pixel space (Y-up, origin = bottom-left).
    /// Optional `clip` is a screen-space rect (post-CTM, in mask pixel
    /// coords) — pixels outside it are unaffected.
    ///
    /// First non-text primitive on the buffer.  Future fill / stroke /
    /// image-blit entry points either call this directly (for solid
    /// fills / outlines) or open their own `LcdMaskBuilder` scope when
    /// they need to batch many paths into one mask.
    ///
    /// First-cut implementation: rasterizes at the buffer's full size.
    /// A later optimization can compute the path's bbox and size the
    /// scratch tightly — measurable win for small paths in large
    /// buffers, but architecturally identical and not required for
    /// correctness.
    pub fn fill_path(
        &mut self,
        path:      &mut PathStorage,
        color:     Color,
        transform: &TransAffine,
        clip:      Option<(f64, f64, f64, f64)>,
    ) {
        if self.width == 0 || self.height == 0 { return; }
        let mut builder = LcdMaskBuilder::new(self.width, self.height).with_clip(clip);
        builder.with_paths(transform, |add| { add(path); });
        let mask = builder.finalize();
        // Convert clip → integer pixel rect for composite-time enforcement.
        // The gray-buffer raster clip should already have zeroed coverage
        // outside, but the 5-tap filter can leak ±2 subpixels at clip
        // edges; composite-time clip catches that.
        let clip_i = clip.map(rect_to_pixel_clip);
        self.composite_mask(&mask, color, 0, 0, clip_i);
    }

    /// Composite an [`LcdMask`] into this buffer using per-channel
    /// **premultiplied** Porter-Duff src-over.  Each subpixel column's
    /// effective alpha is `src.a × mask.channel_coverage`, and colour +
    /// alpha both accumulate under the standard premult src-over:
    ///
    /// ```text
    /// eff_a_c        = src.a * mask.c
    /// buf.color_c   := src.c * eff_a_c + buf.color_c * (1 - eff_a_c)
    /// buf.alpha_c   := eff_a_c         + buf.alpha_c * (1 - eff_a_c)
    /// ```
    ///
    /// `(dst_x, dst_y)` is the mask's bottom-left in this buffer's Y-up
    /// pixel grid; mask row `my` writes to buffer row `dst_y + my`.
    /// Optional `clip` (in this buffer's integer pixel coords:
    /// `(x1, y1, x2, y2)`, half-open) suppresses writes outside its
    /// bounds — used by widgets that paint inside a clipping parent.
    pub fn composite_mask(
        &mut self,
        mask:  &LcdMask,
        src:   Color,
        dst_x: i32,
        dst_y: i32,
        clip:  Option<(i32, i32, i32, i32)>,
    ) {
        if mask.width == 0 || mask.height == 0 { return; }
        let sa = src.a.clamp(0.0, 1.0);
        let sr = src.r.clamp(0.0, 1.0);
        let sg = src.g.clamp(0.0, 1.0);
        let sb = src.b.clamp(0.0, 1.0);
        let dst_w_i = self.width  as i32;
        let dst_h_i = self.height as i32;
        let dst_w_u = self.width as usize;
        let mw = mask.width  as i32;
        let mh = mask.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((cx1, cy1, cx2, cy2)) =>
                (cx1.max(0), cy1.max(0), cx2.min(dst_w_i), cy2.min(dst_h_i)),
            None => (0, 0, dst_w_i, dst_h_i),
        };
        if cx1 >= cx2 || cy1 >= cy2 { return; }

        for my in 0..mh {
            let dy = dst_y + my;
            if dy < cy1 || dy >= cy2 { continue; }
            let dy_u = dy as usize;
            for mx in 0..mw {
                let dx = dst_x + mx;
                if dx < cx1 || dx >= cx2 { continue; }
                let mi = ((my * mw + mx) * 3) as usize;
                // Per-channel effective alpha = src colour alpha × mask coverage.
                let ea_r = sa * (mask.data[mi]     as f32 / 255.0);
                let ea_g = sa * (mask.data[mi + 1] as f32 / 255.0);
                let ea_b = sa * (mask.data[mi + 2] as f32 / 255.0);
                if ea_r == 0.0 && ea_g == 0.0 && ea_b == 0.0 { continue; }

                let di = (dy_u * dst_w_u + (dx as usize)) * 3;
                // Read existing premult colour + per-channel alpha.
                let bc_r = self.color[di]     as f32 / 255.0;
                let bc_g = self.color[di + 1] as f32 / 255.0;
                let bc_b = self.color[di + 2] as f32 / 255.0;
                let ba_r = self.alpha[di]     as f32 / 255.0;
                let ba_g = self.alpha[di + 1] as f32 / 255.0;
                let ba_b = self.alpha[di + 2] as f32 / 255.0;
                // Premult src-over per channel.  `src.c × eff_a` is the
                // premultiplied source colour contribution; it adds to
                // the buffer's existing premult colour, weighted by
                // (1 - eff_a).  Alpha stream does the same Porter-Duff
                // composite independently per channel.
                let rc_r = sr * ea_r + bc_r * (1.0 - ea_r);
                let rc_g = sg * ea_g + bc_g * (1.0 - ea_g);
                let rc_b = sb * ea_b + bc_b * (1.0 - ea_b);
                let ra_r = ea_r + ba_r * (1.0 - ea_r);
                let ra_g = ea_g + ba_g * (1.0 - ea_g);
                let ra_b = ea_b + ba_b * (1.0 - ea_b);

                self.color[di]     = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di]     = (ra_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di + 1] = (ra_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di + 2] = (ra_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// Composite `src` onto this buffer at offset `(dst_x, dst_y)` via
    /// **per-channel premultiplied src-over** — the buffer-level
    /// analogue of [`Self::composite_mask`].  Each of the three
    /// subpixel columns applies `src.ch_alpha` as its own
    /// Porter-Duff weight:
    ///
    /// ```text
    /// buf.color_c := src.color_c + buf.color_c * (1 - src.alpha_c)
    /// buf.alpha_c := src.alpha_c + buf.alpha_c * (1 - src.alpha_c)
    /// ```
    ///
    /// Untouched source pixels (alpha zero on every channel) don't
    /// change the buffer at all — exactly the semantic that makes a
    /// popped layer leave unpainted areas alone, no seed trick needed.
    pub fn composite_buffer(
        &mut self,
        src:   &LcdBuffer,
        dst_x: i32,
        dst_y: i32,
        clip:  Option<(i32, i32, i32, i32)>,
    ) {
        if src.width == 0 || src.height == 0 { return; }
        let dst_w_i = self.width  as i32;
        let dst_h_i = self.height as i32;
        let dst_w_u = self.width as usize;
        let src_w_u = src.width  as usize;
        let sw = src.width  as i32;
        let sh = src.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((x1, y1, x2, y2)) =>
                (x1.max(0), y1.max(0), x2.min(dst_w_i), y2.min(dst_h_i)),
            None => (0, 0, dst_w_i, dst_h_i),
        };
        if cx1 >= cx2 || cy1 >= cy2 { return; }

        for sy in 0..sh {
            let dy = dst_y + sy;
            if dy < cy1 || dy >= cy2 { continue; }
            let dy_u = dy as usize;
            let sy_u = sy as usize;
            for sx in 0..sw {
                let dx = dst_x + sx;
                if dx < cx1 || dx >= cx2 { continue; }
                let si = (sy_u * src_w_u + sx as usize) * 3;
                let di = (dy_u * dst_w_u + dx as usize) * 3;

                let sa_r = src.alpha[si]     as f32 / 255.0;
                let sa_g = src.alpha[si + 1] as f32 / 255.0;
                let sa_b = src.alpha[si + 2] as f32 / 255.0;
                if sa_r == 0.0 && sa_g == 0.0 && sa_b == 0.0 { continue; }

                let sc_r = src.color[si]     as f32 / 255.0;
                let sc_g = src.color[si + 1] as f32 / 255.0;
                let sc_b = src.color[si + 2] as f32 / 255.0;

                let bc_r = self.color[di]     as f32 / 255.0;
                let bc_g = self.color[di + 1] as f32 / 255.0;
                let bc_b = self.color[di + 2] as f32 / 255.0;
                let ba_r = self.alpha[di]     as f32 / 255.0;
                let ba_g = self.alpha[di + 1] as f32 / 255.0;
                let ba_b = self.alpha[di + 2] as f32 / 255.0;

                // src is already premultiplied, so `sc + bc*(1-sa)` is the
                // plain Porter-Duff expression — no additional modulation.
                let rc_r = sc_r + bc_r * (1.0 - sa_r);
                let rc_g = sc_g + bc_g * (1.0 - sa_g);
                let rc_b = sc_b + bc_b * (1.0 - sa_b);
                let ra_r = sa_r + ba_r * (1.0 - sa_r);
                let ra_g = sa_g + ba_g * (1.0 - sa_g);
                let ra_b = sa_b + ba_b * (1.0 - sa_b);

                self.color[di]     = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di]     = (ra_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di + 1] = (ra_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di + 2] = (ra_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Y-flip a 3-byte/pixel plane (Y-up row 0 = bottom → top-row-first).
fn flip_plane(src: &[u8], width: u32, height: u32) -> Vec<u8> {
    let row_bytes = (width * 3) as usize;
    let mut out = vec![0u8; src.len()];
    for y in 0..height as usize {
        let dst_y = height as usize - 1 - y;
        out[dst_y * row_bytes .. (dst_y + 1) * row_bytes]
            .copy_from_slice(&src[y * row_bytes .. (y + 1) * row_bytes]);
    }
    out
}
use agg_rust::path_storage::PathStorage;
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
    /// Typography-style fingerprint — every parameter that `shape_text`
    /// now applies must be part of the cache key, or a slider drag would
    /// keep serving stale masks rendered in the previous style.  Bits
    /// are read off the f64s so we inherit `Eq` / `Hash`.
    width_bits:          u64,
    italic_bits:         u64,
    interval_bits:       u64,
    hint_y:              bool,
    faux_weight_bits:    u64,
    primary_weight_bits: u64,
    gamma_bits:          u64,
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
    // Snapshot the current typography style once so the same values
    // used for the cache key are also used to size the mask below.
    let width_now    = crate::font_settings::current_width();
    let italic_now   = crate::font_settings::current_faux_italic();
    let interval_now = crate::font_settings::current_interval();
    let hint_y_now   = crate::font_settings::hinting_enabled();
    let fweight_now  = crate::font_settings::current_faux_weight();
    let pweight_now  = crate::font_settings::current_primary_weight();
    let gamma_now    = crate::font_settings::current_gamma();

    let key = LcdMaskKey {
        text:      text.to_string(),
        font_ptr:  Arc::as_ptr(font) as *const () as usize,
        size_bits: size.to_bits(),
        width_bits:          width_now.to_bits(),
        italic_bits:         italic_now.to_bits(),
        interval_bits:       interval_now.to_bits(),
        hint_y:              hint_y_now,
        faux_weight_bits:    fweight_now.to_bits(),
        primary_weight_bits: pweight_now.to_bits(),
        gamma_bits:          gamma_now.to_bits(),
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
    // Extra horizontal slack when Width != 1.0 (last glyph outline is
    // scaled beyond its advance) or Faux Italic != 0 (shear lifts the
    // top-right of each glyph past the advance column).  Without this
    // a slider drag past 1.0/0 would crop glyph stems at the mask
    // edges.
    let width_slack  = (width_now - 1.0).abs() * size;
    let italic_slack = (italic_now.abs() / 3.0) * (m.ascent + m.descent);
    let extra_pad    = (width_slack + italic_slack).ceil();
    let pad_x        = MASK_PAD + extra_pad;
    let bw  = (m.width  + pad_x   * 2.0).ceil().max(1.0) as u32;
    let bh  = (m.ascent + m.descent + MASK_PAD * 2.0).ceil().max(1.0) as u32;
    let bx  = pad_x;
    // Snap the mask's internal baseline Y to a whole pixel **only when
    // the user has hinting enabled** — the same checkbox that drives
    // the per-glyph `gy` snap inside `shape_text`.  This keeps the
    // two renderers aligned at integer pixels when the user opted in
    // to hinting, and leaves both at their natural sub-pixel positions
    // when they opted out (the small residual LCD/RGBA Y mismatch when
    // hinting is OFF is intrinsic to LCD's composite-row-alignment
    // requirement, not something we can paper over without forcing a
    // permanent snap that the user explicitly rejected).
    let by_unhinted = MASK_PAD + m.descent;
    let by = if hint_y_now { by_unhinted.round() } else { by_unhinted };
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
/// the standard knob for "darker / lighter" LCD text.  These are the
/// legacy baked-in weights — still used as the fallback when the
/// Primary Weight global sits at its default `1/3` (at which point
/// `lcd_filter_weights()` below reproduces `[1, 2, 3, 2, 1] / 9`).
const FILTER_WEIGHTS: [u32; 5] = [1, 2, 3, 2, 1];
const FILTER_SUM:     u32       = 9;

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
///
/// Now a thin wrapper over [`LcdMaskBuilder`] — kept as a free function
/// because the cached text path keys on `(text, font, size)` and never
/// needs to interleave non-text paths.  Generic callers should reach
/// for the builder directly.
pub fn rasterize_lcd_mask_multi(
    font:      &Font,
    spans:     &[(&str, f64, f64)],
    size:      f64,
    mask_w:    u32,
    mask_h:    u32,
    transform: &TransAffine,
) -> LcdMask {
    let mut builder = LcdMaskBuilder::new(mask_w, mask_h);
    builder.with_paths(transform, |add| {
        for (text, x, y) in spans {
            if text.is_empty() { continue; }
            let (mut paths, _) = shape_text(font, text, size, *x, *y);
            for path in paths.iter_mut() {
                add(path);
            }
        }
    });
    builder.finalize()
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
    gray:   Vec<u8>,
    gray_w: u32,
    gray_h: u32,
    mask_w: u32,
    mask_h: u32,
    /// Optional screen-space clip rect (in mask pixel coords, post-CTM).
    /// Applied to the AGG renderer as a `clip_box_i` with X scaled by 3
    /// before any path is added, so any rasterised coverage outside the
    /// clip gets dropped at raster time (no need to also clip during
    /// the filter pass — zero gray = zero mask).
    clip:   Option<(f64, f64, f64, f64)>,
}

impl LcdMaskBuilder {
    /// Allocate a zeroed builder for an `mask_w × mask_h` output mask.
    /// The internal gray buffer is `(3 × mask_w) × mask_h` bytes.
    pub fn new(mask_w: u32, mask_h: u32) -> Self {
        let gray_w = mask_w.saturating_mul(3);
        let gray_h = mask_h;
        let gray   = vec![0u8; (gray_w as usize) * (gray_h as usize)];
        Self { gray, gray_w, gray_h, mask_w, mask_h, clip: None }
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

    /// Open an AGG rasterizer scope and let `f` add as many paths as
    /// it likes via the supplied `&mut FnMut(&mut PathStorage)`.  All
    /// paths share `transform`, with X supersampled by 3 inside the
    /// scope.  Lifetimes prevent us from keeping the renderer alive
    /// across separate method calls (it borrows `self.gray`), so the
    /// closure pattern scopes the borrow precisely.
    pub fn with_paths<F>(&mut self, transform: &TransAffine, f: F)
    where F: FnOnce(&mut dyn FnMut(&mut PathStorage)),
    {
        rasterize_paths_into_gray(
            &mut self.gray, self.gray_w, self.gray_h, transform, self.clip, f,
        );
    }

    /// Apply the 5-tap low-pass filter to the gray buffer and return
    /// the packed mask.  Consumes the builder; callers usually composite
    /// the result via [`LcdBuffer::composite_mask`] or
    /// [`composite_lcd_mask`].
    pub fn finalize(self) -> LcdMask {
        if self.mask_w == 0 || self.mask_h == 0 {
            return LcdMask { data: Vec::new(), width: self.mask_w, height: self.mask_h };
        }
        let data = apply_5_tap_filter(
            &self.gray, self.gray_w, self.mask_w, self.mask_h,
        );
        LcdMask { data, width: self.mask_w, height: self.mask_h }
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
    gray:      &mut [u8],
    gray_w:    u32,
    gray_h:    u32,
    transform: &TransAffine,
    clip:      Option<(f64, f64, f64, f64)>,
    f:         F,
)
where F: FnOnce(&mut dyn FnMut(&mut PathStorage)),
{
    if gray_w == 0 || gray_h == 0 { return; }
    let stride = gray_w as i32;
    let mut ra = RowAccessor::new();
    unsafe { ra.attach(gray.as_mut_ptr(), gray_w, gray_h, stride); }
    let pf = PixfmtGray8::new(&mut ra);
    let mut rb  = RendererBase::new(pf);
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
    let mut sl  = ScanlineU8::new();

    // Full coverage = 255.  AGG writes `gray_value * alpha / 255` per
    // pixel; with value = 255 the output byte equals AGG's coverage
    // estimate at that pixel — exactly what the 5-tap filter expects
    // as input.
    let cov_color = Gray8::new_opaque(255);

    let mut xform = *transform;
    xform.sx  *= 3.0;
    xform.shx *= 3.0;
    xform.tx  *= 3.0;
    // shy, sy, ty unchanged — only X is supersampled.

    let mut add = |path: &mut PathStorage| {
        let mut curves = ConvCurve::new(path);
        let mut tx     = ConvTransform::new(&mut curves, xform);
        ras.reset();
        ras.add_path(&mut tx, 0);
        render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &cov_color);
    };
    f(&mut add);
}

/// Internal: run the 5-tap low-pass filter over `gray` and produce the
/// packed `(R,G,B)` mask.  See module docs for the per-channel formula
/// and phase shift.
fn apply_5_tap_filter(gray: &[u8], gray_w: u32, mask_w: u32, mask_h: u32) -> Vec<u8> {
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
    let primary  = crate::font_settings::current_primary_weight();
    let gamma    = crate::font_settings::current_gamma();
    let is_default_primary = ((primary - 1.0 / 3.0).abs()) < 1e-6;
    let is_default_gamma   = ((gamma - 1.0).abs()) < 1e-6;
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
        if !need_gamma { return c; }
        let t = (c / 255.0).clamp(0.0, 1.0);
        t.powf(inv_g) * 255.0
    };
    for py in 0..mask_h {
        let row_start = (py as usize) * (gray_w as usize);
        let row = &gray[row_start .. row_start + gray_w as usize];
        for px in 0..mask_w {
            let base = (px as i32) * 3;
            let sample = |off: i32| -> f64 {
                let pos = base + off;
                if pos < 0 || pos >= gw { 0.0 } else { row[pos as usize] as f64 }
            };
            // R samples [-2..=2], G shifts +1, B shifts +2 (phase offsets
            // between the three physical subpixels of the output pixel).
            let cov_r = w[0] * sample(-2) + w[1] * sample(-1)
                      + w[2] * sample( 0) + w[3] * sample( 1)
                      + w[4] * sample( 2);
            let cov_g = w[0] * sample(-1) + w[1] * sample( 0)
                      + w[2] * sample( 1) + w[3] * sample( 2)
                      + w[4] * sample( 3);
            let cov_b = w[0] * sample( 0) + w[1] * sample( 1)
                      + w[2] * sample( 2) + w[3] * sample( 3)
                      + w[4] * sample( 4);
            let mi = ((py as usize) * (mask_w as usize) + (px as usize)) * 3;
            // `.round()` here matches the classic integer filter's
            // rounding semantics more closely than bare `as u8` (which
            // truncates) — minor but measurable difference near mid-gray.
            data[mi]     = apply_gamma(cov_r).round().clamp(0.0, 255.0) as u8;
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
        let row = &gray[row_start .. row_start + gray_w as usize];
        for px in 0..mask_w {
            let base = (px as i32) * 3;
            let sample = |off: i32| -> u32 {
                let pos = base + off;
                if pos < 0 || pos >= gw { 0 } else { row[pos as usize] as u32 }
            };
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
    data
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
    let sa = src.a.clamp(0.0, 1.0);
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
            // Effective per-channel src-over weight is `mask_cov × src.a`.
            // Callers using a Color with alpha < 1 (e.g. placeholder text
            // painted in a half-opacity "dim" colour) depend on this to
            // get a partially-faded blit; without the alpha modulation
            // the blit is full-opacity regardless of src.a.
            let cr = (mask.data[mi]     as f32 / 255.0) * sa;
            let cg = (mask.data[mi + 1] as f32 / 255.0) * sa;
            let cb = (mask.data[mi + 2] as f32 / 255.0) * sa;
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

    /// `composite_lcd_mask` must honour `src.a` — multiply each channel's
    /// coverage by the source alpha.  Without this, partial-alpha text
    /// (e.g. a placeholder drawn in a half-opacity "dim" colour) blits
    /// at full opacity, looking solid instead of faded.
    ///
    /// Regression test for the bug visible in the search-box placeholder
    /// where "Search..." rendered at full intensity in LCD mode.
    #[test]
    fn test_composite_lcd_mask_honours_src_alpha() {
        // Single pixel, full coverage on all three channels.
        let mask = LcdMask { data: vec![255, 255, 255], width: 1, height: 1 };

        // Opaque black on white → full black.
        let mut fb_full = vec![255u8, 255, 255, 255];
        composite_lcd_mask(&mut fb_full, 1, 1, &mask, Color::rgba(0.0, 0.0, 0.0, 1.0), 0, 0);
        assert_eq!(fb_full[0], 0, "alpha=1 black-on-white should fully cover → R=0");

        // Half-alpha black on white → ~50% grey.
        let mut fb_half = vec![255u8, 255, 255, 255];
        composite_lcd_mask(&mut fb_half, 1, 1, &mask, Color::rgba(0.0, 0.0, 0.0, 0.5), 0, 0);
        // Expected: cov = 1.0 × 0.5 = 0.5; dst = 0×0.5 + 255×0.5 ≈ 128.
        assert!(fb_half[0] >= 120 && fb_half[0] <= 135,
            "alpha=0.5 black-on-white should land near R=128, got {}", fb_half[0]);

        // Zero-alpha: dst unchanged.
        let mut fb_zero = vec![255u8, 255, 255, 255];
        composite_lcd_mask(&mut fb_zero, 1, 1, &mask, Color::rgba(0.0, 0.0, 0.0, 0.0), 0, 0);
        assert_eq!(fb_zero[0], 255, "alpha=0 must leave destination untouched");
    }

    // ── LcdBuffer paint primitives ──────────────────────────────────────────

    /// `LcdBuffer::clear` writes the requested colour into every pixel.
    /// Premultiplied alpha applies uniformly across all three channels —
    /// the buffer has no alpha store, so partial-alpha is realised by
    /// darkening the colour, not by storing transparency.
    #[test]
    fn test_lcd_buffer_clear_writes_solid_color() {
        let mut buf = LcdBuffer::new(4, 3);
        buf.clear(Color::rgba(1.0, 0.5, 0.25, 1.0));
        for px in buf.color_plane().chunks_exact(3) {
            assert_eq!(px[0], 255);
            assert_eq!(px[1], 128);
            assert_eq!(px[2], 64);
        }

        // Half-alpha → premultiplied colour at half intensity.
        let mut buf2 = LcdBuffer::new(2, 2);
        buf2.clear(Color::rgba(1.0, 1.0, 1.0, 0.5));
        for px in buf2.color_plane().chunks_exact(3) {
            assert_eq!(px[0], 128);
            assert_eq!(px[1], 128);
            assert_eq!(px[2], 128);
        }
    }

    // ── Per-channel alpha: the new capability ────────────────────────────────

    /// Fresh buffer is fully transparent (both planes zero).  This is
    /// the defining change from the old 3-byte LcdBuffer: unpainted
    /// regions no longer read as "intentional black" on composite.
    #[test]
    fn test_lcd_buffer_fresh_is_fully_transparent() {
        let buf = LcdBuffer::new(8, 4);
        assert!(buf.color_plane().iter().all(|&b| b == 0),
            "fresh buffer's color plane must be zero");
        assert!(buf.alpha_plane().iter().all(|&b| b == 0),
            "fresh buffer's alpha plane must be zero (= fully transparent)");
    }

    /// Paint black text onto a transparent buffer.  The premultiplied
    /// colour is black × alpha = 0, so `color_plane` stays all zeros —
    /// but `alpha_plane` picks up coverage at text pixels and stays
    /// zero elsewhere.  That zero-alpha outside-text region is exactly
    /// the property that lets a cached LcdBuffer blit onto any parent
    /// without the "black rect where unpainted" failure mode.
    #[test]
    fn test_lcd_buffer_transparent_plus_black_text_leaves_alpha_only() {
        let f = font();
        let mask = rasterize_lcd_mask(&f, "Hi", 20.0, 2.0, 14.0, 80, 24, &TransAffine::new());
        let mut buf = LcdBuffer::new(80, 24);
        buf.composite_mask(&mask, Color::black(), 0, 0, None);

        assert!(buf.color_plane().iter().all(|&b| b == 0),
            "black-text-on-transparent: premult colour is 0, so color_plane stays zero");
        let alpha_nonzero = buf.alpha_plane().iter().filter(|&&b| b > 0).count();
        assert!(alpha_nonzero > 0,
            "alpha_plane must show coverage where text was rasterized");

        // Corners of the buffer (far from text) must stay fully transparent.
        let bottom_left_i  = 0;
        let bottom_right_i = (80 - 1) * 3;
        let top_left_i     = (23 * 80) * 3;
        let top_right_i    = (23 * 80 + 79) * 3;
        for i in [bottom_left_i, bottom_right_i, top_left_i, top_right_i] {
            assert_eq!(&buf.alpha_plane()[i .. i + 3], &[0u8, 0, 0],
                "corner at byte offset {i} should be transparent");
        }
    }

    /// Opaque red text deposits premultiplied red into the colour plane
    /// AND full alpha into the alpha plane at fully-covered subpixels.
    /// This is the crisp case where per-channel alpha == per-channel
    /// coverage, no divergence.
    #[test]
    fn test_lcd_buffer_red_text_writes_premultiplied_color() {
        let f = font();
        let w = 80u32; let h = 24u32;
        let mask = rasterize_lcd_mask(&f, "I", 24.0, 4.0, 18.0, w, h, &TransAffine::new());
        let mut buf = LcdBuffer::new(w, h);
        buf.composite_mask(&mask, Color::rgba(1.0, 0.0, 0.0, 1.0), 0, 0, None);

        // Look for at least one pixel where the R channel is fully
        // covered: R_alpha = 255, R_color = 255 (premult red × 1),
        // and G/B colour stay zero (red source has no G or B).
        let mut saw_full_red = false;
        for i in (0..(w * h) as usize).map(|p| p * 3) {
            if buf.alpha_plane()[i]     == 255
            && buf.color_plane()[i]     == 255
            && buf.color_plane()[i + 1] == 0
            && buf.color_plane()[i + 2] == 0
            {
                saw_full_red = true;
                break;
            }
        }
        assert!(saw_full_red, "expected at least one fully-covered pure-red pixel");
    }

    /// `composite_buffer` leaves dst untouched wherever src has alpha=0.
    /// The defining behavioural property of the two-plane design: a
    /// sub-layer with painted content plus unpainted margins flushes
    /// back onto its parent without clobbering the margins.
    #[test]
    fn test_lcd_buffer_composite_buffer_leaves_dst_untouched_where_src_is_transparent() {
        // src: all transparent (no paint).
        let src = LcdBuffer::new(4, 4);

        // dst: solid white.
        let mut dst = LcdBuffer::new(4, 4);
        dst.clear(Color::white());

        // Snapshot expected values: white everywhere, full alpha.
        for px in dst.color_plane().chunks_exact(3) { assert_eq!(px, [255, 255, 255]); }
        for px in dst.alpha_plane().chunks_exact(3) { assert_eq!(px, [255, 255, 255]); }

        // Composite transparent src onto white dst.  Must leave dst unchanged.
        dst.composite_buffer(&src, 0, 0, None);
        for px in dst.color_plane().chunks_exact(3) {
            assert_eq!(px, [255, 255, 255], "dst colour must survive transparent src composite");
        }
        for px in dst.alpha_plane().chunks_exact(3) {
            assert_eq!(px, [255, 255, 255], "dst alpha must survive transparent src composite");
        }
    }

    /// `composite_buffer`: a fully-opaque src pixel fully replaces the
    /// corresponding dst pixel; a fully-transparent src pixel leaves
    /// dst alone.  This is exactly the Porter-Duff src-over you'd want
    /// for any layer-flush operation, just expressed per-channel.
    #[test]
    fn test_lcd_buffer_composite_buffer_opaque_pixel_replaces_dst() {
        // src: pixel (1,1) painted opaque red, rest transparent.
        let mut src = LcdBuffer::new(3, 3);
        // Manually set pixel (1,1) premultiplied red + full alpha on all three channels.
        let i = (1 * 3 + 1) * 3;
        src.color_plane_mut()[i]     = 255;  // R premult = 1.0 * 1.0 = 1.0 → 255
        src.color_plane_mut()[i + 1] = 0;
        src.color_plane_mut()[i + 2] = 0;
        src.alpha_plane_mut()[i]     = 255;
        src.alpha_plane_mut()[i + 1] = 255;
        src.alpha_plane_mut()[i + 2] = 255;

        // dst: solid white.
        let mut dst = LcdBuffer::new(3, 3);
        dst.clear(Color::white());

        dst.composite_buffer(&src, 0, 0, None);

        // Pixel (1,1) should now be red (fully replaced).
        assert_eq!(&dst.color_plane()[i .. i + 3], &[255, 0, 0],
            "opaque src pixel must fully replace dst pixel's colour");
        assert_eq!(&dst.alpha_plane()[i .. i + 3], &[255, 255, 255],
            "alpha stays full opacity after opaque-src overwrite");

        // Corner (0,0) — src transparent → dst white unchanged.
        assert_eq!(&dst.color_plane()[0 .. 3], &[255, 255, 255],
            "corner should retain dst white (src was transparent there)");
    }

    // ── Legacy tests (opaque content — still valid under new semantics) ──────

    /// Compositing a non-empty mask onto a cleared buffer must leave at
    /// least some pixels modified — proves the path connects.
    #[test]
    fn test_lcd_buffer_composite_mask_deposits_coverage() {
        let mask = rasterize_lcd_mask(
            &font(), "Hi", 20.0, 2.0, 14.0,
            80, 24, &TransAffine::new(),
        );
        let mut buf = LcdBuffer::new(80, 24);
        buf.clear(Color::white());                       // white bg
        let before: u64 = buf.color_plane().iter().map(|&b| b as u64).sum();
        buf.composite_mask(&mask, Color::black(), 0, 0, None); // black text
        let after: u64 = buf.color_plane().iter().map(|&b| b as u64).sum();
        assert!(after < before,
            "compositing dark text onto white bg should reduce summed brightness");
    }

    // ── LcdMaskBuilder + LcdBuffer::fill_path ───────────────────────────────

    /// **Refactor regression** — the legacy `rasterize_lcd_mask_multi`
    /// must produce byte-identical output after being rewritten as a
    /// thin wrapper over `LcdMaskBuilder`.  If the bytes drift, every
    /// cached glyph mask in the existing text path subtly changes and
    /// the equivalence chain to all the prior tests breaks.
    #[test]
    fn test_lcd_mask_builder_matches_legacy_text_path() {
        let f = font();
        let w: u32 = 120;
        let h: u32 = 30;
        let xform  = TransAffine::new();

        // Legacy path.
        let legacy = rasterize_lcd_mask_multi(
            &f, &[("Equiv", 4.0, 18.0)], 22.0, w, h, &xform,
        );

        // Builder path — same setup spelt out by hand.
        let mut builder = LcdMaskBuilder::new(w, h);
        builder.with_paths(&xform, |add| {
            let (mut paths, _) = crate::text::shape_text(&f, "Equiv", 22.0, 4.0, 18.0);
            for p in paths.iter_mut() { add(p); }
        });
        let built = builder.finalize();

        assert_eq!(legacy.width,  built.width);
        assert_eq!(legacy.height, built.height);
        assert_eq!(legacy.data, built.data,
            "LcdMaskBuilder must reproduce rasterize_lcd_mask_multi byte-for-byte");
    }

    /// Non-text smoke test for the path entry point — fill a small
    /// rectangular AGG path through the LCD pipeline and verify pixels
    /// inside the rect are dark, outside are untouched.  Exercises the
    /// builder + composite_mask seam without any text shaping involved.
    #[test]
    fn test_lcd_buffer_fill_path_solid_rect() {
        use agg_rust::basics::PATH_FLAGS_NONE;
        let mut buf = LcdBuffer::new(20, 10);
        buf.clear(Color::white());

        // Rectangle from (5, 3) to (15, 7) in Y-up pixel space.
        let mut path = PathStorage::new();
        path.move_to( 5.0, 3.0);
        path.line_to(15.0, 3.0);
        path.line_to(15.0, 7.0);
        path.line_to( 5.0, 7.0);
        path.close_polygon(PATH_FLAGS_NONE);

        buf.fill_path(&mut path, Color::black(), &TransAffine::new(), None);

        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 20 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };

        // Centre of rect — fully covered, must be black on every channel.
        assert_eq!(pixel(10, 5), (0, 0, 0),
            "interior pixel of solid rect should be fully covered black");
        // Outside rect — untouched, must stay white.
        assert_eq!(pixel(1, 1), (255, 255, 255),
            "pixel outside rect should be untouched");
        assert_eq!(pixel(18, 8), (255, 255, 255),
            "pixel outside rect should be untouched");
    }

    /// **End-to-end equivalence** — proves the new path-driven LcdBuffer
    /// pipeline matches the existing text-driven one for the same glyph
    /// outlines, when both are composited onto the same starting bg.
    /// This is the contract the LcdGfxCtx (Step 2) relies on.
    #[test]
    fn test_lcd_buffer_fill_path_matches_text_pipeline_for_glyphs() {
        let f = font();
        let w: u32 = 80;
        let h: u32 = 24;
        let size = 18.0;
        let baseline = (4.0_f64, 14.0_f64);

        // Way A — legacy: rasterize text mask, composite_mask onto white buffer.
        let legacy_mask = rasterize_lcd_mask_multi(
            &f, &[("ag", baseline.0, baseline.1)], size, w, h, &TransAffine::new(),
        );
        let mut buf_a = LcdBuffer::new(w, h);
        buf_a.clear(Color::white());
        buf_a.composite_mask(&legacy_mask, Color::black(), 0, 0, None);

        // Way B — builder + fill_path: shape glyphs to paths, fill each onto a
        // freshly cleared buffer.  The end result must be pixel-identical.
        let (mut paths, _) = crate::text::shape_text(&f, "ag", size, baseline.0, baseline.1);
        let mut buf_b = LcdBuffer::new(w, h);
        buf_b.clear(Color::white());
        // Each glyph is its own path; compose them in one mask via the builder
        // so adjacent glyphs share the same gray buffer (matches the legacy
        // batching — separate fill_path calls would also work but each would
        // re-run the filter independently and two adjacent glyphs near a
        // pixel boundary could disagree on the filter input by one subpixel).
        let mut builder = LcdMaskBuilder::new(w, h);
        builder.with_paths(&TransAffine::new(), |add| {
            for p in paths.iter_mut() { add(p); }
        });
        let mask_b = builder.finalize();
        buf_b.composite_mask(&mask_b, Color::black(), 0, 0, None);

        assert_eq!(buf_a.color_plane(), buf_b.color_plane(),
            "fill_path-via-builder must match legacy text mask pipeline byte-for-byte");
    }

    /// **Equivalence test** — the load-bearing one for this step.
    ///
    /// Painting `text` two ways must produce identical RGB:
    ///
    ///   A. Existing `composite_lcd_mask` writing into a white RGBA frame.
    ///   B. New `LcdBuffer::clear(white) + composite_mask(black)` route.
    ///
    /// If these diverge, the new buffer-side compositor doesn't match the
    /// existing one and any LcdGfxCtx built on top of it will subtly
    /// disagree with the legacy text path.  This is the contract that
    /// future widget-level migrations rely on.
    #[test]
    fn test_lcd_buffer_composite_matches_composite_lcd_mask() {
        let w: u32 = 100;
        let h: u32 = 28;
        let mask = rasterize_lcd_mask(
            &font(), "Equiv", 22.0, 4.0, 18.0, w, h, &TransAffine::new(),
        );

        // Way A — straight RGBA composite.
        let mut rgba = vec![255u8; (w * h * 4) as usize];
        composite_lcd_mask(&mut rgba, w, h, &mask, Color::black(), 0, 0);

        // Way B — paint into LcdBuffer, then read RGB out directly.
        let mut buf = LcdBuffer::new(w, h);
        buf.clear(Color::white());
        buf.composite_mask(&mask, Color::black(), 0, 0, None);

        for y in 0..h as usize {
            for x in 0..w as usize {
                let ai = (y * w as usize + x) * 4;
                let bi = (y * w as usize + x) * 3;
                let a_rgb = (rgba[ai], rgba[ai + 1], rgba[ai + 2]);
                let b_rgb = (buf.color_plane()[bi], buf.color_plane()[bi + 1], buf.color_plane()[bi + 2]);
                assert_eq!(a_rgb, b_rgb,
                    "RGB mismatch at ({x},{y}): RGBA-path={a_rgb:?} LcdBuffer-path={b_rgb:?}");
            }
        }
    }

}
