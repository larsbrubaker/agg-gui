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
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::comp_op::CompOp;
use agg_rust::conv_curve::ConvCurve;
use agg_rust::conv_stroke::ConvStroke;
use agg_rust::gsv_text::GsvText;
use agg_rust::math_stroke::{LineCap, LineJoin};
use agg_rust::path_storage::PathStorage;
use agg_rust::rounded_rect::RoundedRect;
use agg_rust::trans_affine::TransAffine;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::lcd_coverage::{rasterize_text_lcd_cached, LcdBuffer, LcdMask};
use crate::text::{measure_text_metrics, Font, TextMetrics};

// ── State ──────────────────────────────────────────────────────────────────
//
// Mirror of `GfxCtx`'s private `GfxState` so widgets that already set up
// fill colour / font / transform on a `GfxCtx` see the same field shape
// here — and so Step 2c can copy over any logic from the existing fill
// paths without translation.

#[derive(Clone)]
struct LcdState {
    transform:    TransAffine,
    fill_color:   Color,
    stroke_color: Color,
    line_width:   f64,
    line_join:    LineJoin,
    line_cap:     LineCap,
    blend_mode:   CompOp,
    global_alpha: f64,
    font:         Option<Arc<Font>>,
    font_size:    f64,
    /// Scissor clip in Y-up screen space `(x, y, w, h)`.  Stored but not
    /// yet enforced — `LcdMaskBuilder` doesn't accept a clip param yet.
    /// Step 2c.
    clip:         Option<(f64, f64, f64, f64)>,
}

