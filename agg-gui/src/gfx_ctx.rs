//! Graphics context — the primary drawing API for widget painting.
//!
//! `GfxCtx` is modeled after Cairo's `cairo_t`. All drawing goes through this
//! type. It owns a stateful transform + style stack and writes pixels into a
//! [`Framebuffer`] via AGG.
//!
//! # Coordinate system
//!
//! All coordinates are **first-quadrant (Y-up)**. Origin is the bottom-left
//! corner of the framebuffer. Positive X goes right, positive Y goes up.
//! Positive angles rotate counter-clockwise (mathematically standard).

use std::f64::consts::PI;
use std::sync::Arc;

use agg_rust::arc::Arc as AggArc;
use agg_rust::basics::FillingRule;
use agg_rust::basics::VertexSource;
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::comp_op::{CompOp, PixfmtRgba32CompOp};
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_dash::ConvDash;
use agg_rust::conv_stroke::ConvStroke;
use agg_rust::conv_transform::ConvTransform;
use agg_rust::gsv_text::GsvText;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::path_storage::PathStorage;
use agg_rust::rasterizer_scanline_aa::RasterizerScanlineAa;
use agg_rust::renderer_base::RendererBase;
use agg_rust::renderer_scanline::render_scanlines_aa_solid;
use agg_rust::rendering_buffer::RowAccessor;
use agg_rust::rounded_rect::RoundedRect;
use agg_rust::scanline_u::ScanlineU8;
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::draw_ctx::{FillRule, LinearGradientPaint, PatternPaint, RadialGradientPaint};
use crate::framebuffer::Framebuffer;
use crate::text::{measure_advance, shape_text, Font, TextMetrics};

// ---------------------------------------------------------------------------
// Layer stack entry
// ---------------------------------------------------------------------------

/// One entry on the `GfxCtx` layer stack, created by `push_layer`.
struct LayerEntry {
    /// The offscreen framebuffer for this layer.
    fb: Framebuffer,
    /// GfxState snapshot at the moment `push_layer` was called.
    /// Restored verbatim on `pop_layer`.
    saved_state: GfxState,
    /// State-stack snapshot at the moment `push_layer` was called.
    saved_stack: Vec<GfxState>,
    /// Screen-space X origin of this layer (= CTM tx at push time, Y-up).
    origin_x: f64,
    /// Screen-space Y origin of this layer (= CTM ty at push time, Y-up).
    origin_y: f64,
}

// Re-export so callers don't need to import agg_rust directly.
pub use agg_rust::comp_op::CompOp as BlendMode;

/// Snapshot of drawing state, pushed/popped by `save()`/`restore()`.
#[derive(Clone)]
struct GfxState {
    transform: TransAffine,
    fill_color: Color,
    fill_linear_gradient: Option<LinearGradientPaint>,
    fill_radial_gradient: Option<RadialGradientPaint>,
    fill_pattern: Option<PatternPaint>,
    stroke_color: Color,
    stroke_linear_gradient: Option<LinearGradientPaint>,
    stroke_radial_gradient: Option<RadialGradientPaint>,
    stroke_pattern: Option<PatternPaint>,
    fill_rule: FillRule,
    line_width: f64,
    line_join: LineJoin,
    line_cap: LineCap,
    miter_limit: f64,
    line_dash: Vec<f64>,
    dash_offset: f64,
    blend_mode: CompOp,
    /// Scissor clip in Y-up screen space: `(x, y, width, height)`.
    clip: Option<(f64, f64, f64, f64)>,
    /// Global alpha multiplier applied to fill and stroke at draw time.
    global_alpha: f64,
    /// Current font (shared).
    font: Option<Arc<Font>>,
    /// Font size in pixels (height from baseline to top of cap height).
    font_size: f64,
}

