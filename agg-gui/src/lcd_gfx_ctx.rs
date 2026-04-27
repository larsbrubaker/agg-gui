//! `LcdGfxCtx` — a [`DrawCtx`] implementation whose render target is an
//! [`LcdBuffer`] (3 bytes/pixel coverage store) instead of a regular
//! RGBA [`Framebuffer`].  All paint primitives flow through the LCD
//! pipeline (3× horizontal supersample → 5-tap filter → per-channel
//! src-over) so the buffer accumulates the same per-channel coverage
//! representation regardless of whether each call originated from a
//! text raster, a path fill, or a future image blit.
//!
//! # Where this fits in the architecture
//!
//! When a widget opts into [`crate::widget::BackbufferMode::LcdCoverage`],
//! `paint_subtree_backbuffered` allocates an `LcdBuffer` and hands its
//! children an `LcdGfxCtx` (rather than a `GfxCtx` over an RGBA
//! `Framebuffer`).  Children paint normally — the same `DrawCtx`
//! methods, the same widget code — but every primitive flows through
//! the LCD pipeline.  When all children have painted, the host either:
//!
//!   - composites the buffer onto the destination RGBA framebuffer via
//!     [`crate::lcd_coverage::composite_lcd_mask`] (software path), or
//!   - uploads the buffer as an RGB texture and runs the dual-source-blend
//!     shader (GL path).
//!
//! Either way, the buffer is the single intermediate that decouples
//! "what was painted" from "how it lands on the destination."
//!
//! # Status
//!
//! Step 2 of the LCD-architecture migration.  The MVP implements the
//! primitives needed to drive an end-to-end equivalence test against
//! the legacy `GfxCtx + lcd_mode=true` path: state setters, transform
//! stack, axis-aligned `rect`/`fill`, `fill_text`, `clear`, and
//! `draw_lcd_mask`.  Curve / stroke / image-blit / clip primitives are
//! marked `// TODO step 2c` and will land before any widget actually
//! paints into an `LcdGfxCtx`.

use std::f64::consts::PI;
use std::sync::Arc;

use agg_rust::arc::Arc as AggArc;
use agg_rust::basics::{VertexSource, PATH_FLAGS_NONE};
use agg_rust::comp_op::CompOp;
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_dash::ConvDash;
use agg_rust::conv_stroke::ConvStroke;
use agg_rust::gsv_text::GsvText;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::path_storage::PathStorage;
use agg_rust::rounded_rect::RoundedRect;
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::draw_ctx::{DrawCtx, FillRule, LinearGradientPaint, RadialGradientPaint};
use crate::lcd_coverage::{rasterize_text_lcd_cached, LcdBuffer, LcdMask};
use crate::text::{measure_text_metrics, Font, TextMetrics};

mod gradient;
mod image;
mod stroke;

// ── State ──────────────────────────────────────────────────────────────────
//
// Mirror of `GfxCtx`'s private `GfxState` so widgets that already set up
// fill colour / font / transform on a `GfxCtx` see the same field shape
// here — and so Step 2c can copy over any logic from the existing fill
// paths without translation.

#[derive(Clone)]
struct LcdState {
    transform: TransAffine,
    fill_color: Color,
    fill_linear_gradient: Option<LinearGradientPaint>,
    fill_radial_gradient: Option<RadialGradientPaint>,
    stroke_color: Color,
    stroke_linear_gradient: Option<LinearGradientPaint>,
    stroke_radial_gradient: Option<RadialGradientPaint>,
    fill_rule: FillRule,
    line_width: f64,
    line_join: LineJoin,
    line_cap: LineCap,
    miter_limit: f64,
    line_dash: Vec<f64>,
    dash_offset: f64,
    blend_mode: CompOp,
    global_alpha: f64,
    font: Option<Arc<Font>>,
    font_size: f64,
    /// Scissor clip in Y-up screen space `(x, y, w, h)`.  Stored but not
    /// yet enforced — `LcdMaskBuilder` doesn't accept a clip param yet.
    /// Step 2c.
    clip: Option<(f64, f64, f64, f64)>,
}

impl Default for LcdState {
    fn default() -> Self {
        Self {
            transform: TransAffine::new(),
            fill_color: Color::black(),
            fill_linear_gradient: None,
            fill_radial_gradient: None,
            stroke_color: Color::black(),
            stroke_linear_gradient: None,
            stroke_radial_gradient: None,
            fill_rule: FillRule::NonZero,
            line_width: 1.0,
            line_join: LineJoin::Round,
            line_cap: LineCap::Round,
            miter_limit: 4.0,
            line_dash: Vec::new(),
            dash_offset: 0.0,
            blend_mode: CompOp::SrcOver,
            global_alpha: 1.0,
            font: None,
            font_size: 16.0,
            clip: None,
        }
    }
}

