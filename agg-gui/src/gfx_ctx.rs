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
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::comp_op::{CompOp, PixfmtRgba32CompOp};
use agg_rust::conv_curve::ConvCurve;
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
use crate::framebuffer::Framebuffer;
use crate::text::{shape_text, measure_advance, Font, TextMetrics};

// ---------------------------------------------------------------------------
// Layer stack entry
// ---------------------------------------------------------------------------

/// One entry on the `GfxCtx` layer stack, created by `push_layer`.
struct LayerEntry {
    /// The offscreen framebuffer for this layer.
    fb:             Framebuffer,
    /// GfxState snapshot at the moment `push_layer` was called.
    /// Restored verbatim on `pop_layer`.
    saved_state:    GfxState,
    /// State-stack snapshot at the moment `push_layer` was called.
    saved_stack:    Vec<GfxState>,
    /// Screen-space X origin of this layer (= CTM tx at push time, Y-up).
    origin_x:       f64,
    /// Screen-space Y origin of this layer (= CTM ty at push time, Y-up).
    origin_y:       f64,
}

// Re-export so callers don't need to import agg_rust directly.
pub use agg_rust::comp_op::CompOp as BlendMode;

/// Snapshot of drawing state, pushed/popped by `save()`/`restore()`.
#[derive(Clone)]
struct GfxState {
    transform: TransAffine,
    fill_color: Color,
    stroke_color: Color,
    line_width: f64,
    line_join: LineJoin,
    line_cap: LineCap,
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
            stroke_color: Color::black(),
            line_width: 1.0,
            line_join: LineJoin::Round,
            line_cap: LineCap::Round,
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
    base_fb:     &'a mut Framebuffer,
    /// Offscreen layer stack.  Empty when rendering directly to `base_fb`.
    layer_stack: Vec<LayerEntry>,
    state:       GfxState,
    state_stack: Vec<GfxState>,
    /// Accumulated path, reset by `begin_path()`.
    path:        PathStorage,
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
        let Some(layer) = self.layer_stack.pop() else { return; };
        let ox = layer.origin_x as i32;
        let oy = layer.origin_y as i32;
        self.state       = layer.saved_state;
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
        self.state.transform.premultiply(&TransAffine::new_translation(tx, ty));
    }

    /// Append a CCW rotation in radians. Uses pre-multiply semantics.
    pub fn rotate(&mut self, radians: f64) {
        self.state.transform.premultiply(&TransAffine::new_rotation(radians));
    }

    /// Append a scale. Uses pre-multiply semantics.
    pub fn scale(&mut self, sx: f64, sy: f64) {
        self.state.transform.premultiply(&TransAffine::new_scaling(sx, sy));
    }

    pub fn set_transform(&mut self, m: TransAffine) { self.state.transform = m; }
    pub fn reset_transform(&mut self) { self.state.transform = TransAffine::new(); }
    /// Return the current accumulated transform (cumulative translation + scale
    /// from all parent `save/translate/restore` calls). The `tx`/`ty` fields
    /// give the widget's bottom-left corner in framebuffer (Y-up) coordinates.
    pub fn transform(&self) -> TransAffine { self.state.transform }

    // -------------------------------------------------------------------------
    // Style
    // -------------------------------------------------------------------------

    pub fn set_fill_color(&mut self, color: Color) { self.state.fill_color = color; }
    pub fn set_stroke_color(&mut self, color: Color) { self.state.stroke_color = color; }
    pub fn set_line_width(&mut self, w: f64) { self.state.line_width = w; }
    pub fn set_line_join(&mut self, join: LineJoin) { self.state.line_join = join; }
    pub fn set_line_cap(&mut self, cap: LineCap) { self.state.line_cap = cap; }

    /// Set the Porter-Duff compositing mode. Default: `SrcOver`.
    pub fn set_blend_mode(&mut self, mode: CompOp) { self.state.blend_mode = mode; }

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
            if sx < sx_min { sx_min = sx; }
            if sx > sx_max { sx_max = sx; }
            if sy < sy_min { sy_min = sy; }
            if sy > sy_max { sy_max = sy; }
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

    pub fn reset_clip(&mut self) { self.state.clip = None; }

    // -------------------------------------------------------------------------
    // Clear
    // -------------------------------------------------------------------------

    /// Fill the entire active framebuffer with `color`, ignoring transform and clip.
    pub fn clear(&mut self, color: Color) {
        let rgba = color.to_rgba8();
        for chunk in active_fb(&mut self.base_fb, &mut self.layer_stack).pixels_mut().chunks_exact_mut(4) {
            chunk[0] = rgba.r as u8;
            chunk[1] = rgba.g as u8;
            chunk[2] = rgba.b as u8;
            chunk[3] = rgba.a as u8;
        }
    }

    // -------------------------------------------------------------------------
    // Path construction
    // -------------------------------------------------------------------------

    pub fn begin_path(&mut self) { self.path = PathStorage::new(); }

    pub fn move_to(&mut self, x: f64, y: f64) { self.path.move_to(x, y); }
    pub fn line_to(&mut self, x: f64, y: f64) { self.path.line_to(x, y); }

    pub fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        self.path.curve4(cx1, cy1, cx2, cy2, x, y);
    }

    pub fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.path.curve3(cx, cy, x, y);
    }

    pub fn arc_to(&mut self, cx: f64, cy: f64, r: f64, start_angle: f64, end_angle: f64, ccw: bool) {
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

    pub fn close_path(&mut self) { self.path.close_polygon(PATH_FLAGS_NONE); }

    // -------------------------------------------------------------------------
    // Drawing
    // -------------------------------------------------------------------------

    /// Fill the accumulated path.
    pub fn fill(&mut self) {
        let mut color = self.state.fill_color;
        color.a *= self.state.global_alpha as f32;
        let rgba = color.to_rgba8();
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let transform = self.state.transform.clone();
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        rasterize_fill(fb, &mut self.path, &rgba, mode, clip, &transform);
    }

    /// Stroke the accumulated path.
    pub fn stroke(&mut self) {
        let mut color = self.state.stroke_color;
        color.a *= self.state.global_alpha as f32;
        let rgba = color.to_rgba8();
        let width = self.state.line_width;
        let join = self.state.line_join;
        let cap = self.state.line_cap;
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let transform = self.state.transform.clone();
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        rasterize_stroke(fb, &mut self.path, &rgba, width, join, cap, mode, clip, &transform);
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
        let rgba = color.to_rgba8();
        let mode = self.state.blend_mode;
        let clip = self.state.clip;
        let transform = self.state.transform.clone();

        // Shape text and collect per-glyph outline paths.
        let (glyph_paths, _) = shape_text(&font, text, font_size, x, y);
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        for mut path in glyph_paths {
            rasterize_fill(fb, &mut path, &rgba, mode, clip, &transform);
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

// ---------------------------------------------------------------------------
// Active-framebuffer helper
// ---------------------------------------------------------------------------

/// Return a `&mut Framebuffer` for the currently active render target.
///
/// If any layers are on the stack, returns the top layer's framebuffer.
/// Otherwise returns the base framebuffer.  Accepts the two fields as
/// separate `&mut` references so callers can simultaneously borrow other
/// `GfxCtx` fields (e.g. `state`, `path`) without triggering borrow
/// conflicts on `self`.
#[inline]
fn active_fb<'a>(
    base_fb:     &'a mut Framebuffer,
    layer_stack: &'a mut Vec<LayerEntry>,
) -> &'a mut Framebuffer {
    if let Some(top) = layer_stack.last_mut() {
        &mut top.fb
    } else {
        base_fb
    }
}

// ---------------------------------------------------------------------------
// SrcOver layer compositing
// ---------------------------------------------------------------------------

/// Composite `src` onto `dst` using SrcOver alpha blending.
///
/// AGG writes **premultiplied** RGBA into framebuffers.  The premultiplied
/// SrcOver formula is:
///
/// ```text
/// out_channel = src_premul + dst_premul × (1 − src_alpha_norm)
/// ```
///
/// This applies identically to all four channels (R, G, B, A), which makes
/// the implementation straightforward and avoids the division step needed for
/// straight-alpha compositing.
///
/// `dest_x` / `dest_y` are the Y-up pixel coordinates in `dst` where the
/// bottom-left corner of `src` lands.  Out-of-bounds pixels are silently clipped.
fn composite_framebuffers(dst: &mut Framebuffer, src: &Framebuffer, dest_x: i32, dest_y: i32) {
    let src_w = src.width() as i32;
    let src_h = src.height() as i32;
    let dst_w = dst.width() as i32;
    let dst_h = dst.height() as i32;

    let src_px = src.pixels();
    let dst_px = dst.pixels_mut();

    for sy in 0..src_h {
        let dy = dest_y + sy;
        if dy < 0 || dy >= dst_h { continue; }
        for sx in 0..src_w {
            let dx = dest_x + sx;
            if dx < 0 || dx >= dst_w { continue; }
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((dy * dst_w + dx) * 4) as usize;
            let sa = src_px[si + 3] as f32 / 255.0;
            if sa < 1e-4 { continue; } // fully transparent source — skip
            let inv_sa = 1.0 - sa;
            // Premultiplied SrcOver — same formula for all four channels.
            for k in 0..4 {
                let s = src_px[si + k] as f32;
                let d = dst_px[di + k] as f32;
                dst_px[di + k] = (s + d * inv_sa).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Free rasterization helpers — take explicit path and fb references so they
// can be called for both self.path draws and per-glyph text draws without
// borrow-checker conflicts.
// ---------------------------------------------------------------------------

pub(crate) fn rasterize_fill(
    fb: &mut Framebuffer,
    path: &mut PathStorage,
    color: &agg_rust::color::Rgba8,
    mode: CompOp,
    clip: Option<(f64, f64, f64, f64)>,
    transform: &TransAffine,
) {
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
    let mut curves = ConvCurve::new(path);
    let mut transformed = ConvTransform::new(&mut curves, transform.clone());
    ras.add_path(&mut transformed, 0);
    render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, color);
}

pub(crate) fn rasterize_stroke(
    fb: &mut Framebuffer,
    path: &mut PathStorage,
    color: &agg_rust::color::Rgba8,
    width: f64,
    join: LineJoin,
    cap: LineCap,
    mode: CompOp,
    clip: Option<(f64, f64, f64, f64)>,
    transform: &TransAffine,
) {
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
    let mut curves = ConvCurve::new(path);
    let mut stroke = ConvStroke::new(&mut curves);
    stroke.set_width(width);
    stroke.set_line_join(join);
    stroke.set_line_cap(cap);
    let mut transformed = ConvTransform::new(&mut stroke, transform.clone());
    ras.add_path(&mut transformed, 0);
    render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, color);
}

// ---------------------------------------------------------------------------
// DrawCtx blanket impl for GfxCtx
// ---------------------------------------------------------------------------

impl crate::draw_ctx::DrawCtx for GfxCtx<'_> {
    fn set_fill_color(&mut self, c: crate::color::Color)     { self.set_fill_color(c) }
    fn set_stroke_color(&mut self, c: crate::color::Color)   { self.set_stroke_color(c) }
    fn set_line_width(&mut self, w: f64)                      { self.set_line_width(w) }
    fn set_line_join(&mut self, j: agg_rust::math_stroke::LineJoin) { self.set_line_join(j) }
    fn set_line_cap(&mut self, c: agg_rust::math_stroke::LineCap)   { self.set_line_cap(c) }
    fn set_blend_mode(&mut self, m: agg_rust::comp_op::CompOp)      { self.set_blend_mode(m) }
    fn set_global_alpha(&mut self, a: f64)                   { self.set_global_alpha(a) }
    fn set_font(&mut self, f: Arc<crate::text::Font>)        { self.set_font(f) }
    fn set_font_size(&mut self, s: f64)                      { self.set_font_size(s) }
    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) { self.clip_rect(x, y, w, h) }
    fn reset_clip(&mut self)                                  { self.reset_clip() }
    fn clear(&mut self, c: crate::color::Color)              { self.clear(c) }
    fn begin_path(&mut self)                                  { self.begin_path() }
    fn move_to(&mut self, x: f64, y: f64)                    { self.move_to(x, y) }
    fn line_to(&mut self, x: f64, y: f64)                    { self.line_to(x, y) }
    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        self.cubic_to(cx1, cy1, cx2, cy2, x, y)
    }
    fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) { self.quad_to(cx, cy, x, y) }
    fn arc_to(&mut self, cx: f64, cy: f64, r: f64, a1: f64, a2: f64, ccw: bool) {
        self.arc_to(cx, cy, r, a1, a2, ccw)
    }
    fn circle(&mut self, cx: f64, cy: f64, r: f64)          { self.circle(cx, cy, r) }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64)      { self.rect(x, y, w, h) }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.rounded_rect(x, y, w, h, r)
    }
    fn close_path(&mut self)                                  { self.close_path() }
    fn fill(&mut self)                                        { self.fill() }
    fn stroke(&mut self)                                      { self.stroke() }
    fn fill_and_stroke(&mut self)                             { self.fill_and_stroke() }
    fn fill_text(&mut self, t: &str, x: f64, y: f64)        { self.fill_text(t, x, y) }
    fn fill_text_gsv(&mut self, t: &str, x: f64, y: f64, s: f64) { self.fill_text_gsv(t, x, y, s) }
    fn measure_text(&self, t: &str) -> Option<crate::text::TextMetrics> { self.measure_text(t) }
    fn transform(&self) -> agg_rust::trans_affine::TransAffine { self.transform() }
    fn save(&mut self)                                        { self.save() }
    fn restore(&mut self)                                     { self.restore() }
    fn translate(&mut self, tx: f64, ty: f64)                { self.translate(tx, ty) }
    fn rotate(&mut self, r: f64)                             { self.rotate(r) }
    fn scale(&mut self, sx: f64, sy: f64)                    { self.scale(sx, sy) }
    fn set_transform(&mut self, m: agg_rust::trans_affine::TransAffine) { self.set_transform(m) }
    fn reset_transform(&mut self)                             { self.reset_transform() }
    fn push_layer(&mut self, w: f64, h: f64)                 { self.push_layer(w, h) }
    fn pop_layer(&mut self)                                   { self.pop_layer() }

    fn has_image_blit(&self) -> bool { true }

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
        // Scale the source image into a temporary Framebuffer at dst size,
        // then composite it onto the current render target using the CTM origin.
        if img_w == 0 || img_h == 0 || dst_w < 1.0 || dst_h < 1.0 { return; }

        let out_w = dst_w.round() as u32;
        let out_h = dst_h.round() as u32;
        let mut scaled = crate::framebuffer::Framebuffer::new(out_w, out_h);

        // Nearest-neighbour scale — sufficient for README screenshots / badges.
        let px = scaled.pixels_mut();
        for dy in 0..out_h {
            for dx in 0..out_w {
                let sx = (dx as f64 / out_w as f64 * img_w as f64) as u32;
                // Image is top-row-first; Y-up dst means we flip sy.
                let sy_img = ((1.0 - (dy as f64 + 0.5) / out_h as f64) * img_h as f64)
                    .floor()
                    .clamp(0.0, (img_h - 1) as f64) as u32;
                let si = ((sy_img * img_w + sx) * 4) as usize;
                let di = ((dy * out_w + dx) * 4) as usize;
                if si + 3 < data.len() && di + 3 < px.len() {
                    px[di]     = data[si];
                    px[di + 1] = data[si + 1];
                    px[di + 2] = data[si + 2];
                    px[di + 3] = data[si + 3];
                }
            }
        }

        // Apply CTM translation to get screen-space origin.
        let (tx, ty) = { let t = self.transform(); (t.tx, t.ty) };
        let screen_x = (tx + dst_x).round() as i32;
        let screen_y = (ty + dst_y).round() as i32;
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        composite_framebuffers(fb, &scaled, screen_x, screen_y);
    }
}

/// Apply a Y-up scissor clip to a `RendererBase` (pixel-inclusive coordinates).
pub(crate) fn apply_clip<PF: agg_rust::pixfmt_rgba::PixelFormat>(
    rb: &mut RendererBase<PF>,
    clip: Option<(f64, f64, f64, f64)>,
) {
    if let Some((x, y, w, h)) = clip {
        let x1 = x.floor() as i32;
        let y1 = y.floor() as i32;
        let x2 = (x + w).ceil() as i32 - 1;
        let y2 = (y + h).ceil() as i32 - 1;
        rb.clip_box_i(x1, y1, x2, y2);
    }
}