impl Default for GfxState {
    fn default() -> Self {
        Self {
            transform: TransAffine::new(),
            fill_color: Color::black(),
            fill_linear_gradient: None,
            fill_radial_gradient: None,
            fill_pattern: None,
            stroke_color: Color::black(),
            stroke_linear_gradient: None,
            stroke_radial_gradient: None,
            stroke_pattern: None,
            fill_rule: FillRule::NonZero,
            line_width: 1.0,
            line_join: LineJoin::Round,
            line_cap: LineCap::Round,
            miter_limit: 4.0,
            line_dash: Vec::new(),
            dash_offset: 0.0,
            blend_mode: CompOp::SrcOver,
            clip: None,
            global_alpha: 1.0,
            font: None,
            font_size: 16.0,
        }
    }
}

/// Cairo-style stateful 2D graphics context.
///
/// All widget painting goes through `GfxCtx`. Create one per frame from a
/// [`Framebuffer`], draw into it, then let it drop — the framebuffer retains
/// the rendered pixels.
///
/// # Layer compositing
///
/// Call `push_layer(w, h)` to redirect all subsequent drawing into an offscreen
/// framebuffer.  Call `pop_layer()` to SrcOver-composite that buffer back into
/// the previous target (which may itself be a layer or the base framebuffer).
/// Layers nest; each `push` must be matched by exactly one `pop`.
pub struct GfxCtx<'a> {
    base_fb: &'a mut Framebuffer,
    /// Offscreen layer stack.  Empty when rendering directly to `base_fb`.
    layer_stack: Vec<LayerEntry>,
    state: GfxState,
    state_stack: Vec<GfxState>,
    /// Accumulated path, reset by `begin_path()`.
    path: PathStorage,
    /// When true, `fill_text` routes through the 3× horizontal LCD
    /// subpixel pipeline (see `lcd_coverage.rs`) and composites per-channel
    /// onto the active framebuffer.  Controlled by the backbuffer mode —
    /// set to true when this ctx is writing into an `LcdCoverage` widget
    /// backbuffer, false for `Rgba`.  Main render loops set it at frame
    /// start from `font_settings::lcd_enabled()`.
    lcd_mode: bool,
}

impl<'a> GfxCtx<'a> {
    /// Create a new graphics context for the given framebuffer.
    pub fn new(fb: &'a mut Framebuffer) -> Self {
        Self {
            base_fb: fb,
            layer_stack: Vec::new(),
            state: GfxState::default(),
            state_stack: Vec::new(),
            path: PathStorage::new(),
            lcd_mode: false,
        }
    }

    // -------------------------------------------------------------------------
    // Layer compositing
    // -------------------------------------------------------------------------

    /// Begin an offscreen compositing layer of `width × height` pixels.
    ///
    /// All draw calls until the matching `pop_layer` are redirected into a fresh
    /// transparent `Framebuffer`.  The current CTM's translation records the
    /// layer's screen-space origin; drawing inside uses a reset local transform.
    pub fn push_layer(&mut self, width: f64, height: f64) {
        let origin_x = self.state.transform.tx;
        let origin_y = self.state.transform.ty;
        let saved_state = self.state.clone();
        let saved_stack = std::mem::take(&mut self.state_stack);
        let layer_fb = Framebuffer::new(width.ceil() as u32, height.ceil() as u32);
        self.layer_stack.push(LayerEntry {
            fb: layer_fb,
            saved_state,
            saved_stack,
            origin_x,
            origin_y,
        });
        // Reset to local-space origin for the new layer.
        self.state.transform = TransAffine::new();
        self.state.clip = None;
    }

    /// SrcOver-composite the current layer into the previous render target, then
    /// restore the graphics state that was active at the matching `push_layer`.
    pub fn pop_layer(&mut self) {
        let Some(layer) = self.layer_stack.pop() else {
            return;
        };
        let ox = layer.origin_x as i32;
        let oy = layer.origin_y as i32;
        self.state = layer.saved_state;
        self.state_stack = layer.saved_stack;
        // Composite: src = layer.fb, dst = now-active framebuffer.
        if let Some(top) = self.layer_stack.last_mut() {
            composite_framebuffers(&mut top.fb, &layer.fb, ox, oy);
        } else {
            composite_framebuffers(self.base_fb, &layer.fb, ox, oy);
        }
    }