// ── LcdLayer ───────────────────────────────────────────────────────────────
//
// One entry on the `LcdGfxCtx` layer stack, created by `push_layer`.
// Owns its own `LcdBuffer`; `pop_layer` flushes it back into the
// previously-active buffer at the recorded origin.
//
// Mirrors the role of `gfx_ctx::LayerEntry` — the field shape is kept
// close so widget code that uses `push_layer` / `pop_layer` has the
// same mental model on either ctx type.  The compositing semantics
// differ (see `LcdBuffer::composite_buffer`): RGBA layers do
// alpha-aware SrcOver, LCD layers do full-replace, because the
// coverage buffer has no alpha to distinguish "untouched" from
// "intentionally black".

struct LcdLayer {
    buffer: LcdBuffer,
    /// State snapshot at the moment `push_layer` was called.  Restored
    /// verbatim on `pop_layer` so transform / clip / colour all return
    /// to their pre-layer values.
    saved_state: LcdState,
    saved_stack: Vec<LcdState>,
    /// Where the layer's bottom-left lands in the parent buffer's
    /// coords.  Captured from the CTM's translation at push time.
    origin_x: f64,
    origin_y: f64,
}

// ── LcdGfxCtx ──────────────────────────────────────────────────────────────

/// Cairo-style stateful 2D graphics context whose render target is an
/// [`LcdBuffer`].  Borrows the buffer mutably for the lifetime of the
/// ctx; let the ctx drop and the buffer is free to be uploaded /
/// composited / read.
pub struct LcdGfxCtx<'a> {
    base_buffer: &'a mut LcdBuffer,
    /// Offscreen layer stack.  Empty when rendering directly to
    /// `base_buffer`.  Each `push_layer` pushes a new owned
    /// `LcdBuffer`; subsequent paint primitives target the topmost
    /// layer until the matching `pop_layer` flushes it back.
    layer_stack: Vec<LcdLayer>,
    state: LcdState,
    state_stack: Vec<LcdState>,
    /// Accumulated path, reset by `begin_path`.  Same role as in
    /// `GfxCtx` — the `fill` / `stroke` calls consume it.
    path: PathStorage,
}

impl<'a> LcdGfxCtx<'a> {
    pub fn new(buffer: &'a mut LcdBuffer) -> Self {
        Self {
            base_buffer: buffer,
            layer_stack: Vec::new(),
            state: LcdState::default(),
            state_stack: Vec::new(),
            path: PathStorage::new(),
        }
    }

    /// Read-only view of the underlying buffer — for callers that need
    /// to inspect output without releasing the ctx.  Returns the base
    /// buffer; callers inspecting mid-paint while a layer is active
    /// see only state committed before the current layer's push.
    pub fn buffer(&self) -> &LcdBuffer {
        self.base_buffer
    }

    /// Active paint target: the topmost layer's buffer if any, else
    /// the base buffer.  Every paint primitive routes through this so
    /// `push_layer`/`pop_layer` redirects automatically.
    fn active_buffer(&mut self) -> &mut LcdBuffer {
        if let Some(layer) = self.layer_stack.last_mut() {
            &mut layer.buffer
        } else {
            &mut *self.base_buffer
        }
    }
}

// ── DrawCtx impl ───────────────────────────────────────────────────────────

