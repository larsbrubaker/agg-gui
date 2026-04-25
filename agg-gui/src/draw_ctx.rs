//! `DrawCtx` — the unified drawing interface shared by the software (`GfxCtx`)
//! and hardware (`GlGfxCtx`) rendering paths.
//!
//! Every `Widget::paint` implementation receives a `&mut dyn DrawCtx`.  The
//! concrete type is either:
//!
//! - **`GfxCtx`** — software AGG rasteriser (used when a widget opts into a
//!   back-buffer or when GL is unavailable).
//! - **`GlGfxCtx`** — hardware GL path: shapes are tessellated via `tess2`
//!   and submitted as GPU draw calls.
//!
//! The two implementations expose *identical* method signatures so that widget
//! `paint` bodies are unchanged regardless of the render target.

use std::sync::Arc;

use crate::color::Color;
use crate::geometry::Rect;
use crate::text::{Font, TextMetrics};
use crate::theme::Visuals;
use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;

/// Fill rule used when rasterizing closed paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    /// Non-zero winding rule.
    NonZero,
    /// Even-odd parity rule.
    EvenOdd,
}

impl Default for FillRule {
    fn default() -> Self {
        Self::NonZero
    }
}

/// How a gradient behaves outside the normalized `0..=1` range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientSpread {
    /// Clamp to the nearest edge stop.
    Pad,
    /// Mirror each repeated interval.
    Reflect,
    /// Repeat the gradient ramp.
    Repeat,
}

impl Default for GradientSpread {
    fn default() -> Self {
        Self::Pad
    }
}

/// One color stop in a bridge-level gradient paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub offset: f64,
    pub color: Color,
}

/// Linear gradient fill paint expressed in local drawing coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct LinearGradientPaint {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub transform: TransAffine,
    pub spread: GradientSpread,
    pub stops: Vec<GradientStop>,
}

impl LinearGradientPaint {
    pub fn sample(&self, mut x: f64, mut y: f64) -> Color {
        if self.stops.is_empty() {
            return Color::transparent();
        }

        self.transform.inverse_transform(&mut x, &mut y);

        let dx = self.x2 - self.x1;
        let dy = self.y2 - self.y1;
        let len2 = dx * dx + dy * dy;
        let t = if len2 > f64::EPSILON {
            ((x - self.x1) * dx + (y - self.y1) * dy) / len2
        } else {
            0.0
        };
        let t = apply_spread(t, self.spread);

        if t <= self.stops[0].offset {
            return self.stops[0].color;
        }
        for pair in self.stops.windows(2) {
            let a = pair[0];
            let b = pair[1];
            if t <= b.offset {
                let span = (b.offset - a.offset).max(f64::EPSILON);
                let u = ((t - a.offset) / span).clamp(0.0, 1.0) as f32;
                return lerp_color(a.color, b.color, u);
            }
        }
        self.stops[self.stops.len() - 1].color
    }
}