    // -------------------------------------------------------------------------
    // State stack
    // -------------------------------------------------------------------------

    pub fn save(&mut self) {
        self.state_stack.push(self.state.clone());
    }

    pub fn restore(&mut self) {
        if let Some(state) = self.state_stack.pop() {
            self.state = state;
        }
    }

    // -------------------------------------------------------------------------
    // Transform (Y-up, CCW-positive rotations)
    // -------------------------------------------------------------------------

    /// Append a translation. Uses pre-multiply (Cairo semantics).
    pub fn translate(&mut self, tx: f64, ty: f64) {
        self.state
            .transform
            .premultiply(&TransAffine::new_translation(tx, ty));
    }

    /// Append a CCW rotation in radians. Uses pre-multiply semantics.
    pub fn rotate(&mut self, radians: f64) {
        self.state
            .transform
            .premultiply(&TransAffine::new_rotation(radians));
    }

    /// Append a scale. Uses pre-multiply semantics.
    pub fn scale(&mut self, sx: f64, sy: f64) {
        self.state
            .transform
            .premultiply(&TransAffine::new_scaling(sx, sy));
    }

    pub fn set_transform(&mut self, m: TransAffine) {
        self.state.transform = m;
    }
    pub fn reset_transform(&mut self) {
        self.state.transform = TransAffine::new();
    }
    /// Return the current accumulated transform (cumulative translation + scale
    /// from all parent `save/translate/restore` calls). The `tx`/`ty` fields
    /// give the widget's bottom-left corner in framebuffer (Y-up) coordinates.
    pub fn transform(&self) -> TransAffine {
        self.state.transform
    }

    // -------------------------------------------------------------------------
    // Style
    // -------------------------------------------------------------------------

