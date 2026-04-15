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
    fn gl_paint(
        &mut self,
        gl:          &dyn std::any::Any,
        screen_rect: Rect,
        full_w:      i32,
        full_h:      i32,
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
    fn set_line_width(&mut self, w: f64);
    fn set_line_join(&mut self, join: LineJoin);
    fn set_line_cap(&mut self, cap: LineCap);
    fn set_blend_mode(&mut self, mode: CompOp);
    fn set_global_alpha(&mut self, alpha: f64);

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

    // ── Image blitting ────────────────────────────────────────────────────────

    /// Returns `true` if this context implements `draw_image_rgba` with actual
    /// pixel blitting.  `Label` (and any other widget that uses a software
    /// backbuffer) gates its cache path on this method so it can fall back to
    /// direct `fill_text()` on render targets that don't support blitting
    /// (e.g. the GL path).
    ///
    /// Default: `false`.  Override to `true` in `GfxCtx`.
    fn has_image_blit(&self) -> bool { false }

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
        data:  &[u8],
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        let _ = (data, img_w, img_h, dst_x, dst_y, dst_w, dst_h);
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
