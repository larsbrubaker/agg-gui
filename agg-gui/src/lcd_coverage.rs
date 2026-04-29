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

use agg_rust::path_storage::PathStorage;
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::draw_ctx::FillRule;

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
    color: Vec<u8>,
    alpha: Vec<u8>,
    width: u32,
    height: u32,
}

impl LcdBuffer {
    /// Allocate a fully-transparent buffer (color zero, alpha zero
    /// everywhere).  "Transparent" here means the per-channel alpha is
    /// 0, so composite-onto-destination leaves the destination
    /// unchanged wherever no paint has landed yet.
    pub fn new(width: u32, height: u32) -> Self {
        // Safety net: refuse to honour an obviously-pathological size
        // rather than let the allocator try for gigabytes.  Returning a
        // 1×1 buffer means the caller's text doesn't render this
        // frame, but the app keeps running and the offending widget's
        // bounds get clamped naturally on the next layout pass.  A
        // debug build prints the caller info; release silently clamps.
        const MAX_BYTES: usize = 512 * 1024 * 1024; // 512 MB per plane
        let bytes = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(3);
        if bytes > MAX_BYTES {
            #[cfg(debug_assertions)]
            eprintln!(
                "[LcdBuffer] clamped pathological size ({}, {}); \
                 widget bounds likely skipped a size cap",
                width, height,
            );
            return Self {
                color: vec![0u8; 3],
                alpha: vec![0u8; 3],
                width: 1,
                height: 1,
            };
        }
        Self {
            color: vec![0u8; bytes],
            alpha: vec![0u8; bytes],
            width,
            height,
        }
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    pub fn color_plane(&self) -> &[u8] {
        &self.color
    }
    #[inline]
    pub fn alpha_plane(&self) -> &[u8] {
        &self.alpha
    }
    #[inline]
    pub fn color_plane_mut(&mut self) -> &mut [u8] {
        &mut self.color
    }
    #[inline]
    pub fn alpha_plane_mut(&mut self) -> &mut [u8] {
        &mut self.alpha
    }

    /// Both planes mutably in one borrow — for inner loops that update
    /// a pixel's colour and alpha together (image blit, manual composite).
    #[inline]
    pub fn planes_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        (&mut self.color, &mut self.alpha)
    }