    pub fn set_fill_color(&mut self, color: Color) {
        self.state.fill_color = color;
        self.state.fill_linear_gradient = None;
        self.state.fill_radial_gradient = None;
        self.state.fill_pattern = None;
    }
    pub fn set_fill_linear_gradient(&mut self, gradient: LinearGradientPaint) {
        self.state.fill_linear_gradient = Some(gradient);
        self.state.fill_radial_gradient = None;
        self.state.fill_pattern = None;
    }
    pub fn set_fill_radial_gradient(&mut self, gradient: RadialGradientPaint) {
        self.state.fill_linear_gradient = None;
        self.state.fill_radial_gradient = Some(gradient);
        self.state.fill_pattern = None;
    }
    pub fn set_fill_pattern(&mut self, pattern: PatternPaint) {
        self.state.fill_linear_gradient = None;
        self.state.fill_radial_gradient = None;
        self.state.fill_pattern = Some(pattern);
    }
    pub fn set_stroke_color(&mut self, color: Color) {
        self.state.stroke_color = color;
        self.state.stroke_linear_gradient = None;
        self.state.stroke_radial_gradient = None;
        self.state.stroke_pattern = None;
    }
    pub fn set_stroke_linear_gradient(&mut self, gradient: LinearGradientPaint) {
        self.state.stroke_linear_gradient = Some(gradient);
        self.state.stroke_radial_gradient = None;
        self.state.stroke_pattern = None;
    }
    pub fn set_stroke_radial_gradient(&mut self, gradient: RadialGradientPaint) {
        self.state.stroke_linear_gradient = None;
        self.state.stroke_radial_gradient = Some(gradient);
        self.state.stroke_pattern = None;
    }
    pub fn set_stroke_pattern(&mut self, pattern: PatternPaint) {
        self.state.stroke_linear_gradient = None;
        self.state.stroke_radial_gradient = None;
        self.state.stroke_pattern = Some(pattern);
    }
    pub fn set_line_width(&mut self, w: f64) {
        self.state.line_width = w;
    }
    pub fn set_line_join(&mut self, join: LineJoin) {
        self.state.line_join = join;
    }
    pub fn set_line_cap(&mut self, cap: LineCap) {
        self.state.line_cap = cap;
    }
    pub fn set_miter_limit(&mut self, limit: f64) {
        self.state.miter_limit = limit.max(1.0);
    }
    pub fn set_line_dash(&mut self, dashes: &[f64], offset: f64) {
        self.state.line_dash.clear();
        self.state
            .line_dash
            .extend(dashes.iter().copied().filter(|v| *v > 0.0));
        self.state.dash_offset = offset;
    }
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        self.state.fill_rule = rule;
    }

    /// Set the Porter-Duff compositing mode. Default: `SrcOver`.
    pub fn set_blend_mode(&mut self, mode: CompOp) {
        self.state.blend_mode = mode;
    }

    /// Global alpha multiplier (0.0–1.0) applied on top of each color's alpha.
    pub fn set_global_alpha(&mut self, alpha: f64) {
        self.state.global_alpha = alpha.clamp(0.0, 1.0);
    }

    // -------------------------------------------------------------------------
    // Font
    // -------------------------------------------------------------------------

    /// Set the current font. Shared via `Arc` — cheap to clone across widgets.
    pub fn set_font(&mut self, font: Arc<Font>) {
        self.state.font = Some(font);
    }

    /// Set the font size in pixels (distance from baseline to cap height).
    pub fn set_font_size(&mut self, size: f64) {
        self.state.font_size = size.max(1.0);
    }

    /// Enable/disable LCD subpixel rendering on this ctx.  When true,
    /// `fill_text` uses the per-channel coverage pipeline; when false
    /// grayscale AA.  Set by `paint_subtree_backbuffered` for
    /// `LcdCoverage` widget buffers, and by the main render loop for
    /// direct-to-screen text.
    pub fn set_lcd_mode(&mut self, on: bool) {
        self.lcd_mode = on;
    }

    /// Read the ctx's current LCD mode.
    pub fn lcd_mode(&self) -> bool {
        self.lcd_mode
    }

    // -------------------------------------------------------------------------
    // Clipping
    // -------------------------------------------------------------------------

    /// Intersect the current clip with a rectangle in the **current local
    /// coordinate space** (i.e. after all accumulated `translate` / `scale`
    /// calls).  The four corners are mapped through the current transform to
    /// produce an axis-aligned screen-space bounding box, which is then
    /// intersected with any existing clip.
    ///
    /// For the common case of pure translations this is equivalent to the old
    /// "screen-space rectangle" API, but it now works correctly when called
    /// from inside a `paint()` method that runs after the framework has already
    /// translated the context to the widget's origin.
    pub fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        // Map all four corners through the CTM and take the AABB.
        let t = &self.state.transform;
        let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        let mut sx_min = f64::INFINITY;
        let mut sy_min = f64::INFINITY;
        let mut sx_max = f64::NEG_INFINITY;
        let mut sy_max = f64::NEG_INFINITY;
        for (lx, ly) in corners {
            let mut sx = lx;
            let mut sy = ly;
            t.transform(&mut sx, &mut sy);
            if sx < sx_min {
                sx_min = sx;
            }
            if sx > sx_max {
                sx_max = sx;
            }
            if sy < sy_min {
                sy_min = sy;
            }
            if sy > sy_max {
                sy_max = sy;
            }
        }
        let sw = (sx_max - sx_min).max(0.0);
        let sh = (sy_max - sy_min).max(0.0);
        if let Some((cx, cy, cw, ch)) = self.state.clip {
            let x1 = sx_min.max(cx);
            let y1 = sy_min.max(cy);
            let x2 = sx_max.min(cx + cw);
            let y2 = sy_max.min(cy + ch);
            self.state.clip = Some((x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0)));
        } else {
            self.state.clip = Some((sx_min, sy_min, sw, sh));
        }
    }

    pub fn reset_clip(&mut self) {
        self.state.clip = None;
    }

    // -------------------------------------------------------------------------
    // Clear
    // -------------------------------------------------------------------------

    /// Fill the entire active framebuffer with `color`, ignoring transform and clip.
    pub fn clear(&mut self, color: Color) {
        let rgba = color.to_rgba8();
        for chunk in active_fb(&mut self.base_fb, &mut self.layer_stack)
            .pixels_mut()
            .chunks_exact_mut(4)
        {
            chunk[0] = rgba.r as u8;
            chunk[1] = rgba.g as u8;
            chunk[2] = rgba.b as u8;
            chunk[3] = rgba.a as u8;
        }
    }

    // -------------------------------------------------------------------------
    // Path construction
    // -------------------------------------------------------------------------

    pub fn begin_path(&mut self) {
        self.path = PathStorage::new();
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.path.move_to(x, y);
    }
    pub fn line_to(&mut self, x: f64, y: f64) {
        self.path.line_to(x, y);
    }

    pub fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        self.path.curve4(cx1, cy1, cx2, cy2, x, y);
    }

    pub fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.path.curve3(cx, cy, x, y);
    }

    pub fn arc_to(
        &mut self,
        cx: f64,
        cy: f64,
        r: f64,
        start_angle: f64,
        end_angle: f64,
        ccw: bool,
    ) {
        let mut arc = AggArc::new(cx, cy, r, r, start_angle, end_angle, ccw);
        self.path.concat_path(&mut arc, 0);
    }

    /// Full circle at `(cx, cy)` with radius `r`.
    pub fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        self.arc_to(cx, cy, r, 0.0, 2.0 * PI, true);
        self.path.close_polygon(PATH_FLAGS_NONE);
    }

    /// Axis-aligned rectangle — bottom-left `(x, y)`, size `w × h`.
    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.path.move_to(x, y);
        self.path.line_to(x + w, y);
        self.path.line_to(x + w, y + h);
        self.path.line_to(x, y + h);
        self.path.close_polygon(PATH_FLAGS_NONE);
    }

    /// Rounded rectangle — bottom-left `(x, y)`, size `w × h`, corner radius `r`.
    pub fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
        let mut rr = RoundedRect::new(x, y, x + w, y + h, r);
        rr.normalize_radius();
        self.path.concat_path(&mut rr, 0);
    }

    pub fn close_path(&mut self) {
        self.path.close_polygon(PATH_FLAGS_NONE);
    }

    // -------------------------------------------------------------------------
    // Drawing
    // -------------------------------------------------------------------------

    /// Fill the accumulated path.
    pub fn fill(&mut self) {
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let fill_rule = self.state.fill_rule;
        let transform = self.state.transform.clone();
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        if let Some(gradient) = self.state.fill_linear_gradient.clone() {
            draw_impl::rasterize_linear_gradient_fill(
                fb,
                &mut self.path,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                fill_rule,
                &transform,
            );
        } else if let Some(gradient) = self.state.fill_radial_gradient.clone() {
            draw_impl::rasterize_radial_gradient_fill(
                fb,
                &mut self.path,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                fill_rule,
                &transform,
            );
        } else if let Some(pattern) = self.state.fill_pattern.clone() {
            draw_impl::rasterize_pattern_fill(
                fb,
                &mut self.path,
                &pattern,
                self.state.global_alpha as f32,
                mode,
                clip,
                fill_rule,
                &transform,
            );
        } else {
            let mut color = self.state.fill_color;
            color.a *= self.state.global_alpha as f32;
            let rgba = color.to_rgba8();
            rasterize_fill(fb, &mut self.path, &rgba, mode, clip, fill_rule, &transform);
        }
    }

    /// Stroke the accumulated path.
    pub fn stroke(&mut self) {
        let mut color = self.state.stroke_color;
        color.a *= self.state.global_alpha as f32;
        let rgba = color.to_rgba8();
        let width = self.state.line_width;
        let join = self.state.line_join;
        let cap = self.state.line_cap;
        let miter_limit = self.state.miter_limit;
        let dashes = self.state.line_dash.clone();
        let dash_offset = self.state.dash_offset;
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let transform = self.state.transform.clone();
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        if let Some(gradient) = self.state.stroke_linear_gradient.clone() {
            let mut outline = stroke::materialize_stroke_outline(
                &mut self.path,
                width,
                join,
                cap,
                miter_limit,
                &dashes,
                dash_offset,
            );
            draw_impl::rasterize_linear_gradient_fill(
                fb,
                &mut outline,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                FillRule::NonZero,
                &transform,
            );
        } else if let Some(gradient) = self.state.stroke_radial_gradient.clone() {
            let mut outline = stroke::materialize_stroke_outline(
                &mut self.path,
                width,
                join,
                cap,
                miter_limit,
                &dashes,
                dash_offset,
            );
            draw_impl::rasterize_radial_gradient_fill(
                fb,
                &mut outline,
                &gradient,
                self.state.global_alpha as f32,
                mode,
                clip,
                FillRule::NonZero,
                &transform,
            );
        } else if let Some(pattern) = self.state.stroke_pattern.clone() {
            let mut outline = stroke::materialize_stroke_outline(
                &mut self.path,
                width,
                join,
                cap,
                miter_limit,
                &dashes,
                dash_offset,
            );
            draw_impl::rasterize_pattern_fill(
                fb,
                &mut outline,
                &pattern,
                self.state.global_alpha as f32,
                mode,
                clip,
                FillRule::NonZero,
                &transform,
            );
        } else {
            rasterize_stroke(
                fb,
                &mut self.path,
                &rgba,
                width,
                join,
                cap,
                miter_limit,
                &dashes,
                dash_offset,
                mode,
                clip,
                &transform,
            );
        }
    }

    /// Fill then stroke the accumulated path in one call.
    pub fn fill_and_stroke(&mut self) {
        self.fill();
        self.stroke();
    }

    // -------------------------------------------------------------------------
    // Text
    // -------------------------------------------------------------------------

    /// Draw `text` at position `(x, y)` using the current font and fill color.
    ///
    /// `(x, y)` is the **baseline-left** position in Y-up screen coordinates.
    /// Glyphs extend upward (higher Y) for ascenders and downward (lower Y)
    /// for descenders — correct for Y-up rendering with no Y-flip.
    ///
    /// Requires a font to be set via [`set_font`](Self::set_font). Does nothing
    /// if no font has been set.
    pub fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        let font = match self.state.font.clone() {
            Some(f) => f,
            None => return,
        };
        let font_size = self.state.font_size;

        let mut color = self.state.fill_color;
        color.a *= self.state.global_alpha as f32;

        // LCD subpixel path — gated on this ctx's `lcd_mode` flag,
        // which is set by `paint_subtree_backbuffered` when the widget
        // chose `BackbufferMode::LcdCoverage` and by the main render
        // loop for direct-to-screen text when the global font setting
        // says so.  Mask raster is cached keyed on `(text, font, size)`
        // and colour is applied at composite time.
        //
        // HiDPI: rasterise the mask at the **physical** font size (logical
        // × CTM scale) so the 1:1 texel-to-pixel composite fills the
        // expected number of physical pixels.  Without this the mask
        // renders at logical size and ends up half-size (or stretched by a
        // separate scale call) on 2×/3× displays.
        //
        // **Y-axis baseline alignment**: when the global hinting toggle
        // is ON, both renderers place the baseline on the same integer
        // physical pixel row — see the in-mask `by` snap inside
        // `rasterize_text_lcd_cached` paired with `shape_text`'s own
        // hint-driven `gy` snap.  When hinting is OFF, the RGBA path
        // produces baseline at the exact fractional `y`, while the LCD
        // path's intrinsic composite-rounding (`sy.round()` in
        // `draw_lcd_mask`, required for X-subpixel coherence) lands the
        // baseline at the nearest integer plus the fractional descender
        // — a subpixel residual that's impossible to remove without
        // breaking LCD chroma.  This is a deliberate trade-off matching
        // the user's "snap should be a checkbox, not always on".
        if self.lcd_mode {
            let t = &self.state.transform;
            let ctm_scale = (t.sx * t.sx + t.shy * t.shy).sqrt().max(1e-6);
            let phys_size = font_size * ctm_scale;
            let cached = crate::lcd_coverage::rasterize_text_lcd_cached(&font, text, phys_size);
            // `baseline_*_in_mask` is in physical mask pixels; divide by
            // `ctm_scale` so the offset stays in logical units that the
            // CTM then multiplies back to physical at blit time.
            let dst_x = x - cached.baseline_x_in_mask / ctm_scale;
            let dst_y = y - cached.baseline_y_in_mask / ctm_scale;
            <Self as crate::DrawCtx>::draw_lcd_mask_arc(
                self,
                &cached.pixels,
                cached.width,
                cached.height,
                color,
                dst_x,
                dst_y,
            );
            return;
        }

        let rgba = color.to_rgba8();
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let transform = self.state.transform.clone();

        // Shape text and collect per-glyph outline paths.
        let (glyph_paths, _) = shape_text(&font, text, font_size, x, y);
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        for mut path in glyph_paths {
            rasterize_fill(
                fb,
                &mut path,
                &rgba,
                mode,
                clip,
                FillRule::NonZero,
                &transform,
            );
        }
    }

    /// Measure the advance width and metrics of `text` in the current font.
    ///
    /// Returns `None` if no font has been set.
    pub fn measure_text(&self, text: &str) -> Option<TextMetrics> {
        let font = self.state.font.as_ref()?;
        let size = self.state.font_size;
        Some(TextMetrics {
            width: measure_advance(font, text, size),
            ascent: font.ascender_px(size),
            descent: font.descender_px(size),
            line_height: font.line_height_px(size),
        })
    }

    // -------------------------------------------------------------------------
    // Convenience: built-in stroked vector font (no font file required)
    // -------------------------------------------------------------------------

    /// Draw text using AGG's built-in vector font (no external font needed).
    ///
    /// Useful for labels before a full font is loaded.
    pub fn fill_text_gsv(&mut self, text: &str, x: f64, y: f64, size: f64) {
        let mut color = self.state.fill_color;
        color.a *= self.state.global_alpha as f32;
        let rgba = color.to_rgba8();
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let transform = self.state.transform.clone();

        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        let w = fb.width();
        let h = fb.height();
        let stride = (w * 4) as i32;
        let mut ra = RowAccessor::new();
        unsafe { ra.attach(fb.pixels_mut().as_mut_ptr(), w, h, stride) };
        let pf = PixfmtRgba32CompOp::new_with_op(&mut ra, mode);
        let mut rb = RendererBase::new(pf);
        apply_clip(&mut rb, clip);

        let mut ras = RasterizerScanlineAa::new();
        let mut sl = ScanlineU8::new();

        let mut gsv = GsvText::new();
        gsv.size(size, 0.0);
        gsv.start_point(x, y);
        gsv.text(text);

        let mut stroke = ConvStroke::new(&mut gsv);
        stroke.set_width(size * 0.1);
        let mut transformed = ConvTransform::new(&mut stroke, transform);
        ras.add_path(&mut transformed, 0);
        render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, &rgba);
    }
}

mod draw_impl;
mod stroke;

use draw_impl::{active_fb, composite_framebuffers};
pub(crate) use draw_impl::{apply_clip, rasterize_fill, rasterize_stroke};
