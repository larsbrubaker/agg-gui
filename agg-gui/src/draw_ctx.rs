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

/// Free-function bodies for this trait's default methods (arc lowering,
/// pixel snapping, the corner-quad and LCD-plane fallbacks).
pub mod defaults;

use crate::color::Color;
use crate::geometry::Rect;
use crate::text::{Font, TextMetrics};
use crate::theme::Visuals;
use agg_rust::comp_op::CompOp;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::trans_affine::TransAffine;

// Paint definitions live in [`crate::paints`]; re-exported here so existing
// `agg_gui::draw_ctx::{FillRule, RadialGradientPaint, …}` import paths keep
// resolving unchanged.
pub use crate::paints::{
    FillRule, GradientSpread, GradientStop, LinearGradientPaint, PatternPaint, RadialGradientPaint,
};

// ---------------------------------------------------------------------------
// GL paint hook
// ---------------------------------------------------------------------------

// The `GlPaint` trait itself lives in `draw_ctx/gl_paint.rs`; re-exported here
// so `agg_gui::draw_ctx::GlPaint` keeps resolving.
mod gl_paint;
pub use gl_paint::GlPaint;

/// Unified 2-D drawing context.
///
/// All coordinate parameters use the **Y-up, first-quadrant** convention:
/// origin at the bottom-left, positive-Y upward.  This matches `GfxCtx` and
/// the widget tree layout invariant.
pub trait DrawCtx {
    /// Optional escape hatch for widgets that need direct access to a
    /// backend-specific concrete context (e.g. to push a custom GPU draw
    /// command into the deferred command stream).
    ///
    /// The default returns `None`; backends that opt in override to return
    /// `Some(self)`.  Callers must handle the `None` case gracefully — if a
    /// widget falls back through `gl_paint` it works on every backend.
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        None
    }

    // ── State ─────────────────────────────────────────────────────────────────

    fn set_fill_color(&mut self, color: Color);
    fn set_stroke_color(&mut self, color: Color);
    fn set_fill_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
    fn set_fill_radial_gradient(&mut self, _gradient: RadialGradientPaint) {}
    fn set_fill_pattern(&mut self, _pattern: PatternPaint) {}
    fn set_stroke_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
    fn set_stroke_radial_gradient(&mut self, _gradient: RadialGradientPaint) {}
    fn set_stroke_pattern(&mut self, _pattern: PatternPaint) {}
    fn supports_fill_linear_gradient(&self) -> bool {
        false
    }
    fn supports_fill_radial_gradient(&self) -> bool {
        false
    }
    fn supports_fill_pattern(&self) -> bool {
        false
    }
    fn supports_stroke_linear_gradient(&self) -> bool {
        false
    }
    fn supports_stroke_radial_gradient(&self) -> bool {
        false
    }
    fn supports_stroke_pattern(&self) -> bool {
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

    /// Intersect the clip with the current path (canvas-2D `clip()`), using
    /// the current fill rule.
    ///
    /// Everything drawn afterwards is masked by the path, with anti-aliased
    /// edges, until the [`DrawCtx::restore`] that matches the
    /// [`DrawCtx::save`] preceding this call.  Nests: a `clip_path` inside an
    /// already-clipped region intersects with it.
    ///
    /// **The default is a no-op: drawing is NOT clipped at all.**  There is no
    /// bounding-box fallback — a backend that has not implemented path
    /// clipping simply ignores the call, so callers that need to know must ask
    /// [`DrawCtx::supports_clip_path`] and choose their own fallback.
    fn clip_path(&mut self) {}

    /// True when [`DrawCtx::clip_path`] actually masks by the current path.
    /// `false` means the call is a no-op on this backend.
    fn supports_clip_path(&self) -> bool {
        false
    }

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

    /// Append an elliptical arc to the current path, mirroring canvas-2D
    /// `ellipse(cx, cy, rx, ry, rotation, start_angle, end_angle, ccw)`.
    ///
    /// The arc starts with a `move_to` at the start point (callers place it
    /// after `begin_path`, as with [`DrawCtx::circle`]).  Angles are measured
    /// from the +x axis of the ellipse's own — i.e. `rotation`-rotated —
    /// frame.  With `ccw == false` the arc sweeps towards increasing angle,
    /// with `ccw == true` towards decreasing angle.
    ///
    /// A **full turn** — the whole ellipse, with the contour closed — is drawn
    /// only when the sweep covers a revolution *in the requested direction*:
    /// `!ccw && end_angle - start_angle >= 2π`, or
    /// `ccw && start_angle - end_angle >= 2π`.  Any other sweep is normalised
    /// into `[0, 2π)` (`!ccw`) or `(-2π, 0]` (`ccw`), matching the canvas-2D
    /// spec: `ellipse(.., 0.0, -2π, false)` is an empty arc, not a full
    /// ellipse.
    ///
    /// Implemented on top of [`DrawCtx::cubic_to`] with at most one Bézier
    /// segment per quarter turn (see [`defaults::ellipse_ops`]), so every
    /// backend gets it for free; backends with a native primitive may
    /// override it.
    #[allow(clippy::too_many_arguments)]
    fn ellipse(
        &mut self,
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
        start_angle: f64,
        end_angle: f64,
        ccw: bool,
    ) {
        use defaults::EllipseOp;
        for op in defaults::ellipse_ops(cx, cy, rx, ry, rotation, start_angle, end_angle, ccw) {
            match op {
                EllipseOp::MoveTo(x, y) => self.move_to(x, y),
                EllipseOp::CubicTo(c1x, c1y, c2x, c2y, x, y) => {
                    self.cubic_to(c1x, c1y, c2x, c2y, x, y)
                }
                EllipseOp::ClosePath => self.close_path(),
            }
        }
    }

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

    /// **Do not call this from application code, ever.**
    ///
    /// This is the built-in AGG Glyph-Stroke-Vector fallback font — a
    /// stroked vector typeface that pre-dates AGG's real text stack. It
    /// **bypasses every text-rendering facility this framework offers**:
    ///
    /// - no font shaping (no kerning, no proper metrics, no UTF-8 fallback),
    /// - no backbuffer caching (rasterised every frame from scratch),
    /// - no LCD subpixel rendering (always grayscale outlines),
    /// - no theme integration (ignores `Visuals::text_color`),
    /// - no integration with [`crate::font_settings`] (system font,
    ///   font-size scale, hinting, gamma, etc. all ignored).
    ///
    /// It exists only as an internal bootstrap path so the framework can
    /// draw the very first frame before a real [`crate::text::Font`] has
    /// loaded, and so a handful of `agg-rust` reference demos that
    /// specifically test the GSV path stay reproducible. Outside those
    /// two contexts there is **no situation where calling this is
    /// correct** — including for "quick debug text," diagnostics
    /// overlays, perf labels, FPS counters, watermarks, anything.
    ///
    /// Use a [`crate::widgets::Label`] widget instead. `Label` is
    /// backbuffer-cached by default and uses LCD subpixel rendering
    /// when the global toggle is on (which itself defaults to "on at
    /// standard DPI, off at HiDPI" via [`crate::font_settings::lcd_enabled`]).
    /// If you genuinely need imperative text inside a custom widget's
    /// `paint`, call [`set_font`](Self::set_font) + [`fill_text`](Self::fill_text)
    /// — that goes through the real text stack.
    ///
    /// If you find yourself reaching for `fill_text_gsv`, you are
    /// almost certainly looking at a bug in the calling code; do not
    /// add it as a workaround. File an issue against agg-gui.
    fn fill_text_gsv(&mut self, text: &str, x: f64, y: f64, size: f64);

    /// Measure `text` with the current font and font-size settings.
    fn measure_text(&self, text: &str) -> Option<TextMetrics>;

    // ── Transform ─────────────────────────────────────────────────────────────

    /// Current accumulated transform (CTM).
    fn transform(&self) -> TransAffine;

    /// Current transform expressed in the root render target's coordinate
    /// space, even when drawing inside an offscreen layer whose local CTM was
    /// reset to identity. Global overlays use this to submit app-level bounds.
    fn root_transform(&self) -> TransAffine {
        self.transform()
    }

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
        if let Some((fx, fy)) = defaults::pixel_snap_offset(&self.transform()) {
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

    /// Whether this backend can retain named offscreen layers across frames.
    ///
    /// Generic compositing support is enough for isolated opacity groups, but
    /// retained widget backbuffers need a backend-owned surface keyed by ID.
    fn supports_retained_layers(&self) -> bool {
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
    ///
    /// This is a containment clip, not the visual antialiasing edge. Backends
    /// should leave enough room for partially-transparent edge pixels so the
    /// caller's normal alpha coverage can feather corners and edges.
    fn set_layer_rounded_clip(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {}

    /// Declare that the current layer is now covered by opaque content, so
    /// text drawn into it lands on destination alpha 1.
    ///
    /// This re-enables LCD subpixel text inside the layer. Subpixel
    /// coverage is only meaningful against an opaque destination: a fresh
    /// layer texture is cleared to alpha 0, and the per-channel composite
    /// writes colour without alpha, so glyphs over a transparent layer
    /// would stay at alpha 0 and blend additively toward white on pop.
    /// Once an opaque body has been painted, that hazard is gone and
    /// subpixel rendering is as correct as it is on the backbuffer.
    ///
    /// Call it *after* filling the layer, not at push time. Widgets that
    /// composite genuinely translucent content (an opacity fade) must not
    /// call it; they keep the grayscale, alpha-writing text path.
    ///
    /// No-op by default and at the top level, where the target is already
    /// opaque.
    fn set_layer_opaque_backdrop(&mut self, _opaque: bool) {}

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

    /// Blit `data` as a textured quad whose four destination corners
    /// are supplied explicitly. Caller is responsible for choosing the
    /// corners (typically the projection of a 3-D rotated card onto
    /// the 2-D viewport). `corners` is ordered **bottom-left,
    /// bottom-right, top-right, top-left** in agg-gui's Y-up local
    /// coordinate space, and is fed through the current CTM the same
    /// way axis-aligned blits are.
    ///
    /// Backends that can't render a perspective-distorted quad
    /// (software fallback) fall back on the axis-aligned bounding
    /// rect of the four corners.
    fn draw_image_rgba_corners(
        &mut self,
        data: &std::sync::Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        corners: [(f64, f64); 4],
    ) {
        // Default: bounding-rect fallback. The wgpu backend overrides
        // with a real 4-corner textured-quad draw.
        let (x, y, w, h) = defaults::corners_bounding_rect(corners);
        self.draw_image_rgba_arc(data, img_w, img_h, x, y, w, h);
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
    /// `content_version` is a monotone, process-globally-unique content
    /// revision (see `crate::widget::next_content_version`) stamped whenever
    /// the published planes change. Backends that key a GPU texture cache on the
    /// `Arc` buffer *identity* pair it with the pointer so an in-place strip edit
    /// (which keeps the buffer address stable through `Arc::make_mut`) still
    /// forces a re-upload. CPU backends ignore it.
    ///
    /// **Default:** collapses the two planes into a single straight-alpha
    /// RGBA8 image via [`crate::lcd_coverage::collapse_lcd_pixel`]
    /// (Rec.709 luminance-weighted alpha, lifted so the unpremultiply cannot
    /// clamp) and forwards to [`draw_image_rgba`].  Lossy of LCD chroma where
    /// the three channel alphas diverge, but luminance-preserving.  Backends
    /// that want full subpixel quality through the cache override this with a
    /// two-texture shader path.
    ///
    /// This default is live CPU code, not a fallback stub: `LcdGfxCtx` does not
    /// override it, so a nested `BackbufferMode::LcdCoverage` widget blitting
    /// into a parent LCD backbuffer lands here.
    fn draw_lcd_backbuffer_arc(
        &mut self,
        color: &std::sync::Arc<Vec<u8>>,
        alpha: &std::sync::Arc<Vec<u8>>,
        content_version: u64,
        w: u32,
        h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        let _ = content_version; // CPU collapse path is stateless; version unused.
                                 // Collapse to straight-alpha RGBA8 on the fly (see
                                 // `defaults::collapse_lcd_planes`); the row order already matches.
        if let Some(rgba) = defaults::collapse_lcd_planes(color, alpha, w, h) {
            self.draw_image_rgba(&rgba, w, h, dst_x, dst_y, dst_w, dst_h);
        }
    }

    // ── Screenshot capture (GPU-direct path) ──────────────────────────────────
    //
    // Hardware-accelerated screenshot pipeline.  The capture lives on the GPU
    // as a backend-internal texture, so the live preview pane samples it
    // directly with proper downsample filtering — no CPU readback per frame,
    // no re-upload, no mipmap generation in the hot path.  Pixels are pulled
    // back to system memory only when the user actually clicks Save / Copy.
    //
    // Default impls are no-ops returning `false` / empty so the software
    // backend stays unchanged: the screenshot widget falls back to the
    // existing `draw_image_rgba_arc` + Vec<u8> path automatically.

    /// Snapshot the current frame's surface into the backend's internal
    /// screenshot texture (allocating / resizing as needed).  Must be
    /// called inside the active frame, after `end_frame` has flushed the
    /// 2-D render but before the platform shell calls present.
    ///
    /// Returns `true` if the backend supports the capture path.
    fn capture_screenshot(&mut self) -> bool {
        false
    }

    /// True if a previously-captured screenshot is held by the backend
    /// and available for [`Self::draw_captured_screenshot`].
    fn has_captured_screenshot(&self) -> bool {
        false
    }

    /// Dimensions of the held capture, or `None` when no capture exists.
    fn captured_screenshot_size(&self) -> Option<(u32, u32)> {
        None
    }

    /// Draw the held capture into `(dst_x, dst_y, dst_w, dst_h)` using the
    /// backend's preferred filtered sampling.  Returns `true` if the
    /// capture exists and was drawn.
    fn draw_captured_screenshot(
        &mut self,
        _dst_x: f64,
        _dst_y: f64,
        _dst_w: f64,
        _dst_h: f64,
    ) -> bool {
        false
    }

    /// Read the held capture's pixels back to CPU memory as Y-down RGBA8 —
    /// for Save / Copy.  This is intentionally a single-shot synchronous
    /// readback; widgets should NOT call this every frame.  Returns
    /// `(empty, 0, 0)` on backends without a capture or without GPU
    /// readback support.
    fn read_captured_screenshot(&mut self) -> (Vec<u8>, u32, u32) {
        (Vec::new(), 0, 0)
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
