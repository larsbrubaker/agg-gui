//! `impl DrawCtx for WgpuGfxCtx` — the full drawing interface.
//!
//! State setters and path-building methods are fully implemented here; they
//! work from Phase 1 onwards.  Rendering methods (`fill`, `stroke`, `fill_text`,
//! etc.) push [`DrawCommand`] entries into `self.commands` and are flushed by
//! [`WgpuGfxCtx::end_frame`] (implemented in Phase 4).

use super::*;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::{measure_text_metrics, TextMetrics};
use agg_gui::CompOp;
use agg_rust::arc::Arc as AggArc;
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::rounded_rect::RoundedRect;

impl DrawCtx for WgpuGfxCtx {
    // ── State ─────────────────────────────────────────────────────────────────

    fn set_fill_color(&mut self, c: Color) {
        self.fill_color = c;
        self.fill_linear_gradient = None;
        self.fill_radial_gradient = None;
    }

    fn set_fill_linear_gradient(&mut self, gradient: LinearGradientPaint) {
        self.fill_linear_gradient = Some(gradient);
        self.fill_radial_gradient = None;
    }

    fn supports_fill_linear_gradient(&self) -> bool {
        true
    }

    fn set_fill_radial_gradient(&mut self, gradient: RadialGradientPaint) {
        self.fill_linear_gradient = None;
        self.fill_radial_gradient = Some(gradient);
    }

    fn supports_fill_radial_gradient(&self) -> bool {
        true
    }

    fn set_stroke_color(&mut self, c: Color) {
        self.stroke_color = c;
        self.stroke_linear_gradient = None;
        self.stroke_radial_gradient = None;
    }

    fn set_stroke_linear_gradient(&mut self, gradient: LinearGradientPaint) {
        self.stroke_linear_gradient = Some(gradient);
        self.stroke_radial_gradient = None;
    }

    fn supports_stroke_linear_gradient(&self) -> bool {
        true
    }

    fn set_stroke_radial_gradient(&mut self, gradient: RadialGradientPaint) {
        self.stroke_linear_gradient = None;
        self.stroke_radial_gradient = Some(gradient);
    }

    fn supports_stroke_radial_gradient(&self) -> bool {
        true
    }

    fn set_line_width(&mut self, w: f64) {
        self.line_width = w;
    }

    fn set_line_join(&mut self, j: LineJoin) {
        self.line_join = j;
    }

    fn set_line_cap(&mut self, c: LineCap) {
        self.line_cap = c;
    }

    fn set_miter_limit(&mut self, limit: f64) {
        self.miter_limit = limit.max(1.0);
    }

    fn set_line_dash(&mut self, dashes: &[f64], offset: f64) {
        self.line_dash.clear();
        self.line_dash
            .extend(dashes.iter().copied().filter(|v| *v > 0.0));
        self.dash_offset = offset;
    }

    fn set_blend_mode(&mut self, _mode: CompOp) {
        // wgpu blend state is baked into pipeline objects at creation time.
        // Dynamic blend-mode changes are not supported in the initial port.
    }

    fn set_global_alpha(&mut self, a: f64) {
        self.global_alpha = a;
    }

    fn set_fill_rule(&mut self, rule: agg_gui::draw_ctx::FillRule) {
        self.fill_rule = rule;
    }

    // ── Font ──────────────────────────────────────────────────────────────────

    fn set_font(&mut self, font: std::sync::Arc<Font>) {
        self.font = Some(font);
    }

    fn set_font_size(&mut self, size: f64) {
        self.font_size = size;
    }

    // ── Clipping ──────────────────────────────────────────────────────────────

    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        // Transform clip corners through the CTM to screen space.
        let (mut x0, mut y0) = (x, y);
        let (mut x1, mut y1) = (x + w, y + h);
        self.ctm().transform(&mut x0, &mut y0);
        self.ctm().transform(&mut x1, &mut y1);
        let (lx, rx) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
        let (by, ty) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
        let [nx, ny, nw, nh] = Self::compute_scissor(lx, by, rx, ty);

