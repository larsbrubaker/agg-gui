//! [`LcdMaskBuilder`]: the rasterization stage of the LCD pipeline.
//!
//! Rasterizes one or more AGG paths into a 3×-horizontally-supersampled
//! gray coverage buffer, then hands it to [`super::filter`] for the packed
//! 3-byte mask.  Split out of [`super::mask`], which builds the text,
//! bounded-mask and composite entry points on top of this builder.

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
use super::mask::LcdMask;
use crate::curve_flattening::curve_approximation_scale;
use crate::draw_ctx::FillRule;

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
#[derive(Clone)]
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
    /// Whether the AGG rasterizer gets its edge clip box.  Always true in
    /// production; tests turn it off to get the unclipped ground truth the
    /// box must reproduce byte for byte.
    clip_rasterizer: bool,
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
            clip_rasterizer: true,
        }
    }

    /// A builder whose rasterizer has no edge clip box at all: the ground
    /// truth the clip box has to reproduce byte for byte.
    #[cfg(test)]
    pub(super) fn new_with_unclipped_rasterizer(mask_w: u32, mask_h: u32) -> Self {
        let mut builder = Self::new(mask_w, mask_h);
        builder.clip_rasterizer = false;
        builder
    }

    /// The 3×-wide gray coverage buffer accumulated so far (Y-up).
    #[cfg(test)]
    pub(super) fn gray_buffer(&self) -> &[u8] {
        &self.gray
    }

    /// Width of the gray buffer in subpixels (`3 × mask_w`).
    #[cfg(test)]
    pub(super) fn gray_width(&self) -> u32 {
        self.gray_w
    }

    /// Height of the gray buffer in rows (`mask_h`).
    #[cfg(test)]
    pub(super) fn gray_height(&self) -> u32 {
        self.gray_h
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
        rasterize_paths_into_gray(self, transform, f);
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
fn rasterize_paths_into_gray<F>(builder: &mut LcdMaskBuilder, transform: &TransAffine, f: F)
where
    F: FnOnce(&mut dyn FnMut(&mut PathStorage)),
{
    let (gray_w, gray_h) = (builder.gray_w, builder.gray_h);
    let (clip, fill_rule) = (builder.clip, builder.fill_rule);
    let clip_rasterizer = builder.clip_rasterizer;
    let gray = &mut builder.gray;
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
    if clip_rasterizer {
        apply_overflow_clip_box(&mut ras, gray_w, gray_h);
    }
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
        // Flattened for the supersampled transform, so the 3x-wide coverage
        // has the same half-pixel curve tolerance the RGBA fill gets.
        curves.set_approximation_scale(curve_approximation_scale(&xform));
        let mut tx = ConvTransform::new(&mut curves, xform);
        ras.reset();
        ras.add_path(&mut tx, 0);
        render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &cov_color);
    };
    f(&mut add);
}

/// How far outside the gray buffer, in pixels, the rasterizer's overflow
/// clip box sits.  Everything visible is bounded by the renderer clip above,
/// so the box never needs to touch real geometry.
const OVERFLOW_CLIP_PAD_PIXELS: i32 = 4096;

/// Hard cap on the span the overflow clip box may permit on either axis, so
/// a clipped edge's 24.8 `dx`/`dy` stays under the cell rasterizer's
/// subdivision limit (with two pixels to spare for the clipper's rounding).
const MAX_CLIP_SPAN_PIXELS: i32 = 16382;

/// Give the rasterizer a clip box that exists purely as overflow protection
/// and as a bound on the cells a far-off edge can generate, thousands of
/// pixels clear of the gray buffer.  It is not on the buffer edge because
/// clipping real geometry is not free: AGG's clipper rounds the crossing
/// point (`mul_div` → `iround`) and the cell rasterizer re-seeds its
/// interpolation from it, which walks the whole edge up to 1/256 px off —
/// moving interior gray bytes of any edge that crosses the box.  Geometry
/// that stays inside is handed to the rasterizer verbatim, so every edge on
/// or near the buffer rasterizes exactly as with no box at all.
fn apply_overflow_clip_box(ras: &mut RasterizerScanlineAa, gray_w: u32, gray_h: u32) {
    let gw = gray_w.min(i32::MAX as u32) as i32;
    let gh = gray_h.min(i32::MAX as u32) as i32;
    let pad_x = OVERFLOW_CLIP_PAD_PIXELS
        .min((MAX_CLIP_SPAN_PIXELS - gw) / 2)
        .max(0);
    let pad_y = OVERFLOW_CLIP_PAD_PIXELS
        .min((MAX_CLIP_SPAN_PIXELS - gh) / 2)
        .max(0);
    ras.clip_box(
        -pad_x as f64,
        -pad_y as f64,
        (gw + pad_x) as f64,
        (gh + pad_y) as f64,
    );
}

fn to_agg_fill_rule(rule: FillRule) -> FillingRule {
    match rule {
        FillRule::NonZero => FillingRule::NonZero,
        FillRule::EvenOdd => FillingRule::EvenOdd,
    }
}