    /// Consume the buffer, returning the owned `(color, alpha)` planes
    /// as a pair — used when moving the painted pixels into `Arc`s for
    /// a widget's backbuffer cache or for GPU texture upload.
    pub fn into_planes(self) -> (Vec<u8>, Vec<u8>) {
        (self.color, self.alpha)
    }

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
        let w = self.width as usize;
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
                let a = ra.max(ga).max(ba);
                if a == 0 {
                    continue;
                } // fully transparent → keep RGBA zero
                let af = a as f32 / 255.0;
                let rc = self.color[si] as f32 / 255.0;
                let gc = self.color[si + 1] as f32 / 255.0;
                let bc = self.color[si + 2] as f32 / 255.0;
                out[di] = ((rc / af) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
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
        let a = color.a.clamp(0.0, 1.0);
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
        path: &mut PathStorage,
        color: Color,
        transform: &TransAffine,
        clip: Option<(f64, f64, f64, f64)>,
        fill_rule: FillRule,
    ) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let mut builder = LcdMaskBuilder::new(self.width, self.height)
            .with_clip(clip)
            .with_fill_rule(fill_rule);
        builder.with_paths(transform, |add| {
            add(path);
        });
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
        mask: &LcdMask,
        src: Color,
        dst_x: i32,
        dst_y: i32,
        clip: Option<(i32, i32, i32, i32)>,
    ) {
        if mask.width == 0 || mask.height == 0 {
            return;
        }
        let sa = src.a.clamp(0.0, 1.0);
        let sr = src.r.clamp(0.0, 1.0);
        let sg = src.g.clamp(0.0, 1.0);
        let sb = src.b.clamp(0.0, 1.0);
        if sa == 1.0 {
            self.composite_opaque_mask(mask, sr, sg, sb, dst_x, dst_y, clip);
            return;
        }
        let dst_w_i = self.width as i32;
        let dst_h_i = self.height as i32;
        let dst_w_u = self.width as usize;
        let mw = mask.width as i32;
        let mh = mask.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((cx1, cy1, cx2, cy2)) => {
                (cx1.max(0), cy1.max(0), cx2.min(dst_w_i), cy2.min(dst_h_i))
            }
            None => (0, 0, dst_w_i, dst_h_i),
        };
        if cx1 >= cx2 || cy1 >= cy2 {
            return;
        }

        for my in 0..mh {
            let dy = dst_y + my;
            if dy < cy1 || dy >= cy2 {
                continue;
            }
            let dy_u = dy as usize;
            for mx in 0..mw {
                let dx = dst_x + mx;
                if dx < cx1 || dx >= cx2 {
                    continue;
                }
                let mi = ((my * mw + mx) * 3) as usize;
                // Per-channel effective alpha = src colour alpha × mask coverage.
                let ea_r = sa * (mask.data[mi] as f32 / 255.0);
                let ea_g = sa * (mask.data[mi + 1] as f32 / 255.0);
                let ea_b = sa * (mask.data[mi + 2] as f32 / 255.0);
                if ea_r == 0.0 && ea_g == 0.0 && ea_b == 0.0 {
                    continue;
                }

                let di = (dy_u * dst_w_u + (dx as usize)) * 3;
                // Read existing premult colour + per-channel alpha.
                let bc_r = self.color[di] as f32 / 255.0;
                let bc_g = self.color[di + 1] as f32 / 255.0;
                let bc_b = self.color[di + 2] as f32 / 255.0;
                let ba_r = self.alpha[di] as f32 / 255.0;
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

                self.color[di] = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di] = (ra_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di + 1] = (ra_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di + 2] = (ra_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }

    fn composite_opaque_mask(
        &mut self,
        mask: &LcdMask,
        sr: f32,
        sg: f32,
        sb: f32,
        dst_x: i32,
        dst_y: i32,
        clip: Option<(i32, i32, i32, i32)>,
    ) {
        let sr = (sr * 255.0 + 0.5).clamp(0.0, 255.0) as u16;
        let sg = (sg * 255.0 + 0.5).clamp(0.0, 255.0) as u16;
        let sb = (sb * 255.0 + 0.5).clamp(0.0, 255.0) as u16;
        let dst_w_i = self.width as i32;
        let dst_h_i = self.height as i32;
        let dst_w_u = self.width as usize;
        let mw = mask.width as i32;
        let mh = mask.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((cx1, cy1, cx2, cy2)) => {
                (cx1.max(0), cy1.max(0), cx2.min(dst_w_i), cy2.min(dst_h_i))
            }
            None => (0, 0, dst_w_i, dst_h_i),
        };
        if cx1 >= cx2 || cy1 >= cy2 {
            return;
        }

        for my in 0..mh {
            let dy = dst_y + my;
            if dy < cy1 || dy >= cy2 {
                continue;
            }
            let dy_u = dy as usize;
            for mx in 0..mw {
                let dx = dst_x + mx;
                if dx < cx1 || dx >= cx2 {
                    continue;
                }
                let mi = ((my * mw + mx) * 3) as usize;
                let mr = mask.data[mi] as u16;
                let mg = mask.data[mi + 1] as u16;
                let mb = mask.data[mi + 2] as u16;
                if mr == 0 && mg == 0 && mb == 0 {
                    continue;
                }

                let di = (dy_u * dst_w_u + (dx as usize)) * 3;
                self.color[di] = blend_opaque_channel(sr, self.color[di], mr);
                self.color[di + 1] = blend_opaque_channel(sg, self.color[di + 1], mg);
                self.color[di + 2] = blend_opaque_channel(sb, self.color[di + 2], mb);
                self.alpha[di] = blend_opaque_channel(255, self.alpha[di], mr);
                self.alpha[di + 1] = blend_opaque_channel(255, self.alpha[di + 1], mg);
                self.alpha[di + 2] = blend_opaque_channel(255, self.alpha[di + 2], mb);
            }
        }
    }

    /// Composite an [`LcdMask`] using a per-pixel source colour callback.
    ///
    /// The callback receives destination pixel coordinates in this buffer's
    /// Y-up pixel space.  This keeps the LCD coverage pipeline shared for
    /// solid and gradient fills while allowing colour to vary across the mask.
    pub fn composite_mask_with_color<F>(
        &mut self,
        mask: &LcdMask,
        dst_x: i32,
        dst_y: i32,
        clip: Option<(i32, i32, i32, i32)>,
        mut color_at: F,
    ) where
        F: FnMut(i32, i32) -> Color,
    {
        if mask.width == 0 || mask.height == 0 {
            return;
        }
        let dst_w_i = self.width as i32;
        let dst_h_i = self.height as i32;
        let dst_w_u = self.width as usize;
        let mw = mask.width as i32;
        let mh = mask.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((cx1, cy1, cx2, cy2)) => {
                (cx1.max(0), cy1.max(0), cx2.min(dst_w_i), cy2.min(dst_h_i))
            }
            None => (0, 0, dst_w_i, dst_h_i),
        };
        if cx1 >= cx2 || cy1 >= cy2 {
            return;
        }

        for my in 0..mh {
            let dy = dst_y + my;
            if dy < cy1 || dy >= cy2 {
                continue;
            }
            let dy_u = dy as usize;
            for mx in 0..mw {
                let dx = dst_x + mx;
                if dx < cx1 || dx >= cx2 {
                    continue;
                }
                let mi = ((my * mw + mx) * 3) as usize;
                let src = color_at(dx, dy);
                let sa = src.a.clamp(0.0, 1.0);
                let sr = src.r.clamp(0.0, 1.0);
                let sg = src.g.clamp(0.0, 1.0);
                let sb = src.b.clamp(0.0, 1.0);
                let ea_r = sa * (mask.data[mi] as f32 / 255.0);
                let ea_g = sa * (mask.data[mi + 1] as f32 / 255.0);
                let ea_b = sa * (mask.data[mi + 2] as f32 / 255.0);
                if ea_r == 0.0 && ea_g == 0.0 && ea_b == 0.0 {
                    continue;
                }

                let di = (dy_u * dst_w_u + (dx as usize)) * 3;
                let bc_r = self.color[di] as f32 / 255.0;
                let bc_g = self.color[di + 1] as f32 / 255.0;
                let bc_b = self.color[di + 2] as f32 / 255.0;
                let ba_r = self.alpha[di] as f32 / 255.0;
                let ba_g = self.alpha[di + 1] as f32 / 255.0;
                let ba_b = self.alpha[di + 2] as f32 / 255.0;

                let rc_r = sr * ea_r + bc_r * (1.0 - ea_r);
                let rc_g = sg * ea_g + bc_g * (1.0 - ea_g);
                let rc_b = sb * ea_b + bc_b * (1.0 - ea_b);
                let ra_r = ea_r + ba_r * (1.0 - ea_r);
                let ra_g = ea_g + ba_g * (1.0 - ea_g);
                let ra_b = ea_b + ba_b * (1.0 - ea_b);

                self.color[di] = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di] = (ra_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
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
        src: &LcdBuffer,
        dst_x: i32,
        dst_y: i32,
        clip: Option<(i32, i32, i32, i32)>,
    ) {
        if src.width == 0 || src.height == 0 {
            return;
        }
        let dst_w_i = self.width as i32;
        let dst_h_i = self.height as i32;
        let dst_w_u = self.width as usize;
        let src_w_u = src.width as usize;
        let sw = src.width as i32;
        let sh = src.height as i32;
        let (cx1, cy1, cx2, cy2) = match clip {
            Some((x1, y1, x2, y2)) => (x1.max(0), y1.max(0), x2.min(dst_w_i), y2.min(dst_h_i)),
            None => (0, 0, dst_w_i, dst_h_i),
        };
        if cx1 >= cx2 || cy1 >= cy2 {
            return;
        }

        for sy in 0..sh {
            let dy = dst_y + sy;
            if dy < cy1 || dy >= cy2 {
                continue;
            }
            let dy_u = dy as usize;
            let sy_u = sy as usize;
            for sx in 0..sw {
                let dx = dst_x + sx;
                if dx < cx1 || dx >= cx2 {
                    continue;
                }
                let si = (sy_u * src_w_u + sx as usize) * 3;
                let di = (dy_u * dst_w_u + dx as usize) * 3;

                let sa_r = src.alpha[si] as f32 / 255.0;
                let sa_g = src.alpha[si + 1] as f32 / 255.0;
                let sa_b = src.alpha[si + 2] as f32 / 255.0;
                if sa_r == 0.0 && sa_g == 0.0 && sa_b == 0.0 {
                    continue;
                }

                let sc_r = src.color[si] as f32 / 255.0;
                let sc_g = src.color[si + 1] as f32 / 255.0;
                let sc_b = src.color[si + 2] as f32 / 255.0;

                let bc_r = self.color[di] as f32 / 255.0;
                let bc_g = self.color[di + 1] as f32 / 255.0;
                let bc_b = self.color[di + 2] as f32 / 255.0;
                let ba_r = self.alpha[di] as f32 / 255.0;
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

                self.color[di] = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.color[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                self.alpha[di] = (ra_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
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
        out[dst_y * row_bytes..(dst_y + 1) * row_bytes]
            .copy_from_slice(&src[y * row_bytes..(y + 1) * row_bytes]);
    }
    out
}

#[inline]
fn blend_opaque_channel(src: u16, dst: u8, coverage: u16) -> u8 {
    ((src * coverage + (dst as u16) * (255 - coverage) + 127) / 255) as u8
}

mod mask;
#[cfg(test)]
mod tests;

pub use mask::{
    composite_lcd_mask, identity_xform, rasterize_lcd_mask, rasterize_lcd_mask_multi,
    rasterize_text_lcd_cached, rect_to_pixel_clip, CachedLcdText, LcdMask, LcdMaskBuilder,
};