impl Default for LcdState {
    fn default() -> Self {
        Self {
            transform:    TransAffine::new(),
            fill_color:   Color::black(),
            stroke_color: Color::black(),
            line_width:   1.0,
            line_join:    LineJoin::Round,
            line_cap:     LineCap::Round,
            blend_mode:   CompOp::SrcOver,
            global_alpha: 1.0,
            font:         None,
            font_size:    16.0,
            clip:         None,
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
    buffer:      LcdBuffer,
    /// State snapshot at the moment `push_layer` was called.  Restored
    /// verbatim on `pop_layer` so transform / clip / colour all return
    /// to their pre-layer values.
    saved_state: LcdState,
    saved_stack: Vec<LcdState>,
    /// Where the layer's bottom-left lands in the parent buffer's
    /// coords.  Captured from the CTM's translation at push time.
    origin_x:    f64,
    origin_y:    f64,
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
    state:       LcdState,
    state_stack: Vec<LcdState>,
    /// Accumulated path, reset by `begin_path`.  Same role as in
    /// `GfxCtx` — the `fill` / `stroke` calls consume it.
    path:        PathStorage,
}

impl<'a> LcdGfxCtx<'a> {
    pub fn new(buffer: &'a mut LcdBuffer) -> Self {
        Self {
            base_buffer: buffer,
            layer_stack: Vec::new(),
            state:       LcdState::default(),
            state_stack: Vec::new(),
            path:        PathStorage::new(),
        }
    }

    /// Read-only view of the underlying buffer — for callers that need
    /// to inspect output without releasing the ctx.  Returns the base
    /// buffer; callers inspecting mid-paint while a layer is active
    /// see only state committed before the current layer's push.
    pub fn buffer(&self) -> &LcdBuffer { self.base_buffer }

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
    fn set_fill_color  (&mut self, color: Color) { self.state.fill_color   = color; }
    fn set_stroke_color(&mut self, color: Color) { self.state.stroke_color = color; }
    fn set_line_width  (&mut self, w: f64)       { self.state.line_width   = w; }
    fn set_line_join   (&mut self, j: LineJoin)  { self.state.line_join    = j; }
    fn set_line_cap    (&mut self, c: LineCap)   { self.state.line_cap     = c; }
    fn set_blend_mode  (&mut self, m: CompOp)    { self.state.blend_mode   = m; }
    fn set_global_alpha(&mut self, a: f64)       { self.state.global_alpha = a.clamp(0.0, 1.0); }

    // ── Font ──────────────────────────────────────────────────────────────
    fn set_font     (&mut self, f: Arc<Font>) { self.state.font      = Some(f); }
    fn set_font_size(&mut self, s: f64)       { self.state.font_size = s.max(1.0); }

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
            let mut sx = lx; let mut sy = ly;
            t.transform(&mut sx, &mut sy);
            if sx < sx_min { sx_min = sx; }
            if sx > sx_max { sx_max = sx; }
            if sy < sy_min { sy_min = sy; }
            if sy > sy_max { sy_max = sy; }
        }
        let new_clip = (sx_min, sy_min, (sx_max - sx_min).max(0.0), (sy_max - sy_min).max(0.0));
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
    fn reset_clip(&mut self) { self.state.clip = None; }

    // ── Clear ─────────────────────────────────────────────────────────────
    fn clear(&mut self, color: Color) { self.active_buffer().clear(color); }

    // ── Path building ─────────────────────────────────────────────────────
    fn begin_path(&mut self)                  { self.path = PathStorage::new(); }
    fn move_to(&mut self, x: f64, y: f64)     { self.path.move_to(x, y); }
    fn line_to(&mut self, x: f64, y: f64)     { self.path.line_to(x, y); }
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
    fn close_path(&mut self) { self.path.close_polygon(PATH_FLAGS_NONE); }

    // ── Path drawing ──────────────────────────────────────────────────────
    fn fill(&mut self) {
        let mut color = self.state.fill_color;
        color.a *= self.state.global_alpha as f32;
        let xform = self.state.transform;
        let clip  = self.state.clip;
        // Borrow gymnastics: `fill_path` needs `&mut path` AND `&mut buffer`,
        // both fields of `self`.  Take the path out, fill into the active
        // buffer, then put the path back — preserves the "path persists
        // across fill calls" GfxCtx contract.
        let mut path = std::mem::replace(&mut self.path, PathStorage::new());
        self.active_buffer().fill_path(&mut path, color, &xform, clip);
        self.path = path;
    }
    fn stroke(&mut self) {
        // Materialize the stroked outline as a flat polygon, then route it
        // through the same `fill_path` LCD pipeline as a regular fill.
        // This is one indirection more than `GfxCtx::stroke` (which feeds
        // `ConvStroke` straight to AGG) — we accept the extra `concat_path`
        // because it avoids duplicating the gray-buffer scaffolding here
        // and keeps `LcdBuffer::fill_path` the single inner primitive.
        //
        // Stroke width is in user coordinates (matches `GfxCtx`): the CTM
        // applied inside `fill_path` scales it just like any other geometry,
        // so a 1-px stroke at scale=2 paints 2 pixels wide.
        let mut color = self.state.stroke_color;
        color.a *= self.state.global_alpha as f32;
        let mut materialized = PathStorage::new();
        {
            let mut curves = ConvCurve::new(&mut self.path);
            let mut stroke = ConvStroke::new(&mut curves);
            stroke.set_width(self.state.line_width);
            stroke.set_line_join(self.state.line_join);
            stroke.set_line_cap(self.state.line_cap);
            materialized.concat_path(&mut stroke, 0);
        }
        let xform = self.state.transform;
        let clip  = self.state.clip;
        self.active_buffer().fill_path(&mut materialized, color, &xform, clip);
    }
    fn fill_and_stroke(&mut self) {
        self.fill();
        self.stroke();
    }

    // ── Text ──────────────────────────────────────────────────────────────
    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        let font = match self.state.font.clone() {
            Some(f) => f,
            None => return,
        };
        let mut color = self.state.fill_color;
        color.a *= self.state.global_alpha as f32;

        let cached = rasterize_text_lcd_cached(&font, text, self.state.font_size);
        // Match the legacy CPU LCD compositor: apply CTM to the destination
        // origin, then snap to integer pixels.  Sub-pixel placement of an
        // LCD mask smears the per-channel phase pattern across pixel
        // boundaries (see `gfx_ctx::draw_lcd_mask` for the long story).
        let dst_x = x - cached.baseline_x_in_mask;
        let dst_y = y - cached.baseline_y_in_mask;
        let t = &self.state.transform;
        let sx = (dst_x * t.sx + dst_y * t.shx + t.tx).round() as i32;
        let sy = (dst_x * t.shy + dst_y * t.sy + t.ty).round() as i32;

        // Construct a borrowed-shape `LcdMask` for the cached bytes.  The
        // clone is wasteful — Step 2b should give `composite_mask` a
        // slice variant so we can hand it `&cached.pixels[..]` with no
        // allocation.  For an MVP it doesn't matter.
        let mask = LcdMask {
            data:   (*cached.pixels).clone(),
            width:  cached.width,
            height: cached.height,
        };
        let clip_i = self.state.clip.map(crate::lcd_coverage::rect_to_pixel_clip);
        self.active_buffer().composite_mask(&mask, color, sx, sy, clip_i);
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
        let clip  = self.state.clip;
        self.active_buffer().fill_path(&mut materialized, color, &xform, clip);
    }

    fn measure_text(&self, text: &str) -> Option<TextMetrics> {
        let font = self.state.font.as_ref()?;
        Some(measure_text_metrics(font, text, self.state.font_size))
    }