        // Intersect with the existing scissor so parent clips constrain children.
        let [ix, iy, iw, ih] = if let Some([ex, ey, ew, eh]) = self.current_clip() {
            let nx2 = nx.saturating_add(nw).min(ex.saturating_add(ew));
            let ny2 = ny.saturating_add(nh).min(ey.saturating_add(eh));
            let rx2 = nx.max(ex);
            let ry2 = ny.max(ey);
            [rx2, ry2, nx2.saturating_sub(rx2).max(0), ny2.saturating_sub(ry2).max(0)]
        } else {
            [nx, ny, nw, nh]
        };

        self.state_stack.last_mut().unwrap().1 = Some([ix, iy, iw, ih]);
    }

    fn reset_clip(&mut self) {
        self.state_stack.last_mut().unwrap().1 = None;
    }

    // ── Clear ─────────────────────────────────────────────────────────────────

    fn clear(&mut self, color: Color) {
        self.commands.push(DrawCommand::Clear(color));
    }

    // ── Path building ─────────────────────────────────────────────────────────

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
        self.arc_to(cx, cy, r, 0.0, std::f64::consts::TAU, true);
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

    // ── Path drawing ──────────────────────────────────────────────────────────

    fn fill(&mut self) {
        self.do_fill();
    }

    fn stroke(&mut self) {
        self.do_stroke();
    }

    fn fill_and_stroke(&mut self) {
        // Re-use the path for both operations; path is not consumed by tessellation.
        self.do_fill();
        self.do_stroke();
    }

    fn draw_triangles_aa(&mut self, vertices: &[[f32; 3]], indices: &[u32], color: Color) {
        if vertices.is_empty() || indices.is_empty() {
            return;
        }
        // Apply the current CTM to each vertex's XY; alpha passes through.
        let ctm = *self.ctm();
        let transformed: Vec<[f32; 3]> = vertices
            .iter()
            .map(|v| {
                let (mut x, mut y) = (v[0] as f64, v[1] as f64);
                ctm.transform(&mut x, &mut y);
                [x as f32, y as f32, v[2]]
            })
            .collect();
        self.commands.push(DrawCommand::AaSolid {
            verts: transformed,
            indices: indices.to_vec(),
            color,
            global_alpha: self.global_alpha as f32,
            clip: self.current_clip(),
        });
    }

    // ── Text ──────────────────────────────────────────────────────────────────

    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.fill_text_impl(text, x, y);
    }

    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {
        // GSV (Glyph-Stroke-Vector) font is AGG-specific; not available in the
        // wgpu path.  Silently ignore — used only in placeholder widgets.
    }

    fn measure_text(&self, text: &str) -> Option<TextMetrics> {
        let font = self.font.as_ref()?;
        Some(measure_text_metrics(font, text, self.font_size))
    }

    // ── Image blitting ────────────────────────────────────────────────────────

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
        self.draw_image_rgba_slice_impl(data, img_w, img_h, dst_x, dst_y, dst_w, dst_h);
    }

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
        self.draw_image_rgba_arc_impl(data, img_w, img_h, dst_x, dst_y, dst_w, dst_h);
    }

    fn has_lcd_mask_composite(&self) -> bool {
        true
    }

    fn draw_lcd_mask(
        &mut self,
        mask: &[u8],
        mask_w: u32,
        mask_h: u32,
        src_color: agg_gui::Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        self.draw_lcd_mask_slice_impl(mask, mask_w, mask_h, src_color, dst_x, dst_y);
    }

    fn draw_lcd_mask_arc(
        &mut self,
        mask: &std::sync::Arc<Vec<u8>>,
        mask_w: u32,
        mask_h: u32,
        src_color: agg_gui::Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        self.draw_lcd_mask_arc_impl(mask, mask_w, mask_h, src_color, dst_x, dst_y);
    }

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
        self.draw_lcd_backbuffer_arc_impl(color, alpha, w, h, dst_x, dst_y, dst_w, dst_h);
    }

    // ── Transform ─────────────────────────────────────────────────────────────

    fn transform(&self) -> TransAffine {
        *self.ctm()
    }

    fn root_transform(&self) -> TransAffine {
        let mut t = *self.ctm();
        for layer in self.layer_stack.iter().rev() {
            t.premultiply(&TransAffine::new_translation(
                layer.origin_x,
                layer.origin_y,
            ));
        }
        t
    }

    fn save(&mut self) {
        let top = *self.state_stack.last().unwrap();
        self.state_stack.push(top);
    }

    fn restore(&mut self) {
        if self.state_stack.len() > 1 {
            self.state_stack.pop();
            // Scissor is deferred; no GPU state to restore immediately.
            self.apply_scissor();
        }
    }

    fn translate(&mut self, tx: f64, ty: f64) {
        self.ctm_mut()
            .premultiply(&TransAffine::new_translation(tx, ty));
    }

    fn rotate(&mut self, radians: f64) {
        self.ctm_mut()
            .premultiply(&TransAffine::new_rotation(radians));
    }

    fn scale(&mut self, sx: f64, sy: f64) {
        self.ctm_mut()
            .premultiply(&TransAffine::new_scaling(sx, sy));
    }

    fn set_transform(&mut self, m: TransAffine) {
        *self.ctm_mut() = m;
    }

    fn reset_transform(&mut self) {
        *self.ctm_mut() = TransAffine::new();
    }

    // ── Compositing layers ────────────────────────────────────────────────────

    fn supports_compositing_layers(&self) -> bool {
        true
    }

    fn supports_retained_layers(&self) -> bool {
        true
    }

    fn push_layer(&mut self, width: f64, height: f64) {
        self.push_layer_with_alpha_impl(width, height, 1.0, None);
    }

    fn push_layer_with_alpha(&mut self, width: f64, height: f64, alpha: f64) {
        self.push_layer_with_alpha_impl(width, height, alpha, None);
    }

    fn pop_layer(&mut self) {
        self.pop_layer_impl();
    }

    fn set_layer_rounded_clip(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.set_layer_rounded_clip_impl(x, y, w, h, r);
    }

    fn composite_retained_layer(
        &mut self,
        key: u64,
        width: f64,
        height: f64,
        alpha: f64,
    ) -> bool {
        self.composite_retained_layer_impl(key, width, height, alpha)
    }

    fn push_retained_layer_with_alpha(&mut self, key: u64, width: f64, height: f64, alpha: f64) {
        self.push_layer_with_alpha_impl(width, height, alpha, Some(key));
    }

    // ── GL / GPU content ──────────────────────────────────────────────────────

    fn gl_paint(&mut self, screen_rect: agg_gui::Rect, painter: &mut dyn agg_gui::GlPaint) {
        // Flush any 2-D commands accumulated up to this point so the painter
        // overlays on the correct backdrop.  Mid-frame layer state isn't
        // supported by this path yet — the wgpu cube widget is always at the
        // top level on the surface in the current demo.
        let Some(view) = self.surface_view.clone() else {
            return;
        };
        if !self.commands.is_empty() {
            self.flush_to_surface(&view);
        }

        let target_size = (self.viewport.0 as u32, self.viewport.1 as u32);
        let pctx = crate::WgpuPaintContext {
            device: std::sync::Arc::clone(&self.device),
            queue: std::sync::Arc::clone(&self.queue),
            target_view: view,
            surface_format: self.surface_format,
            target_size,
        };
        let parent_clip = self.current_clip();
        painter.gl_paint(
            &pctx as &dyn std::any::Any,
            screen_rect,
            target_size.0 as i32,
            target_size.1 as i32,
            parent_clip,
        );
    }
}