fn apply_spread(t: f64, spread: GradientSpread) -> f64 {
    match spread {
        GradientSpread::Pad => t.clamp(0.0, 1.0),
        GradientSpread::Repeat => t - t.floor(),
        GradientSpread::Reflect => {
            let period = t.rem_euclid(2.0);
            if period <= 1.0 {
                period
            } else {
                2.0 - period
            }
        }
    }
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

// ---------------------------------------------------------------------------
// GL paint hook
// ---------------------------------------------------------------------------

/// Trait for widgets that want to render 3-D (or other GPU) content inline
/// during the widget paint pass.
///
/// `DrawCtx::gl_paint` calls this with an opaque `gl` handle — implementations
/// downcast it to `glow::Context` (or whatever GL type the platform provides).
/// The software `GfxCtx` never calls `paint`; see [`DrawCtx::gl_paint`].
pub trait GlPaint {
    /// Execute GPU draw calls for the widget's 3-D content.
    ///
    /// `gl` — opaque platform GL context; downcast via `std::any::Any`.
    /// `screen_rect` — Y-up screen-space rect for this widget (for viewport/scissor).
    /// `full_w`, `full_h` — full viewport dimensions (for restoring after).
    /// `parent_clip` — current framework scissor rect `[x, y, w, h]` in GL/Y-up
    ///   pixels, or `None` if no clip is active.  Implementations **must intersect**
    ///   any scissor they set with this rect so that parent widget clips (e.g. a
    ///   collapsed window) correctly hide GPU-rendered content.
    fn gl_paint(
        &mut self,
        gl: &dyn std::any::Any,
        screen_rect: Rect,
        full_w: i32,
        full_h: i32,
        parent_clip: Option<[i32; 4]>,
    );
}

/// Unified 2-D drawing context.
///
/// All coordinate parameters use the **Y-up, first-quadrant** convention:
/// origin at the bottom-left, positive-Y upward.  This matches `GfxCtx` and
/// the widget tree layout invariant.
pub trait DrawCtx {
    // ── State ─────────────────────────────────────────────────────────────────

    fn set_fill_color(&mut self, color: Color);
    fn set_stroke_color(&mut self, color: Color);
    fn set_fill_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
    fn supports_fill_linear_gradient(&self) -> bool {
        false
    }
    fn set_line_width(&mut self, w: f64);
    fn set_line_join(&mut self, join: LineJoin);
    fn set_line_cap(&mut self, cap: LineCap);
    fn set_miter_limit(&mut self, limit: f64);
    fn set_line_dash(&mut self, dashes: &[f64], offset: f64);
    fn set_blend_mode(&mut self, mode: CompOp);
    fn set_global_alpha(&mut self, alpha: f64);
    fn set_fill_rule(&mut self, rule: FillRule);

    // ── Font ──────────────────────────────────────────────────────────────────

    fn set_font(&mut self, font: Arc<Font>);
    fn set_font_size(&mut self, size: f64);

    // ── Clipping ──────────────────────────────────────────────────────────────

    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64);
    fn reset_clip(&mut self);

    // ── Clear ─────────────────────────────────────────────────────────────────

    /// Fill the entire render target with `color`, ignoring the current clip.
    fn clear(&mut self, color: Color);

    // ── Path building ─────────────────────────────────────────────────────────

    fn begin_path(&mut self);
    fn move_to(&mut self, x: f64, y: f64);
    fn line_to(&mut self, x: f64, y: f64);
    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64);
    fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64);
    fn arc_to(&mut self, cx: f64, cy: f64, r: f64, start_angle: f64, end_angle: f64, ccw: bool);

    /// Add a full circle contour to the current path.
    fn circle(&mut self, cx: f64, cy: f64, r: f64);

    /// Add an axis-aligned rectangle contour to the current path.
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64);

    /// Add a rounded-rectangle contour to the current path.
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64);

    fn close_path(&mut self);

    // ── Path drawing ──────────────────────────────────────────────────────────

    fn fill(&mut self);
    fn stroke(&mut self);
    fn fill_and_stroke(&mut self);

    /// Submit **pre-tessellated** AA triangles with per-vertex coverage
    /// (`x`, `y`, `alpha`) and triangle indices.
    ///
    /// This is the fast path for callers that tessellate their geometry
    /// ONCE at load time (e.g. the Lion demo, SVG icons): they do the
    /// `tessellate_path_aa` pass themselves, cache the vertex+index
    /// buffers, then submit them every frame with only a cheap CPU
    /// transform applied to the x/y components.  Compared to issuing
    /// `move_to` / `line_to` / `fill` every frame, this keeps the polygon
    /// set deterministic (no tess2 re-running on subtly-different
    /// coordinates), avoids thousands of re-tessellations per frame, and
    /// produces identical output regardless of the widget's transform.
    ///
    /// Vertices are `(x_logical_pixels, y_logical_pixels, alpha_0_to_1)`.
    /// `alpha` is multiplied into the supplied `color.a` in the AA shader
    /// so halo-strip edge AA survives this fast path.
    ///
    /// The software `GfxCtx` ignores the alpha attribute and rasterises
    /// each triangle as a solid fill — correct but without edge AA, which
    /// matches the software path's existing stroke/fill behaviour.
    fn draw_triangles_aa(
        &mut self,
        vertices: &[[f32; 3]],
        indices: &[u32],
        color: crate::color::Color,
    );

    // ── Text ──────────────────────────────────────────────────────────────────

    /// Draw `text` with the bottom of the baseline at `(x, y)`.
    fn fill_text(&mut self, text: &str, x: f64, y: f64);

    /// Draw `text` using the built-in AGG Glyph-Stroke-Vector font at `size`
    /// pixels.  Useful before a proper font is loaded.
    fn fill_text_gsv(&mut self, text: &str, x: f64, y: f64, size: f64);

    /// Measure `text` with the current font and font-size settings.
    fn measure_text(&self, text: &str) -> Option<TextMetrics>;

    // ── Transform ─────────────────────────────────────────────────────────────

    /// Current accumulated transform (CTM).
    fn transform(&self) -> TransAffine;

    fn save(&mut self);
    fn restore(&mut self);
    fn translate(&mut self, tx: f64, ty: f64);
    fn rotate(&mut self, radians: f64);
    fn scale(&mut self, sx: f64, sy: f64);
    fn set_transform(&mut self, m: TransAffine);
    fn reset_transform(&mut self);

    /// **Opt-in** pixel snapping.  Strips the fractional part of the current
    /// CTM translation so subsequent integer-coordinate `rect` / `fill` /
    /// `stroke` / `draw_image_rgba*` calls land exactly on the physical pixel
    /// grid — no AA fringe on edges, no LINEAR-filter blur on 1:1 texture
    /// blits.
    ///
    /// Call this ONLY when the widget genuinely wants pixel-aligned drawing
    /// (text backbuffers, pixel-alignment diagnostics, crisp UI strokes).
    /// Sub-pixel positioning remains the default — e.g. a smooth-scrolling
    /// panel or an animated marker may legitimately want a fractional offset.
    /// Typical usage:
    /// ```ignore
    /// ctx.save();
    /// ctx.snap_to_pixel();
    /// ctx.rect(0.0, 0.0, 10.0, 10.0);
    /// ctx.fill();
    /// ctx.restore();
    /// ```
    ///
    /// Only the translation component is affected; rotations and non-uniform
    /// scales pass through untouched (pixel alignment under those transforms
    /// isn't well defined, and forcing a snap would visibly jitter rotated
    /// content).
    fn snap_to_pixel(&mut self) {
        let t = self.transform();
        let fx = t.tx - t.tx.floor();
        let fy = t.ty - t.ty.floor();
        if fx != 0.0 || fy != 0.0 {
            self.translate(-fx, -fy);
        }
    }

    // ── Compositing layers ────────────────────────────────────────────────────

    /// Begin a new transparent compositing layer of the given pixel dimensions.
    ///
    /// All subsequent drawing (by this widget and its descendants) is redirected
    /// into the new layer until [`pop_layer`] is called.  Layers nest: each
    /// `push_layer` must be matched by exactly one `pop_layer`.
    ///
    /// The current accumulated transform records the layer's screen-space origin;
    /// drawing inside the layer uses a fresh local-space transform (origin 0,0).
    ///
    /// Implementations that do not support layers (e.g. the GL path) may leave
    /// this as a no-op — the widget renders pass-through into the parent target.
    fn push_layer(&mut self, _width: f64, _height: f64) {}

    /// Whether this backend implements real offscreen compositing layers.
    ///
    /// The default is `false` so widgets can opt into layer-based rendering
    /// without forcing every backend to pay for, or emulate, that feature.
    fn supports_compositing_layers(&self) -> bool {
        false
    }

    /// Begin a new transparent compositing layer that will be multiplied by
    /// `alpha` when composited back into the parent target.
    ///
    /// Backends that do not support layer alpha can fall back to `push_layer`;
    /// callers gate this through [`supports_compositing_layers`].
    fn push_layer_with_alpha(&mut self, width: f64, height: f64, _alpha: f64) {
        self.push_layer(width, height);
    }

    /// Constrain subsequent drawing in the current layer to a rounded-rect
    /// mask. Used by window layers after shadows are drawn so chrome/content
    /// cannot write into rounded transparent corners.
    fn set_layer_rounded_clip(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {}

    /// Composite a previously retained backend layer. Returns `true` when
    /// the backend had a retained surface for `key` and drew it.
    fn composite_retained_layer(
        &mut self,
        _key: u64,
        _width: f64,
        _height: f64,
        _alpha: f64,
    ) -> bool {
        false
    }

    /// Begin rendering into a retained backend layer identified by `key`.
    /// Backends that do not retain layers may fall back to a transient layer.
    fn push_retained_layer_with_alpha(&mut self, _key: u64, width: f64, height: f64, alpha: f64) {
        self.push_layer_with_alpha(width, height, alpha);
    }

    /// Composite the current layer back into the previous render target using
    /// SrcOver alpha blending, then discard the layer.
    ///
    /// Must be called after a matching `push_layer`.  Unmatched calls are ignored.
    fn pop_layer(&mut self) {}

    // ── GL / GPU content ──────────────────────────────────────────────────────

    /// Render GPU content (3-D scene, video frame, etc.) inline at the correct
    /// painter-order position.
    ///
    /// `screen_rect` is the widget's screen-space rect in Y-up coordinates
    /// (i.e. `ctx.transform()` origin + `widget.bounds().size`).
    ///
    /// The GL implementation executes `painter.gl_paint()` immediately so that
    /// any 2-D widgets painted after this call naturally overdraw the GPU
    /// content — correct back-to-front ordering with no post-frame fixup.
    ///
    /// The **software (`GfxCtx`) path is a no-op**: widgets should draw a 2-D
    /// placeholder before calling this method so the software render has
    /// something visible.
    fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}

    // ── LCD mask compositing ──────────────────────────────────────────────────

    /// Composite a pre-rasterized LCD subpixel mask onto the current
    /// render target, mixing `src_color` into the destination through
    /// per-channel coverage.
    ///
    /// `mask` is three bytes per pixel (`cov_r`, `cov_g`, `cov_b`) as
    /// produced by [`crate::text_lcd::rasterize_lcd_mask`].  The caller
    /// specifies `(dst_x, dst_y)` in local coordinates (Y-up in our
    /// convention) and `mask_w × mask_h` to tell the backend the mask's
    /// dimensions.
    ///
    /// Per-channel source-over blend:
    /// ```text
    /// dst.r = src.r * mask.r + dst.r * (1 - mask.r)
    /// dst.g = src.g * mask.g + dst.g * (1 - mask.g)
    /// dst.b = src.b * mask.b + dst.b * (1 - mask.b)
    /// ```
    ///
    /// **This is the universal "composite LCD text onto arbitrary bg"
    /// primitive** — it replaces the prior walk / sample / pre-fill
    /// approach.  Software ctx implements it as an inner-loop blend; the
    /// GL ctx implements it via a dual-source-blend fragment shader.
    /// Backends that haven't wired it yet use the default no-op, which
    /// makes callers fall back to grayscale AA.
    fn draw_lcd_mask(
        &mut self,
        _mask: &[u8],
        _mask_w: u32,
        _mask_h: u32,
        _src_color: Color,
        _dst_x: f64,
        _dst_y: f64,
    ) {
    }

    /// Arc-keyed variant so GL backends can cache the uploaded texture
    /// on the `Arc`'s pointer identity — one `glTexImage2D` per unique
    /// raster, lifetime tied to the mask's strong-ref count.  Software
    /// backends fall through to the slice path.
    fn draw_lcd_mask_arc(
        &mut self,
        mask: &std::sync::Arc<Vec<u8>>,
        mask_w: u32,
        mask_h: u32,
        src_color: Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        self.draw_lcd_mask(mask.as_slice(), mask_w, mask_h, src_color, dst_x, dst_y);
    }

    /// Returns `true` if this backend supports [`draw_lcd_mask`] — i.e.
    /// it can composite per-channel LCD coverage onto the active target.
    /// Label queries this to decide between the LCD and grayscale AA
    /// paths; a backend that returns `false` will never see LCD text.
    fn has_lcd_mask_composite(&self) -> bool {
        false
    }

    // ── Image blitting ────────────────────────────────────────────────────────

    /// Returns `true` if this context implements `draw_image_rgba` with actual
    /// pixel blitting.  `Label` (and any other widget that uses a software
    /// backbuffer) gates its cache path on this method so it can fall back to
    /// direct `fill_text()` on render targets that don't support blitting
    /// (e.g. the GL path).
    ///
    /// Default: `false`.  Override to `true` in `GfxCtx`.
    fn has_image_blit(&self) -> bool {
        false
    }

    /// Draw raw RGBA pixel data into `dst_rect` (Y-up local coordinates).
    ///
    /// `data` must be `img_w * img_h * 4` bytes of tightly-packed RGBA8 data
    /// in row-major order, **top-row first** (Y-down image storage convention).
    /// The image is scaled to fit `(dst_x, dst_y, dst_w, dst_h)`.
    ///
    /// Default implementation: no-op (GL path or software paths that do not
    /// implement blitting can leave this as a placeholder).
    fn draw_image_rgba(
        &mut self,
        data: &[u8],
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        let _ = (data, img_w, img_h, dst_x, dst_y, dst_w, dst_h);
    }

    /// Same as [`draw_image_rgba`] but accepts an `Arc<Vec<u8>>` so the GL
    /// backend can key its texture cache on the `Arc`'s pointer identity and
    /// hold a `Weak` ref for automatic cleanup when the underlying buffer is
    /// dropped — the pattern MatterCAD implements with C# `ConditionalWeakTable`.
    ///
    /// Used by `Label` (and future glyph-atlas consumers) in tandem with the
    /// crate-level [`image_cache`](crate::image_cache) so that rebuilt widget
    /// trees with unchanged content never re-rasterize OR re-upload.
    ///
    /// Default implementation: forward to [`draw_image_rgba`] via slice
    /// borrow.  Software backends don't benefit from GPU texture caching so
    /// the default is usually fine; the GL backend overrides.
    fn draw_image_rgba_arc(
        &mut self,
        data: &std::sync::Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        self.draw_image_rgba(data.as_slice(), img_w, img_h, dst_x, dst_y, dst_w, dst_h);
    }

    // ── LCD backbuffer blit ───────────────────────────────────────────────────

    /// Composite a two-plane `LcdCoverage`-mode backbuffer onto the active
    /// render target at `(dst_x, dst_y)` with size `(dst_w, dst_h)` (in
    /// local coords).  Inputs are two `Arc<Vec<u8>>`, each 3 bytes per
    /// pixel, **top-row-first**:
    ///
    /// - `color`: premultiplied per-channel RGB.
    /// - `alpha`: per-channel alpha (coverage).
    ///
    /// The compositor applies per-channel premultiplied src-over:
    ///
    /// ```text
    /// dst.ch := src.color_ch + dst.ch * (1 - src.alpha_ch)
    /// ```
    ///
    /// which preserves LCD subpixel chroma through the cache round-trip.
    /// Used by [`crate::widget::paint_subtree_backbuffered`] when a widget's
    /// [`crate::widget::BackbufferMode::LcdCoverage`] cache is ready to
    /// composite onto its parent.
    ///
    /// **Default:** collapses the two planes into a single straight-alpha
    /// RGBA8 image (max of channel alphas, divided back to straight colour)
    /// and forwards to [`draw_image_rgba`].  Correct for any content where
    /// the three channel alphas agree; lossy of LCD chroma where they
    /// diverge.  Backends that want full subpixel quality through the
    /// cache override this with a two-texture shader path.
    fn draw_lcd_backbuffer_arc(
        &mut self,
        color: &std::sync::Arc<Vec<u8>>,
        alpha: &std::sync::Arc<Vec<u8>>,
        w: u32,
        h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        // Collapse to straight-alpha RGBA8 on the fly.  Matches the same
        // math `LcdBuffer::to_rgba8_top_down_collapsed` uses internally,
        // except applied to a top-down pair rather than a Y-up pair.
        let w_u = w as usize;
        let h_u = h as usize;
        if color.len() < w_u * h_u * 3 || alpha.len() < w_u * h_u * 3 {
            return;
        }
        let mut rgba = vec![0u8; w_u * h_u * 4];
        for i in 0..(w_u * h_u) {
            let ci = i * 3;
            let ra = alpha[ci];
            let ga = alpha[ci + 1];
            let ba = alpha[ci + 2];
            let a = ra.max(ga).max(ba);
            if a == 0 {
                continue;
            }
            let af = a as f32 / 255.0;
            let rc = color[ci] as f32 / 255.0;
            let gc = color[ci + 1] as f32 / 255.0;
            let bc = color[ci + 2] as f32 / 255.0;
            let di = i * 4;
            rgba[di] = ((rc / af) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            rgba[di + 1] = ((gc / af) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            rgba[di + 2] = ((bc / af) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            rgba[di + 3] = a;
        }
        self.draw_image_rgba(&rgba, w, h, dst_x, dst_y, dst_w, dst_h);
    }

    // ── Theme / Visuals ───────────────────────────────────────────────────────

    /// Return the currently-active [`Visuals`] palette.
    ///
    /// Delegates to [`crate::theme::current_visuals`], which reads the
    /// thread-local set by [`crate::theme::set_visuals`].  Widget `paint()`
    /// implementations call this to get colours instead of hardcoding them.
    fn visuals(&self) -> Visuals {
        crate::theme::current_visuals()
    }
}