    // ── Transform ─────────────────────────────────────────────────────────
    fn transform(&self) -> TransAffine { self.state.transform }
    fn save   (&mut self) { self.state_stack.push(self.state.clone()); }
    fn restore(&mut self) {
        if let Some(s) = self.state_stack.pop() { self.state = s; }
    }
    fn translate(&mut self, tx: f64, ty: f64) {
        self.state.transform.premultiply(&TransAffine::new_translation(tx, ty));
    }
    fn rotate(&mut self, radians: f64) {
        self.state.transform.premultiply(&TransAffine::new_rotation(radians));
    }
    fn scale(&mut self, sx: f64, sy: f64) {
        self.state.transform.premultiply(&TransAffine::new_scaling(sx, sy));
    }
    fn set_transform(&mut self, m: TransAffine) { self.state.transform = m; }
    fn reset_transform(&mut self)               { self.state.transform = TransAffine::new(); }

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
        let lw = width.ceil().max(1.0)  as u32;
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
        self.state.clip      = None;
    }

    fn pop_layer(&mut self) {
        let Some(layer) = self.layer_stack.pop() else { return; };
        // Restore the state snapshot captured at push time.
        self.state       = layer.saved_state;
        self.state_stack = layer.saved_stack;
        // Composite the layer onto whatever buffer is now active (could
        // be the base buffer, or another layer if we were nested).
        // Origin is in the parent's coords; round so the layer lands on
        // the integer pixel grid (same reason `draw_lcd_mask` rounds).
        let dst_x = layer.origin_x.round() as i32;
        let dst_y = layer.origin_y.round() as i32;
        let clip_i = self.state.clip.map(crate::lcd_coverage::rect_to_pixel_clip);
        self.active_buffer().composite_buffer(&layer.buffer, dst_x, dst_y, clip_i);
    }

    // ── LCD mask compositing — native format for this ctx ─────────────────
    //
    // Unlike `GfxCtx` (which has a separate `lcd_mode` flag), an
    // `LcdGfxCtx`'s render target IS an LCD coverage buffer.  Compositing
    // an `LcdMask` is the most direct primitive available.

    fn draw_lcd_mask(
        &mut self,
        mask:      &[u8],
        mask_w:    u32,
        mask_h:    u32,
        src_color: Color,
        dst_x:     f64,
        dst_y:     f64,
    ) {
        if mask.len() < (mask_w as usize) * (mask_h as usize) * 3 { return; }
        let lcd_mask = LcdMask { data: mask.to_vec(), width: mask_w, height: mask_h };
        let t = &self.state.transform;
        let sx = (dst_x * t.sx + dst_y * t.shx + t.tx).round() as i32;
        let sy = (dst_x * t.shy + dst_y * t.sy + t.ty).round() as i32;
        let clip_i = self.state.clip.map(crate::lcd_coverage::rect_to_pixel_clip);
        self.active_buffer().composite_mask(&lcd_mask, src_color, sx, sy, clip_i);
    }

    fn has_lcd_mask_composite(&self) -> bool { true }

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
        if img_w == 0 || img_h == 0 { return; }
        if dst_w <= 0.0 || dst_h <= 0.0 { return; }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 { return; }

        // Apply CTM to destination origin, snap to integer pixel grid.
        // Pixel-snap matters here for the same reason it matters for LCD
        // text: NEAREST sampling at fractional offsets picks the wrong
        // texel half the time and the icon visibly shifts.  Sample-area
        // size is taken from the CTM's scale factors — for the typical
        // pure-translation CTM that's just dst_w × dst_h.
        let t = &self.state.transform;
        let ox = (dst_x * t.sx + dst_y * t.shx + t.tx).round() as i32;
        let oy = (dst_x * t.shy + dst_y * t.sy + t.ty).round() as i32;
        let scaled_w = ((dst_w * t.sx).abs()).round() as i32;
        let scaled_h = ((dst_h * t.sy).abs()).round() as i32;
        if scaled_w <= 0 || scaled_h <= 0 { return; }

        let global_alpha = (self.state.global_alpha as f32).clamp(0.0, 1.0);
        let clip_i       = self.state.clip.map(crate::lcd_coverage::rect_to_pixel_clip);

        let buf = self.active_buffer();
        let buf_w   = buf.width()  as i32;
        let buf_h   = buf.height() as i32;
        let buf_w_u = buf_w as usize;
        let img_w_u = img_w as usize;

        // Intersect any active clip with the buffer's bounds — the inner
        // loop becomes a single range check per pixel against this rect.
        let (cx1, cy1, cx2, cy2) = match clip_i {
            Some((x1, y1, x2, y2)) => (x1.max(0), y1.max(0), x2.min(buf_w), y2.min(buf_h)),
            None => (0, 0, buf_w, buf_h),
        };
        if cx1 >= cx2 || cy1 >= cy2 { return; }

        let (color_plane, alpha_plane) = buf.planes_mut();
        for ly in 0..scaled_h {
            let dy = oy + ly;
            if dy < cy1 || dy >= cy2 { continue; }
            // ly = 0 is bottom of dst rect (Y-up).  Source image is stored
            // top-row-first, so the bottom of the visual image is row
            // `img_h - 1` and that's what we sample first.
            let frac_y = (ly as f64 + 0.5) / (scaled_h as f64);
            let sy_visual = (frac_y * img_h as f64) as u32;
            let sy_visual = sy_visual.min(img_h - 1);
            let sy_storage = (img_h - 1 - sy_visual) as usize;

            for lx in 0..scaled_w {
                let dx = ox + lx;
                if dx < cx1 || dx >= cx2 { continue; }
                let frac_x = (lx as f64 + 0.5) / (scaled_w as f64);
                let sx_storage = ((frac_x * img_w as f64) as u32).min(img_w - 1) as usize;

                // Source image is straight-alpha RGBA; effective src alpha =
                // image alpha × ctx global_alpha.  Regular images have one
                // alpha per pixel — we apply it identically across all three
                // subpixel channels (no per-subpixel variation for source).
                // That's the one case where the per-channel-alpha buffer
                // takes redundant data; true per-subpixel image edges would
                // come from a rasteriser-based image path, not NEAREST blit.
                let si = (sy_storage * img_w_u + sx_storage) * 4;
                let sa = (data[si + 3] as f32 / 255.0) * global_alpha;
                if sa <= 0.0 { continue; }
                let sr = (data[si]     as f32 / 255.0) * sa;   // premultiply
                let sg = (data[si + 1] as f32 / 255.0) * sa;
                let sb = (data[si + 2] as f32 / 255.0) * sa;

                let di = ((dy as usize) * buf_w_u + (dx as usize)) * 3;

                // Read current premult colour + per-channel alpha.
                let bc_r = color_plane[di]     as f32 / 255.0;
                let bc_g = color_plane[di + 1] as f32 / 255.0;
                let bc_b = color_plane[di + 2] as f32 / 255.0;
                let ba_r = alpha_plane[di]     as f32 / 255.0;
                let ba_g = alpha_plane[di + 1] as f32 / 255.0;
                let ba_b = alpha_plane[di + 2] as f32 / 255.0;

                // Premult src-over per channel (all three share `sa` since
                // the source image had a single per-pixel alpha).
                let rc_r = sr + bc_r * (1.0 - sa);
                let rc_g = sg + bc_g * (1.0 - sa);
                let rc_b = sb + bc_b * (1.0 - sa);
                let ra_r = sa + ba_r * (1.0 - sa);
                let ra_g = sa + ba_g * (1.0 - sa);
                let ra_b = sa + ba_b * (1.0 - sa);

                color_plane[di]     = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                color_plane[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                color_plane[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                alpha_plane[di]     = (ra_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                alpha_plane[di + 1] = (ra_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                alpha_plane[di + 2] = (ra_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framebuffer::Framebuffer;
    use crate::gfx_ctx::GfxCtx;

    const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

    fn font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    /// Smoke test: an `LcdGfxCtx` constructed over a fresh `LcdBuffer`
    /// can `clear` + `set_fill_color` + `set_font` + `fill_text` without
    /// panicking, and produces non-zero coverage somewhere.  Catches
    /// any state-plumbing typo that would silently no-op the path.
    #[test]
    fn test_lcd_gfx_ctx_basic_fill_text_smoke() {
        let mut buf = LcdBuffer::new(80, 24);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.set_font(font());
            ctx.set_font_size(16.0);
            ctx.fill_text("ABC", 4.0, 14.0);
        }
        // Some pixels should be darker than white (where text was painted).
        let any_dark = buf.color_plane().chunks_exact(3)
            .any(|p| p[0] < 250 || p[1] < 250 || p[2] < 250);
        assert!(any_dark, "fill_text via LcdGfxCtx left buffer fully white");
    }

    /// **End-to-end equivalence (Step 2 contract).**
    ///
    /// Painting the SAME text two ways must produce byte-identical RGB:
    ///
    ///   A. Legacy: `GfxCtx` over an RGBA `Framebuffer` with `lcd_mode=true`.
    ///   B. New:    `LcdGfxCtx` over an `LcdBuffer`.
    ///
    /// Both routes go through `rasterize_text_lcd_cached` (same mask) and
    /// per-channel src-over compositing (same math); the only difference
    /// is destination format (4 bytes vs 3 bytes per pixel).  If the RGB
    /// triplets diverge, the new ctx is producing a different mask
    /// placement or compositor than the existing one, and any widget
    /// rewired to paint into an `LcdGfxCtx` would visibly disagree with
    /// today's text rendering.  This is the contract Step 3 (wiring the
    /// ctx into `paint_subtree_backbuffered`) builds on.
    #[test]
    fn test_lcd_gfx_ctx_text_matches_legacy_lcd_mode() {
        let f  = font();
        let w  = 120u32;
        let h  = 28u32;

        // Way A — legacy `GfxCtx + lcd_mode=true` onto RGBA `Framebuffer`.
        let mut fb = Framebuffer::new(w, h);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            ctx.set_lcd_mode(true);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.set_font(Arc::clone(&f));
            ctx.set_font_size(18.0);
            <GfxCtx as DrawCtx>::fill_text(&mut ctx, "Hello!", 4.0, 18.0);
        }

        // Way B — new `LcdGfxCtx` onto `LcdBuffer`.
        let mut buf = LcdBuffer::new(w, h);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.set_font(Arc::clone(&f));
            ctx.set_font_size(18.0);
            ctx.fill_text("Hello!", 4.0, 18.0);
        }

        // Compare RGB triplets at every pixel — alpha column in `fb`
        // is not part of the contract (LcdBuffer has no alpha to match
        // against).
        for y in 0..h as usize {
            for x in 0..w as usize {
                let ai = (y * w as usize + x) * 4;
                let bi = (y * w as usize + x) * 3;
                let a_rgb = (fb.pixels()[ai], fb.pixels()[ai + 1], fb.pixels()[ai + 2]);
                let b_rgb = (buf.color_plane()[bi], buf.color_plane()[bi + 1], buf.color_plane()[bi + 2]);
                assert_eq!(a_rgb, b_rgb,
                    "pixel mismatch at ({x},{y}): legacy={a_rgb:?} LcdGfxCtx={b_rgb:?}");
            }
        }
    }

    // ── Step 2c: stroke / arc / circle / rounded_rect / image blit ──────────

    /// `stroke` of a horizontal line must deposit dark pixels along the
    /// line's path.  Uses width=1, so we expect the line's row to read
    /// noticeably darker than the surrounding rows.
    #[test]
    fn test_lcd_gfx_ctx_stroke_horizontal_line() {
        let mut buf = LcdBuffer::new(20, 11);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_stroke_color(Color::black());
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(2.0, 5.0);
            ctx.line_to(18.0, 5.0);
            ctx.stroke();
        }
        let row_brightness = |y: usize| -> u32 {
            (4..16).map(|x| {
                let i = (y * 20 + x) * 3;
                buf.color_plane()[i] as u32 + buf.color_plane()[i + 1] as u32 + buf.color_plane()[i + 2] as u32
            }).sum()
        };
        let line  = row_brightness(5);  // line row in Y-up
        let above = row_brightness(8);
        let below = row_brightness(2);
        assert!(line < above, "stroke row should be darker than row above (line={line}, above={above})");
        assert!(line < below, "stroke row should be darker than row below (line={line}, below={below})");
    }

    /// `circle` then `fill` must darken the centre but leave a corner
    /// well outside the disc untouched — proves arc emission + concat
    /// produce a closed region rather than degenerating to nothing.
    #[test]
    fn test_lcd_gfx_ctx_circle_darkens_center_not_corner() {
        let mut buf = LcdBuffer::new(20, 20);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.circle(10.0, 10.0, 5.0);
            ctx.fill();
        }
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 20 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        let (cr, cg, cb) = pixel(10, 10);
        assert!(cr < 60 && cg < 60 && cb < 60,
            "circle centre should be dark; got ({cr}, {cg}, {cb})");
        let (xr, xg, xb) = pixel(1, 1);
        assert!(xr > 240 && xg > 240 && xb > 240,
            "outside-circle corner should stay white; got ({xr}, {xg}, {xb})");
    }

    /// `rounded_rect` — corner pixels must remain background (rounded
    /// off), while the centre is filled.  Catches a missing
    /// `concat_path` or a bogus radius normalize that would degenerate
    /// the rounded rect to a sharp rect or to nothing.
    ///
    /// Rect (0,0)–(20,20) with r=8: the BL corner arc has centre (8,8)
    /// and radius 8, so any pixel outside that arc (distance from (8,8)
    /// > 8) but inside the bbox is in the "rounded-off" region.  We
    /// pick (1,1) which is ~9.9 px from (8,8) — well past the arc edge,
    /// so AA leak from the LCD filter (which has ±2 subpixel = ~0.67
    /// pixel reach) cannot reach it.
    #[test]
    fn test_lcd_gfx_ctx_rounded_rect_clips_corners() {
        let mut buf = LcdBuffer::new(20, 20);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, 20.0, 20.0, 8.0);
            ctx.fill();
        }
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 20 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        // Centre fully inside the rounded rect → dark.
        let (cr, cg, cb) = pixel(10, 10);
        assert!(cr < 50 && cg < 50 && cb < 50,
            "rounded rect centre should be dark; got ({cr}, {cg}, {cb})");
        // Far corner of the bbox (1, 1) — beyond the corner arc, inside
        // the rounded-off region.  Must remain white.
        let (xr, xg, xb) = pixel(1, 1);
        assert!(xr > 240 && xg > 240 && xb > 240,
            "rounded rect corner area should stay white; got ({xr}, {xg}, {xb})");
        // Mid-edge (10, 1) — inside the rect on its straight bottom edge,
        // far from any corner arc.  Must be dark.
        let (er, eg, eb) = pixel(10, 1);
        assert!(er < 50 && eg < 50 && eb < 50,
            "rounded rect mid-edge should be dark; got ({er}, {eg}, {eb})");
    }

    /// Image blit with Y-flip: a 2×2 source image with distinct colours
    /// per cell (top-left=red, top-right=green, bottom-left=blue,
    /// bottom-right=opaque-grey).  After blit into a Y-up LcdBuffer at
    /// (1,1), the source's top row must land at the buffer's TOP-of-rect
    /// row (Y-up = higher Y), the bottom row at the BOTTOM-of-rect row.
    /// Catches any Y-flip arithmetic mistake.
    #[test]
    fn test_lcd_gfx_ctx_image_blit_y_flips_correctly() {
        // RGBA, top-row first.
        let img: Vec<u8> = vec![
            // Row 0 (top): red, green
            255,   0,   0, 255,    0, 255,   0, 255,
            // Row 1 (bottom): blue, grey
              0,   0, 255, 255,  128, 128, 128, 255,
        ];
        let mut buf = LcdBuffer::new(8, 8);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::black());
            ctx.draw_image_rgba(&img, 2, 2, 1.0, 1.0, 2.0, 2.0);
        }
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 8 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        // Y-up: y=1 is bottom row of dst rect, y=2 is top.  Source's top
        // row (row 0 in storage) is the visually-top row, which lands at
        // buffer y=2.
        assert_eq!(pixel(1, 2), (255,   0,   0), "top-left source must land at top-left of dst rect (Y-up high)");
        assert_eq!(pixel(2, 2), (  0, 255,   0), "top-right source must land at top-right of dst rect");
        assert_eq!(pixel(1, 1), (  0,   0, 255), "bottom-left source must land at bottom-left of dst rect (Y-up low)");
        assert_eq!(pixel(2, 1), (128, 128, 128), "bottom-right source must land at bottom-right of dst rect");
        // Outside the blit rect — untouched.
        assert_eq!(pixel(0, 0), (0, 0, 0), "pixel outside blit rect should be untouched");
    }

    /// Image blit alpha — a half-transparent source over a known bg
    /// must produce per-channel src-over output (alpha is the same on
    /// all three subpixels for image data, by design).
    #[test]
    fn test_lcd_gfx_ctx_image_blit_alpha_blends_with_destination() {
        // Single pixel: red at 50% alpha (straight-alpha encoding).
        let img: Vec<u8> = vec![255, 0, 0, 128];
        let mut buf = LcdBuffer::new(4, 4);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.draw_image_rgba(&img, 1, 1, 1.0, 1.0, 1.0, 1.0);
        }
        let i = (1 * 4 + 1) * 3;
        let (r, g, b) = (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2]);
        // Expected: src(255,0,0) * 0.502 + dst(255,255,255) * 0.498
        //         = (255, ~127, ~127)  (slightly biased by quantization)
        assert!(r > 250,           "R should be near 255 (bg + src red); got {r}");
        assert!(g > 120 && g < 140, "G should be near 127 (white minus alpha-attenuated red); got {g}");
        assert!(b > 120 && b < 140, "B should be near 127; got {b}");
    }

    // ── Step 2d.1: clip enforcement ─────────────────────────────────────────

    /// `fill` of a rect that crosses the clip boundary must darken
    /// only the pixels inside the clip; the half outside the clip
    /// stays untouched.  Catches a missing clip plumb-through to
    /// either the AGG raster step or the composite step.
    #[test]
    fn test_lcd_gfx_ctx_clip_rect_constrains_fill() {
        let mut buf = LcdBuffer::new(20, 10);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.clip_rect(0.0, 0.0, 10.0, 10.0);   // clip to LEFT half
            ctx.begin_path();
            ctx.rect(2.0, 2.0, 16.0, 6.0);          // straddles the clip edge
            ctx.fill();
        }
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 20 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        // Inside clip + inside rect → dark.
        let (lr, lg, lb) = pixel(5, 5);
        assert!(lr < 50 && lg < 50 && lb < 50,
            "pixel inside clip + rect should be dark; got ({lr}, {lg}, {lb})");
        // Outside clip but inside rect → must stay white.
        let (rr, rg, rb) = pixel(15, 5);
        assert!(rr > 240 && rg > 240 && rb > 240,
            "pixel outside clip should stay white; got ({rr}, {rg}, {rb})");
    }

    /// `fill_text` honours the clip — text that runs past the clip
    /// edge should leave the post-clip region untouched.  Set up a
    /// long string and a short clip; sample beyond the clip edge.
    #[test]
    fn test_lcd_gfx_ctx_clip_rect_constrains_fill_text() {
        let mut buf = LcdBuffer::new(120, 24);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.set_font(font());
            ctx.set_font_size(18.0);
            ctx.clip_rect(0.0, 0.0, 40.0, 24.0);    // clip to first ~40 px
            ctx.fill_text("MMMMMMMMMMMM", 2.0, 18.0);
        }
        // Inside clip, on glyph stroke → expect some dark pixel in the
        // first 40 px columns.
        let mut saw_dark_inside = false;
        for x in 0..40 {
            for y in 0..24 {
                let i = (y * 120 + x) * 3;
                if buf.color_plane()[i] < 100 { saw_dark_inside = true; break; }
            }
            if saw_dark_inside { break; }
        }
        assert!(saw_dark_inside, "expected some dark text pixel inside the clip");

        // Outside clip — every pixel beyond x=42 (a small margin past
        // the clip edge to absorb the 5-tap filter's ±2 subpixel reach)
        // must remain white.
        for x in 42..120 {
            for y in 0..24 {
                let i = (y * 120 + x) * 3;
                let (r, g, b) = (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2]);
                assert!(r > 240 && g > 240 && b > 240,
                    "pixel at ({x},{y}) outside clip should stay white; got ({r}, {g}, {b})");
            }
        }
    }

    /// `draw_image_rgba` honours the clip — pixels outside the clip
    /// rect stay untouched even though the source image's destination
    /// rect overlaps them.
    #[test]
    fn test_lcd_gfx_ctx_clip_rect_constrains_image_blit() {
        // Solid red 10×10 RGBA.
        let img: Vec<u8> = (0..10*10).flat_map(|_| [255u8, 0, 0, 255]).collect();
        let mut buf = LcdBuffer::new(20, 10);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.clip_rect(0.0, 0.0, 5.0, 10.0);     // clip to leftmost 5 columns
            ctx.draw_image_rgba(&img, 10, 10, 0.0, 0.0, 10.0, 10.0);
        }
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 20 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        // Inside clip → red.
        assert_eq!(pixel(2, 5), (255, 0, 0), "inside clip should show source red");
        // Outside clip → white (image suppressed there).
        assert_eq!(pixel(7, 5), (255, 255, 255), "outside clip should stay white");
    }

    /// `reset_clip` removes a previously-set clip — paint after the
    /// reset should reach the full buffer again.
    #[test]
    fn test_lcd_gfx_ctx_reset_clip_restores_full_buffer() {
        let mut buf = LcdBuffer::new(20, 10);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.clip_rect(0.0, 0.0, 5.0, 10.0);
            ctx.reset_clip();
            ctx.begin_path();
            ctx.rect(2.0, 2.0, 16.0, 6.0);          // would be clipped at x=5 if clip remained
            ctx.fill();
        }
        // Pixel at x=15 should now be dark (no clip blocking it).
        let i = (5 * 20 + 15) * 3;
        let (r, g, b) = (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2]);
        assert!(r < 50 && g < 50 && b < 50,
            "after reset_clip, fill at x=15 should be dark; got ({r}, {g}, {b})");
    }

    /// Nested `clip_rect` calls intersect — the second call narrows
    /// the active clip, doesn't replace it.  Mirrors `GfxCtx::clip_rect`
    /// semantics so widget code that nests clips behaves identically.
    #[test]
    fn test_lcd_gfx_ctx_clip_rect_nests_via_intersection() {
        let mut buf = LcdBuffer::new(20, 20);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            // Outer clip: left half.
            ctx.clip_rect(0.0, 0.0, 10.0, 20.0);
            // Inner clip: top half.  Intersection = top-left quadrant.
            ctx.clip_rect(0.0, 10.0, 20.0, 10.0);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, 20.0, 20.0);          // would fill everything if no clip
            ctx.fill();
        }
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 20 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        // Top-left (inside intersection) — dark.
        let (tlr, tlg, tlb) = pixel(2, 17);
        assert!(tlr < 50 && tlg < 50 && tlb < 50,
            "top-left should be dark; got ({tlr}, {tlg}, {tlb})");
        // Top-right (outside outer clip) — white.
        let (trr, trg, trb) = pixel(17, 17);
        assert!(trr > 240 && trg > 240 && trb > 240,
            "top-right should stay white; got ({trr}, {trg}, {trb})");
        // Bottom-left (outside inner clip) — white.
        let (blr, blg, blb) = pixel(2, 2);
        assert!(blr > 240 && blg > 240 && blb > 240,
            "bottom-left should stay white; got ({blr}, {blg}, {blb})");
    }

    // ── Step 2d.2: push_layer / pop_layer ───────────────────────────────────

    /// Sanity: paint inside a `push_layer`/`pop_layer` block lands in
    /// the parent buffer at the recorded origin.  Catches a missing
    /// composite-on-pop or a wrong-origin bug.
    #[test]
    fn test_lcd_gfx_ctx_push_pop_layer_flushes_into_parent() {
        let mut buf = LcdBuffer::new(20, 20);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            // Translate the parent so the layer lands at (5, 5) in the
            // base buffer's coords — exercises the origin pickup from
            // the CTM at push time.
            ctx.translate(5.0, 5.0);
            ctx.push_layer(8.0, 8.0);
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, 8.0, 8.0);          // fills the whole layer
            ctx.fill();
            ctx.pop_layer();
        }
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 20 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        // Inside the layer's destination region in the parent → dark.
        assert_eq!(pixel(8, 8), (0, 0, 0), "interior of flushed layer should be dark");
        // Just outside the layer's region → still white.
        assert_eq!(pixel(2, 2), (255, 255, 255), "outside layer region should stay white");
        assert_eq!(pixel(15, 15), (255, 255, 255), "outside layer region should stay white");
    }

    /// State must be restored after `pop_layer`: the fill colour, font
    /// size, transform, and clip rect set inside the layer must NOT
    /// leak out into the parent's subsequent paint.  Also: the layer's
    /// transform starts at identity (matches `GfxCtx::push_layer`).
    #[test]
    fn test_lcd_gfx_ctx_push_pop_layer_restores_state() {
        let mut buf = LcdBuffer::new(20, 20);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());

            ctx.set_fill_color(Color::white());     // pre-layer fill colour
            ctx.translate(3.0, 4.0);
            assert_eq!((ctx.transform().tx, ctx.transform().ty), (3.0, 4.0));

            ctx.push_layer(10.0, 10.0);
            // Inside the layer transform must reset to identity.
            assert_eq!((ctx.transform().tx, ctx.transform().ty), (0.0, 0.0),
                "push_layer must reset transform inside the layer");
            // Mutate state inside the layer.
            ctx.set_fill_color(Color::rgba(0.1, 0.2, 0.3, 1.0));
            ctx.translate(1.0, 1.0);
            ctx.pop_layer();

            // After pop: transform restored to (3, 4); fill colour restored
            // to white.
            assert_eq!((ctx.transform().tx, ctx.transform().ty), (3.0, 4.0),
                "pop_layer must restore transform to its push-time value");

            // Verify fill colour by painting and inspecting bg-untouched
            // pixels.  We fill a small rect into the parent — if the
            // fill colour were the leaked dark teal, those pixels would
            // be that, not white.
            ctx.begin_path();
            ctx.rect(0.0, 0.0, 4.0, 4.0);
            ctx.fill();
        }
        // The post-pop fill happens at translate(3,4), filling rect (3..7, 4..8).
        // Fill colour is white (restored) → those pixels must be white.
        let i = (5 * 20 + 5) * 3;
        let (r, g, b) = (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2]);
        assert_eq!((r, g, b), (255, 255, 255), "post-pop fill must use restored white colour");
    }

    /// Paint inside a layer must NOT touch the parent buffer until pop.
    /// Inspect the parent buffer mid-layer and verify the painted pixels
    /// haven't appeared yet.
    #[test]
    fn test_lcd_gfx_ctx_push_layer_isolates_paint_until_pop() {
        let mut buf = LcdBuffer::new(20, 20);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.push_layer(10.0, 10.0);
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, 10.0, 10.0);
            ctx.fill();
            // Mid-layer: parent buffer's pixels must still be all white.
            let base = ctx.buffer();
            assert!(base.color_plane().chunks_exact(3).all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255),
                "base buffer must not see layer paint until pop_layer");
            ctx.pop_layer();
        }
        // After pop: pixels (0..10, 0..10) should be dark.
        let i = (5 * 20 + 5) * 3;
        let (r, g, b) = (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2]);
        assert_eq!((r, g, b), (0, 0, 0), "after pop_layer, painted pixels should appear in base");
    }

    /// Nested layers compose correctly: outer layer flushes the inner
    /// layer's contribution as part of its own flush.  Catches stack
    /// management bugs where a pop misroutes which buffer becomes
    /// "active" after.
    #[test]
    fn test_lcd_gfx_ctx_push_layer_nests() {
        let mut buf = LcdBuffer::new(30, 30);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.translate(2.0, 2.0);
            ctx.push_layer(20.0, 20.0);                // outer layer at (2,2)
            ctx.set_fill_color(Color::black());

            ctx.translate(4.0, 4.0);
            ctx.push_layer(8.0, 8.0);                  // inner layer at (4,4) within outer
            ctx.begin_path();
            ctx.rect(0.0, 0.0, 8.0, 8.0);
            ctx.fill();
            ctx.pop_layer();                           // flush inner → outer at (4,4)

            ctx.pop_layer();                           // flush outer → base at (2,2)
        }
        // Inner layer fills (0..8, 0..8) of itself.  Outer composites it
        // at (4,4) → outer pixels (4..12, 4..12) = inner content.  Base
        // composites outer at (2,2) → base pixels (6..14, 6..14) = inner
        // black region.
        let pixel = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 30 + x) * 3;
            (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2])
        };
        assert_eq!(pixel(10, 10), (0, 0, 0), "centre of nested layer region should be dark");
        assert_eq!(pixel(2, 2),   (255, 255, 255), "well outside nested region should stay white");
        assert_eq!(pixel(20, 20), (255, 255, 255), "well outside nested region should stay white");
    }

    /// Unmatched `pop_layer` (no preceding `push_layer`) must be a
    /// silent no-op — same contract as `GfxCtx::pop_layer`.
    #[test]
    fn test_lcd_gfx_ctx_unmatched_pop_layer_is_noop() {
        let mut buf = LcdBuffer::new(8, 8);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf);
            ctx.clear(Color::white());
            ctx.pop_layer();   // must not panic
            ctx.set_fill_color(Color::black());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, 8.0, 8.0);
            ctx.fill();
        }
        // Subsequent paint still works — sample an INTERIOR pixel; the
        // 5-tap LCD filter naturally produces partial coverage at the
        // buffer edges (subpixel samples beyond the buffer read as 0)
        // which is a known + correct property of the pipeline.
        let i = (4 * 8 + 4) * 3;
        let (r, g, b) = (buf.color_plane()[i], buf.color_plane()[i + 1], buf.color_plane()[i + 2]);
        assert_eq!((r, g, b), (0, 0, 0), "subsequent paint after unmatched pop should still work");
    }

    /// CTM must be honoured by `fill_text` — translating the ctx by
    /// `(dx, dy)` then drawing at `(x, y)` should land at the same pixel
    /// as drawing at `(x+dx, y+dy)` with no translation.  Guards against
    /// "forgot to apply CTM in the LCD path" bugs (we hit one of those
    /// in the legacy path two iterations ago).
    #[test]
    fn test_lcd_gfx_ctx_fill_text_honours_translation() {
        let f = font();
        let w = 100u32;
        let h = 24u32;

        let mut buf_a = LcdBuffer::new(w, h);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf_a);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.set_font(Arc::clone(&f));
            ctx.set_font_size(16.0);
            ctx.translate(10.0, 4.0);
            ctx.fill_text("Hi", 0.0, 12.0);
        }

        let mut buf_b = LcdBuffer::new(w, h);
        {
            let mut ctx = LcdGfxCtx::new(&mut buf_b);
            ctx.clear(Color::white());
            ctx.set_fill_color(Color::black());
            ctx.set_font(f);
            ctx.set_font_size(16.0);
            ctx.fill_text("Hi", 10.0, 16.0);
        }

        assert_eq!(buf_a.color_plane(), buf_b.color_plane(),
            "translate(10,4) + fill_text(0,12) must equal fill_text(10,16)");
    }
}