impl<'a> DrawCtx for LcdGfxCtx<'a> {
    // ── State ─────────────────────────────────────────────────────────────
    fn set_fill_color(&mut self, color: Color) {
        self.state.fill_color = color;
        self.state.fill_linear_gradient = None;
        self.state.fill_radial_gradient = None;
    }
    fn set_fill_linear_gradient(&mut self, gradient: LinearGradientPaint) {
        self.state.fill_linear_gradient = Some(gradient);
        self.state.fill_radial_gradient = None;
    }
    fn supports_fill_linear_gradient(&self) -> bool {
        true
    }
    fn set_fill_radial_gradient(&mut self, gradient: RadialGradientPaint) {
        self.state.fill_linear_gradient = None;
        self.state.fill_radial_gradient = Some(gradient);
    }
    fn supports_fill_radial_gradient(&self) -> bool {
        true
    }
    fn set_stroke_color(&mut self, color: Color) {
        self.state.stroke_color = color;
        self.state.stroke_linear_gradient = None;
        self.state.stroke_radial_gradient = None;
    }
    fn set_stroke_linear_gradient(&mut self, gradient: LinearGradientPaint) {
        self.state.stroke_linear_gradient = Some(gradient);
        self.state.stroke_radial_gradient = None;
    }
    fn supports_stroke_linear_gradient(&self) -> bool {
        true
    }
    fn set_stroke_radial_gradient(&mut self, gradient: RadialGradientPaint) {
        self.state.stroke_linear_gradient = None;
        self.state.stroke_radial_gradient = Some(gradient);
    }
    fn supports_stroke_radial_gradient(&self) -> bool {
        true
    }
    fn set_line_width(&mut self, w: f64) {
        self.state.line_width = w;
    }
    fn set_line_join(&mut self, j: LineJoin) {
        self.state.line_join = j;
    }
    fn set_line_cap(&mut self, c: LineCap) {
        self.state.line_cap = c;
    }
    fn set_miter_limit(&mut self, limit: f64) {
        self.state.miter_limit = limit.max(1.0);
    }
    fn set_line_dash(&mut self, dashes: &[f64], offset: f64) {
        self.state.line_dash.clear();
        self.state
            .line_dash
            .extend(dashes.iter().copied().filter(|v| *v > 0.0));
        self.state.dash_offset = offset;
    }
    fn set_blend_mode(&mut self, m: CompOp) {
        self.state.blend_mode = m;
    }
    fn set_global_alpha(&mut self, a: f64) {
        self.state.global_alpha = a.clamp(0.0, 1.0);
    }
    fn set_fill_rule(&mut self, r: FillRule) {
        self.state.fill_rule = r;
    }

    // ── Font ──────────────────────────────────────────────────────────────
    fn set_font(&mut self, f: Arc<Font>) {
        self.state.font = Some(f);
    }
    fn set_font_size(&mut self, s: f64) {
        self.state.font_size = s.max(1.0);
    }

    // ── Clipping ──────────────────────────────────────────────────────────
    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        // TODO step 2c — currently stored but not enforced; LcdMaskBuilder
        // needs a clip-aware variant before we can honour it during fill.
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
        let new_clip = (
            sx_min,
            sy_min,
            (sx_max - sx_min).max(0.0),
            (sy_max - sy_min).max(0.0),
        );
        self.state.clip = Some(match self.state.clip {
            Some((cx, cy, cw, ch)) => {
                let x1 = sx_min.max(cx);
                let y1 = sy_min.max(cy);
                let x2 = (new_clip.0 + new_clip.2).min(cx + cw);
                let y2 = (new_clip.1 + new_clip.3).min(cy + ch);
                (x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
            }
            None => new_clip,
        });
    }
    fn reset_clip(&mut self) {
        self.state.clip = None;
    }

    // ── Clear ─────────────────────────────────────────────────────────────
    fn clear(&mut self, color: Color) {
        self.active_buffer().clear(color);
    }

