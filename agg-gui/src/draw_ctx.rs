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

// The long method docs of `DrawCtx` live in `draw_ctx/docs/<method>.md`, pulled in with
// `#[doc = include_str!(..)]`, so the trait stays under the file-size limit without splitting
// it (a supertrait split would break every `impl DrawCtx`).

// The GL paint hook (`draw_ctx/gl_paint.rs`) and the paint definitions are
// re-exported from `draw_ctx/reexports.rs` under their old paths.
mod gl_paint;
mod image_sampling;
mod paint_nesting;
mod reexports;
pub use reexports::*;

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

    /// Anti-alias the edges of filled and stroked shapes (`true`, the
    /// default) or draw only whole pixels, those at least half covered.
    /// Saved and restored with the rest of the state. A rendering hint, as
    /// SVG's `shape-rendering` is: the software `GfxCtx` honours it; a
    /// backend without an aliased path keeps anti-aliasing.
    fn set_anti_alias(&mut self, _anti_alias: bool) {}

    // ── Font ──────────────────────────────────────────────────────────────────

    fn set_font(&mut self, font: Arc<Font>);
    fn set_font_size(&mut self, size: f64);

    // ── Clipping ──────────────────────────────────────────────────────────────

    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64);
    fn reset_clip(&mut self);

    #[doc = include_str!("draw_ctx/docs/clip_path.md")]
    fn clip_path(&mut self) {}

    /// True when [`DrawCtx::clip_path`] actually masks by the current path.
    /// `false` means the call is a no-op on this backend.
    fn supports_clip_path(&self) -> bool {
        false
    }

    // ── Clear ─────────────────────────────────────────────────────────────────

    /// Fill the entire render target with `color`, ignoring the current clip.
    fn clear(&mut self, color: Color);

    #[doc = include_str!("draw_ctx/docs/clear_clip.md")]
    fn clear_clip(&mut self, color: Color) {
        defaults::clear_clip(self, color);
    }

    /// Replace the pixels of the rectangle `x, y, w, h`, given in the
    /// target's own pixels (y up) and not through the transform, that lie
    /// inside the current clip with `color` (agg-sharp's
    /// `Graphics2D.Clear(RectangleDouble, Color)`).
    fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64, color: Color) {
        defaults::clear_rect(self, x, y, w, h, color);
    }

    // ── Path building ─────────────────────────────────────────────────────────

    fn begin_path(&mut self);
    fn move_to(&mut self, x: f64, y: f64);
    fn line_to(&mut self, x: f64, y: f64);
    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64);
    fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64);

    #[doc = include_str!("draw_ctx/docs/arc_to.md")]
    fn arc_to(&mut self, cx: f64, cy: f64, r: f64, start_angle: f64, end_angle: f64, ccw: bool);

    /// Add a full circle contour to the current path.  It is always its own
    /// closed subpath: unlike [`DrawCtx::arc_to`] it never joins an open
    /// subpath that precedes it.
    fn circle(&mut self, cx: f64, cy: f64, r: f64);

    #[doc = include_str!("draw_ctx/docs/ellipse.md")]
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
        defaults::ellipse(self, cx, cy, rx, ry, rotation, start_angle, end_angle, ccw)
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

    #[doc = include_str!("draw_ctx/docs/draw_triangles_aa.md")]
    fn draw_triangles_aa(
        &mut self,
        vertices: &[[f32; 3]],
        indices: &[u32],
        color: crate::color::Color,
    );

    // ── Text ──────────────────────────────────────────────────────────────────

    /// Draw `text` with the bottom of the baseline at `(x, y)`.
    fn fill_text(&mut self, text: &str, x: f64, y: f64);

    #[doc = include_str!("draw_ctx/docs/fill_text_gsv.md")]
    fn fill_text_gsv(&mut self, text: &str, x: f64, y: f64, size: f64);

    /// Measure `text` with the current font and font-size settings.
    fn measure_text(&self, text: &str) -> Option<TextMetrics>;

    // ── Transform ─────────────────────────────────────────────────────────────

    /// Current accumulated transform (CTM).
    fn transform(&self) -> TransAffine;

    #[doc = include_str!("draw_ctx/docs/root_transform.md")]
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

    #[doc = include_str!("draw_ctx/docs/snap_to_pixel.md")]
    fn snap_to_pixel(&mut self) {
        defaults::snap_to_pixel(self);
    }

    // ── Compositing layers ────────────────────────────────────────────────────

    #[doc = include_str!("draw_ctx/docs/push_layer.md")]
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

    #[doc = include_str!("draw_ctx/docs/set_layer_opaque_backdrop.md")]
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

    /// How many `save()` levels and layers are open right now. Paint
    /// containment reads it before calling into a widget and hands it to
    /// [`unwind_paint_nesting`](Self::unwind_paint_nesting) if that widget
    /// panicked, so a draw that unwound between a `save` (or `push_layer`) and
    /// its `restore` (or `pop_layer`) cannot leak the level into later widgets.
    /// The default reports nothing open: a context that keeps no stacks has
    /// nothing to leak; every renderer in the workspace overrides both.
    fn paint_nesting(&self) -> PaintNesting {
        PaintNesting::default()
    }

    /// Close every layer and `save()` level opened since
    /// [`paint_nesting`](Self::paint_nesting) returned `to`, innermost first
    /// (layers composite as `pop_layer` does). No-op when nothing is open past it.
    fn unwind_paint_nesting(&mut self, _to: PaintNesting) {}

    // ── GL / GPU content ──────────────────────────────────────────────────────

    #[doc = include_str!("draw_ctx/docs/gl_paint.md")]
    fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}

    // ── LCD mask compositing ──────────────────────────────────────────────────

    #[doc = include_str!("draw_ctx/docs/draw_lcd_mask.md")]
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

    /// Whether an icon's 1:1 LCD-coverage composite
    /// ([`draw_lcd_backbuffer_arc`](DrawCtx::draw_lcd_backbuffer_arc)) may land
    /// on the device-pixel rect `left..right × bottom..top` (half-open, Y-up)
    /// and keep its subpixel coverage: on RGBA, LCD is on and every pixel it
    /// can touch is opaque (one alpha cannot carry three channel coverages);
    /// an LCD-buffer target always takes it.  agg-sharp's
    /// `CanCompositeLcdBuffer` + `TakesLcdImageOver`; see [`crate::IconImage`].
    /// Default `false`: the icon takes its plain blit.
    fn takes_lcd_image_over(&mut self, _left: i32, _bottom: i32, _right: i32, _top: i32) -> bool {
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

    #[doc = include_str!("draw_ctx/docs/draw_image_rgba.md")]
    // Public API: the signature is shared with downstream callers.
    #[allow(clippy::too_many_arguments)]
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

    #[doc = include_str!("draw_ctx/docs/draw_image_rgba_arc.md")]
    // Public API: the signature is shared with downstream callers.
    #[allow(clippy::too_many_arguments)]
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

    #[doc = include_str!("draw_ctx/docs/draw_image_rgba_arc_sampled.md")]
    // Public API: the signature is shared with downstream callers.
    #[allow(clippy::too_many_arguments)]
    fn draw_image_rgba_arc_sampled(
        &mut self,
        data: &std::sync::Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
        sampling: ImageSampling,
    ) {
        // A backend without a choice of sampler draws every mode as `Auto`.
        let _ = sampling;
        self.draw_image_rgba_arc(data, img_w, img_h, dst_x, dst_y, dst_w, dst_h);
    }

    #[doc = include_str!("draw_ctx/docs/draw_image_rgba_corners.md")]
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

    #[doc = include_str!("draw_ctx/docs/draw_lcd_backbuffer_arc.md")]
    // Public API: the signature is shared with downstream callers.
    #[allow(clippy::too_many_arguments)]
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