    // ── Path building ─────────────────────────────────────────────────────
    fn begin_path(&mut self) {
        self.path = PathStorage::new();
    }
    fn move_to(&mut self, x: f64, y: f64) {
        self.path.move_to(x, y);
    }
    fn line_to(&mut self, x: f64, y: f64) {
        self.path.line_to(x, y);
    }
    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        self.path.curve4(cx1, cy1, cx2, cy2, x, y);
    }
    fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.path.curve3(cx, cy, x, y);
    }
    fn arc_to(&mut self, cx: f64, cy: f64, r: f64, start_angle: f64, end_angle: f64, ccw: bool) {
        let mut arc = AggArc::new(cx, cy, r, r, start_angle, end_angle, ccw);
        self.path.concat_path(&mut arc, 0);
    }
    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        self.arc_to(cx, cy, r, 0.0, 2.0 * PI, true);
        self.path.close_polygon(PATH_FLAGS_NONE);
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.path.move_to(x, y);
        self.path.line_to(x + w, y);
        self.path.line_to(x + w, y + h);
        self.path.line_to(x, y + h);
        self.path.close_polygon(PATH_FLAGS_NONE);
    }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
        let mut rr = RoundedRect::new(x, y, x + w, y + h, r);
        rr.normalize_radius();
        self.path.concat_path(&mut rr, 0);
    }
    fn close_path(&mut self) {
        self.path.close_polygon(PATH_FLAGS_NONE);
    }

    // ── Path drawing ──────────────────────────────────────────────────────
    fn fill(&mut self) {
        let xform = self.state.transform;
        let clip = self.state.clip;
        let rule = self.state.fill_rule;
        // Borrow gymnastics: `fill_path` needs `&mut path` AND `&mut buffer`,
        // both fields of `self`.  Take the path out, fill into the active
        // buffer, then put the path back — preserves the "path persists
        // across fill calls" GfxCtx contract.
        let mut path = std::mem::replace(&mut self.path, PathStorage::new());
        if let Some(gradient) = self.state.fill_linear_gradient.clone() {
            let global_alpha = self.state.global_alpha as f32;
            gradient::fill_linear_gradient(
                self.active_buffer(),
                &mut path,
                &gradient,
                global_alpha,
                &xform,
                clip,
                rule,
            );
        } else if let Some(gradient) = self.state.fill_radial_gradient.clone() {
            let global_alpha = self.state.global_alpha as f32;
            gradient::fill_radial_gradient(
                self.active_buffer(),
                &mut path,
                &gradient,
                global_alpha,
                &xform,
                clip,
                rule,
            );
        } else {
            let mut color = self.state.fill_color;
            color.a *= self.state.global_alpha as f32;
            self.active_buffer()
                .fill_path(&mut path, color, &xform, clip, rule);
        }
        self.path = path;
    }
    fn stroke(&mut self) {
        stroke::stroke(self);
    }
    fn fill_and_stroke(&mut self) {
        self.fill();
        self.stroke();
    }

    fn draw_triangles_aa(
        &mut self,
        vertices: &[[f32; 3]],
        indices: &[u32],
        color: crate::color::Color,
    ) {
        // LCD-coverage-cache backbuffer doesn't have a dedicated halo-AA
        // path; rasterise each triangle as a solid fill, same as the
        // software `GfxCtx` path.
        let saved_fill = self.state.fill_color;
        self.state.fill_color = color;
        let n = indices.len() / 3;
        for t in 0..n {
            let i0 = indices[t * 3] as usize;
            let i1 = indices[t * 3 + 1] as usize;
            let i2 = indices[t * 3 + 2] as usize;
            if i0 >= vertices.len() || i1 >= vertices.len() || i2 >= vertices.len() {
                continue;
            }
            let v0 = vertices[i0];
            let v1 = vertices[i1];
            let v2 = vertices[i2];
            self.begin_path();
            self.move_to(v0[0] as f64, v0[1] as f64);
            self.line_to(v1[0] as f64, v1[1] as f64);
            self.line_to(v2[0] as f64, v2[1] as f64);
            self.close_path();
            self.fill();
        }
        self.state.fill_color = saved_fill;
    }

    // ── Text ──────────────────────────────────────────────────────────────
    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        let font = match self.state.font.clone() {
            Some(f) => f,
            None => return,
        };
        let mut color = self.state.fill_color;
        color.a *= self.state.global_alpha as f32;

        // HiDPI: rasterise at the **physical** font size (logical × CTM
        // scale).  See `gfx_ctx::fill_text` for the long version; short
        // version: the mask composites 1:1 at its rasterised pixel count,
        // so caching at logical size would shrink text on 2×/3× displays.
        let t = &self.state.transform;
        let ctm_scale = (t.sx * t.sx + t.shy * t.shy).sqrt().max(1e-6);
        let phys_size = self.state.font_size * ctm_scale;
        let cached = rasterize_text_lcd_cached(&font, text, phys_size);
        // Match the legacy CPU LCD compositor: apply CTM to the destination
        // origin, then snap to integer pixels.  Sub-pixel placement of an
        // LCD mask smears the per-channel phase pattern across pixel
        // boundaries (see `gfx_ctx::draw_lcd_mask` for the long story).
        // Divide `baseline_*_in_mask` by `ctm_scale` so offsets stay in
        // logical units that the CTM multiplies back to physical.
        let dst_x = x - cached.baseline_x_in_mask / ctm_scale;
        let dst_y = y - cached.baseline_y_in_mask / ctm_scale;
        let sx = (dst_x * t.sx + dst_y * t.shx + t.tx).round() as i32;
        let sy = (dst_x * t.shy + dst_y * t.sy + t.ty).round() as i32;

        // Construct a borrowed-shape `LcdMask` for the cached bytes.  The
        // clone is wasteful — Step 2b should give `composite_mask` a
        // slice variant so we can hand it `&cached.pixels[..]` with no
        // allocation.  For an MVP it doesn't matter.
        let mask = LcdMask {
            data: (*cached.pixels).clone(),
            width: cached.width,
            height: cached.height,
        };
        let clip_i = self.state.clip.map(crate::lcd_coverage::rect_to_pixel_clip);
        self.active_buffer()
            .composite_mask(&mask, color, sx, sy, clip_i);
    }

    fn fill_text_gsv(&mut self, text: &str, x: f64, y: f64, size: f64) {
        // GSV is AGG's stroke-vector font — used for placeholder text
        // before the real font is loaded.  We materialize the stroked
        // outline into a flat path and feed it through `fill_path`,
        // same shape as `stroke`.  Stroke width follows GfxCtx's choice
        // of `size * 0.1` for visual parity.
        let mut color = self.state.fill_color;
        color.a *= self.state.global_alpha as f32;
        let mut gsv = GsvText::new();
        gsv.size(size, 0.0);
        gsv.start_point(x, y);
        gsv.text(text);
        let mut materialized = PathStorage::new();
        {
            let mut stroke = ConvStroke::new(&mut gsv);
            stroke.set_width(size * 0.1);
            materialized.concat_path(&mut stroke, 0);
        }
        let xform = self.state.transform;
        let clip = self.state.clip;
        self.active_buffer()
            .fill_path(&mut materialized, color, &xform, clip, FillRule::NonZero);
    }

    fn measure_text(&self, text: &str) -> Option<TextMetrics> {
        let font = self.state.font.as_ref()?;
        Some(measure_text_metrics(font, text, self.state.font_size))
    }

    // ── Transform ─────────────────────────────────────────────────────────
    fn transform(&self) -> TransAffine {
        self.state.transform
    }
    fn root_transform(&self) -> TransAffine {
        let mut t = self.state.transform;
        for layer in self.layer_stack.iter().rev() {
            t.premultiply(&TransAffine::new_translation(
                layer.origin_x,
                layer.origin_y,
            ));
        }
        t
    }
    fn save(&mut self) {
        self.state_stack.push(self.state.clone());
    }
    fn restore(&mut self) {
        if let Some(s) = self.state_stack.pop() {
            self.state = s;
        }
    }
    fn translate(&mut self, tx: f64, ty: f64) {
        self.state
            .transform
            .premultiply(&TransAffine::new_translation(tx, ty));
    }
    fn rotate(&mut self, radians: f64) {
        self.state
            .transform
            .premultiply(&TransAffine::new_rotation(radians));
    }
    fn scale(&mut self, sx: f64, sy: f64) {
        self.state
            .transform
            .premultiply(&TransAffine::new_scaling(sx, sy));
    }
    fn set_transform(&mut self, m: TransAffine) {
        self.state.transform = m;
    }
    fn reset_transform(&mut self) {
        self.state.transform = TransAffine::new();
    }

    // ── Compositing layers ────────────────────────────────────────────────
    //
    // `push_layer` redirects subsequent paint into a fresh `LcdBuffer`;
    // `pop_layer` flushes that buffer back into the previously-active
    // one at the layer's recorded origin (the CTM translation at push
    // time).  Compositing is full-replace — see `LcdBuffer::composite_buffer`
    // for why LCD layers can't do alpha-aware SrcOver and what that
    // means for callers.

    fn push_layer(&mut self, width: f64, height: f64) {
        let origin_x = self.state.transform.tx;
        let origin_y = self.state.transform.ty;
        let lw = width.ceil().max(1.0) as u32;
        let lh = height.ceil().max(1.0) as u32;
        let mut layer_buffer = LcdBuffer::new(lw, lh);

        // Seed the layer with the parent's pixels at the layer's bounds.
        // Without this, an LCD layer would composite-replace the parent
        // region with its zero-init (black) wherever the user didn't paint —
        // any "untouched" pixel inside the layer would visibly clear the
        // parent on pop.  GfxCtx's RGBA layer dodges this with
        // alpha=0 + SrcOver; LcdBuffer has no alpha, so we inherit the
        // parent's content as the "neutral" starting state instead.
        let dx = -(origin_x.round() as i32);
        let dy = -(origin_y.round() as i32);
        let parent_ref: &LcdBuffer = if let Some(layer) = self.layer_stack.last() {
            &layer.buffer
        } else {
            &*self.base_buffer
        };
        layer_buffer.composite_buffer(parent_ref, dx, dy, None);

        let saved_state = self.state.clone();
        let saved_stack = std::mem::take(&mut self.state_stack);
        self.layer_stack.push(LcdLayer {
            buffer: layer_buffer,
            saved_state,
            saved_stack,
            origin_x,
            origin_y,
        });
        // Drawing inside the layer uses local coords (origin = layer's
        // bottom-left).  Match `GfxCtx::push_layer` semantics — the new
        // sub-region paints into a clean transform / no clip.
        self.state.transform = TransAffine::new();
        self.state.clip = None;
    }

    fn pop_layer(&mut self) {
        let Some(layer) = self.layer_stack.pop() else {
            return;
        };
        // Restore the state snapshot captured at push time.
        self.state = layer.saved_state;
        self.state_stack = layer.saved_stack;
        // Composite the layer onto whatever buffer is now active (could
        // be the base buffer, or another layer if we were nested).
        // Origin is in the parent's coords; round so the layer lands on
        // the integer pixel grid (same reason `draw_lcd_mask` rounds).
        let dst_x = layer.origin_x.round() as i32;
        let dst_y = layer.origin_y.round() as i32;
        let clip_i = self.state.clip.map(crate::lcd_coverage::rect_to_pixel_clip);
        self.active_buffer()
            .composite_buffer(&layer.buffer, dst_x, dst_y, clip_i);
    }

    // ── LCD mask compositing — native format for this ctx ─────────────────
    //
    // Unlike `GfxCtx` (which has a separate `lcd_mode` flag), an
    // `LcdGfxCtx`'s render target IS an LCD coverage buffer.  Compositing
    // an `LcdMask` is the most direct primitive available.

    fn draw_lcd_mask(
        &mut self,
        mask: &[u8],
        mask_w: u32,
        mask_h: u32,
        src_color: Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        if mask.len() < (mask_w as usize) * (mask_h as usize) * 3 {
            return;
        }
        let lcd_mask = LcdMask {
            data: mask.to_vec(),
            width: mask_w,
            height: mask_h,
        };
        let t = &self.state.transform;
        let sx = (dst_x * t.sx + dst_y * t.shx + t.tx).round() as i32;
        let sy = (dst_x * t.shy + dst_y * t.sy + t.ty).round() as i32;
        let clip_i = self.state.clip.map(crate::lcd_coverage::rect_to_pixel_clip);
        self.active_buffer()
            .composite_mask(&lcd_mask, src_color, sx, sy, clip_i);
    }

    fn has_lcd_mask_composite(&self) -> bool {
        true
    }

    // ── Image blitting ────────────────────────────────────────────────────
    //
    // Images are written as plain colour content — every subpixel of every
    // destination pixel mixes the source colour by the source's alpha.
    // We deliberately DON'T run image data through the 3× supersample +
    // 5-tap filter (the pipeline is for coverage, not colour) — that would
    // smear chroma across pixel boundaries and tint sharp icon edges with
    // R/G/B fringing.  This matches the convention of every LCD text
    // renderer (FreeType / CoreText / DirectWrite): subpixel treatment is
    // for glyph coverage; bitmaps go through standard alpha compositing.

    fn has_image_blit(&self) -> bool {
        true
    }

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
        let transform = self.state.transform;
        let global_alpha = self.state.global_alpha as f32;
        let clip = self.state.clip;
        image::draw_image_rgba(
            self.active_buffer(),
            data,
            img_w,
            img_h,
            dst_x,
            dst_y,
            dst_w,
            dst_h,
            &transform,
            global_alpha,
            clip,
        );
    }
}

fn configure_stroke<VS: VertexSource>(
    stroke: &mut ConvStroke<VS>,
    width: f64,
    join: LineJoin,
    cap: LineCap,
    miter_limit: f64,
) {
    stroke.set_width(width);
    stroke.set_line_join(join);
    stroke.set_line_cap(cap);
    stroke.set_miter_limit(miter_limit);
}

fn configure_dashes<VS: VertexSource>(dash: &mut ConvDash<VS>, dashes: &[f64], dash_offset: f64) {
    let mut chunks = dashes.chunks_exact(2);
    for pair in &mut chunks {
        dash.add_dash(pair[0], pair[1]);
    }
    if let Some(&last) = chunks.remainder().first() {
        dash.add_dash(last, last);
    }
    dash.dash_start(dash_offset);
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
