use super::*;

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
pub(super) fn active_fb<'a>(
    base_fb: &'a mut Framebuffer,
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
pub(super) fn composite_framebuffers(
    dst: &mut Framebuffer,
    src: &Framebuffer,
    dest_x: i32,
    dest_y: i32,
    alpha: f64,
) {
    let src_w = src.width() as i32;
    let src_h = src.height() as i32;
    let dst_w = dst.width() as i32;
    let dst_h = dst.height() as i32;

    let src_px = src.pixels();
    let dst_px = dst.pixels_mut();

    for sy in 0..src_h {
        let dy = dest_y + sy;
        if dy < 0 || dy >= dst_h {
            continue;
        }
        for sx in 0..src_w {
            let dx = dest_x + sx;
            if dx < 0 || dx >= dst_w {
                continue;
            }
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((dy * dst_w + dx) * 4) as usize;
            let layer_alpha = alpha.clamp(0.0, 1.0) as f32;
            let sa = (src_px[si + 3] as f32 / 255.0) * layer_alpha;
            if sa < 1e-4 {
                continue;
            } // fully transparent source — skip
            let inv_sa = 1.0 - sa;
            // Premultiplied SrcOver — same formula for all four channels.
            for k in 0..4 {
                let s = src_px[si + k] as f32 * layer_alpha;
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
    fill_rule: FillRule,
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
    ras.filling_rule(to_agg_fill_rule(fill_rule));
    let mut sl = ScanlineU8::new();
    let mut curves = ConvCurve::new(path);
    let mut transformed = ConvTransform::new(&mut curves, transform.clone());
    ras.add_path(&mut transformed, 0);
    render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, color);
}

fn to_agg_fill_rule(rule: FillRule) -> FillingRule {
    match rule {
        FillRule::NonZero => FillingRule::NonZero,
        FillRule::EvenOdd => FillingRule::EvenOdd,
    }
}

pub(crate) fn rasterize_stroke(
    fb: &mut Framebuffer,
    path: &mut PathStorage,
    color: &agg_rust::color::Rgba8,
    width: f64,
    join: LineJoin,
    cap: LineCap,
    miter_limit: f64,
    dashes: &[f64],
    dash_offset: f64,
    mode: CompOp,
    clip: Option<(f64, f64, f64, f64)>,
    transform: &TransAffine,
) {
    let mut curves = ConvCurve::new(path);
    if dashes.is_empty() {
        rasterize_stroke_source(
            fb,
            &mut curves,
            color,
            width,
            join,
            cap,
            miter_limit,
            mode,
            clip,
            transform,
        );
    } else {
        let mut dash = ConvDash::new(&mut curves);
        configure_dashes(&mut dash, dashes, dash_offset);
        rasterize_stroke_source(
            fb,
            dash,
            color,
            width,
            join,
            cap,
            miter_limit,
            mode,
            clip,
            transform,
        );
    }
}

fn rasterize_stroke_source<VS: VertexSource>(
    fb: &mut Framebuffer,
    source: VS,
    color: &agg_rust::color::Rgba8,
    width: f64,
    join: LineJoin,
    cap: LineCap,
    miter_limit: f64,
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
    let mut stroke = ConvStroke::new(source);
    stroke.set_width(width);
    stroke.set_line_join(join);
    stroke.set_line_cap(cap);
    stroke.set_miter_limit(miter_limit);
    let mut transformed = ConvTransform::new(&mut stroke, transform.clone());
    ras.add_path(&mut transformed, 0);
    render_scanlines_aa_solid(&mut ras, &mut sl, &mut rb, color);
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

// ---------------------------------------------------------------------------
// DrawCtx blanket impl for GfxCtx
// ---------------------------------------------------------------------------

impl crate::draw_ctx::DrawCtx for GfxCtx<'_> {
    fn set_fill_color(&mut self, c: crate::color::Color) {
        self.set_fill_color(c)
    }
    fn set_fill_linear_gradient(&mut self, gradient: crate::draw_ctx::LinearGradientPaint) {
        self.set_fill_linear_gradient(gradient)
    }
    fn set_fill_radial_gradient(&mut self, gradient: crate::draw_ctx::RadialGradientPaint) {
        self.set_fill_radial_gradient(gradient)
    }
    fn set_fill_pattern(&mut self, pattern: crate::draw_ctx::PatternPaint) {
        self.set_fill_pattern(pattern)
    }
    fn supports_fill_linear_gradient(&self) -> bool {
        true
    }
    fn supports_fill_radial_gradient(&self) -> bool {
        true
    }
    fn supports_fill_pattern(&self) -> bool {
        true
    }
    fn set_stroke_color(&mut self, c: crate::color::Color) {
        self.set_stroke_color(c)
    }
    fn set_stroke_linear_gradient(&mut self, gradient: crate::draw_ctx::LinearGradientPaint) {
        self.set_stroke_linear_gradient(gradient)
    }
    fn set_stroke_radial_gradient(&mut self, gradient: crate::draw_ctx::RadialGradientPaint) {
        self.set_stroke_radial_gradient(gradient)
    }
    fn set_stroke_pattern(&mut self, pattern: crate::draw_ctx::PatternPaint) {
        self.set_stroke_pattern(pattern)
    }
    fn supports_stroke_linear_gradient(&self) -> bool {
        true
    }
    fn supports_stroke_radial_gradient(&self) -> bool {
        true
    }
    fn supports_stroke_pattern(&self) -> bool {
        true
    }
    fn set_line_width(&mut self, w: f64) {
        self.set_line_width(w)
    }
    fn set_line_join(&mut self, j: agg_rust::math_stroke::LineJoin) {
        self.set_line_join(j)
    }
    fn set_line_cap(&mut self, c: agg_rust::math_stroke::LineCap) {
        self.set_line_cap(c)
    }
    fn set_miter_limit(&mut self, limit: f64) {
        self.set_miter_limit(limit)
    }
    fn set_line_dash(&mut self, dashes: &[f64], offset: f64) {
        self.set_line_dash(dashes, offset)
    }
    fn set_fill_rule(&mut self, rule: crate::draw_ctx::FillRule) {
        self.set_fill_rule(rule)
    }
    fn set_blend_mode(&mut self, m: agg_rust::comp_op::CompOp) {
        self.set_blend_mode(m)
    }
    fn set_global_alpha(&mut self, a: f64) {
        self.set_global_alpha(a)
    }
    fn set_font(&mut self, f: Arc<crate::text::Font>) {
        self.set_font(f)
    }
    fn set_font_size(&mut self, s: f64) {
        self.set_font_size(s)
    }
    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.clip_rect(x, y, w, h)
    }
    fn reset_clip(&mut self) {
        self.reset_clip()
    }
    fn clear(&mut self, c: crate::color::Color) {
        self.clear(c)
    }
    fn begin_path(&mut self) {
        self.begin_path()
    }
    fn move_to(&mut self, x: f64, y: f64) {
        self.move_to(x, y)
    }
    fn line_to(&mut self, x: f64, y: f64) {
        self.line_to(x, y)
    }
    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        self.cubic_to(cx1, cy1, cx2, cy2, x, y)
    }
    fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.quad_to(cx, cy, x, y)
    }
    fn arc_to(&mut self, cx: f64, cy: f64, r: f64, a1: f64, a2: f64, ccw: bool) {
        self.arc_to(cx, cy, r, a1, a2, ccw)
    }
    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        self.circle(cx, cy, r)
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.rect(x, y, w, h)
    }
    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.rounded_rect(x, y, w, h, r)
    }
    fn close_path(&mut self) {
        self.close_path()
    }
    fn fill(&mut self) {
        self.fill()
    }
    fn stroke(&mut self) {
        self.stroke()
    }
    fn fill_and_stroke(&mut self) {
        self.fill_and_stroke()
    }

    fn draw_triangles_aa(
        &mut self,
        vertices: &[[f32; 3]],
        indices: &[u32],
        color: crate::color::Color,
    ) {
        // Software fallback: rasterise each triangle as a solid filled
        // polygon.  The per-vertex `alpha` is ignored (software already has
        // analytic AA via the scanline rasteriser), so halo quads from the
        // GPU pipeline end up as redundant thin slivers — visually harmless
        // but inefficient.  Callers that care should check `has_image_blit`
        // / a similar capability flag; for now this keeps parity with the
        // trait so the Lion demo renders correctly on the CPU path too.
        let saved_fill = self.state.fill_color;
        self.set_fill_color(color);
        let n_tris = indices.len() / 3;
        for t in 0..n_tris {
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
        self.set_fill_color(saved_fill);
    }
    fn fill_text(&mut self, t: &str, x: f64, y: f64) {
        self.fill_text(t, x, y)
    }
    fn fill_text_gsv(&mut self, t: &str, x: f64, y: f64, s: f64) {
        self.fill_text_gsv(t, x, y, s)
    }
    fn measure_text(&self, t: &str) -> Option<crate::text::TextMetrics> {
        self.measure_text(t)
    }
    fn transform(&self) -> agg_rust::trans_affine::TransAffine {
        self.transform()
    }
    fn root_transform(&self) -> agg_rust::trans_affine::TransAffine {
        let mut t = self.transform();
        for layer in self.layer_stack.iter().rev() {
            t.premultiply(&agg_rust::trans_affine::TransAffine::new_translation(
                layer.origin_x,
                layer.origin_y,
            ));
        }
        t
    }
    fn save(&mut self) {
        self.save()
    }
    fn restore(&mut self) {
        self.restore()
    }
    fn translate(&mut self, tx: f64, ty: f64) {
        self.translate(tx, ty)
    }
    fn rotate(&mut self, r: f64) {
        self.rotate(r)
    }
    fn scale(&mut self, sx: f64, sy: f64) {
        self.scale(sx, sy)
    }
    fn set_transform(&mut self, m: agg_rust::trans_affine::TransAffine) {
        self.set_transform(m)
    }
    fn reset_transform(&mut self) {
        self.reset_transform()
    }
    fn push_layer(&mut self, w: f64, h: f64) {
        self.push_layer(w, h)
    }
    fn supports_compositing_layers(&self) -> bool {
        true
    }
    fn push_layer_with_alpha(&mut self, w: f64, h: f64, alpha: f64) {
        self.push_layer_with_alpha(w, h, alpha)
    }
    fn pop_layer(&mut self) {
        self.pop_layer()
    }

    fn has_image_blit(&self) -> bool {
        true
    }

    fn draw_image_rgba_arc(
        &mut self,
        data: &Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        // Software backend has no GPU texture cache; the CPU composite path
        // is the same as the slice entry point.
        self.draw_image_rgba(data.as_slice(), img_w, img_h, dst_x, dst_y, dst_w, dst_h);
    }

    fn draw_lcd_backbuffer_arc(
        &mut self,
        color: &Arc<Vec<u8>>,
        alpha: &Arc<Vec<u8>>,
        w: u32,
        h: u32,
        dst_x: f64,
        dst_y: f64,
        _dst_w: f64,
        _dst_h: f64,
    ) {
        // Per-channel premultiplied src-over directly onto the active
        // framebuffer.  Preserves LCD chroma: each subpixel's alpha
        // drives the src-over of that subpixel's colour into the
        // destination independently of the other two.
        //
        // Inputs are **top-row-first** (matches the cache layout); the
        // destination `Framebuffer` is Y-up with row 0 at the bottom, so
        // src row `sy` maps to dst row `origin_y + (h-1-sy)`.
        if w == 0 || h == 0 {
            return;
        }
        let w_u = w as usize;
        let h_u = h as usize;
        if color.len() < w_u * h_u * 3 || alpha.len() < w_u * h_u * 3 {
            return;
        }

        let t = &self.state.transform;
        let sx = (dst_x * t.sx + dst_y * t.shx + t.tx).round() as i32;
        let sy = (dst_x * t.shy + dst_y * t.sy + t.ty).round() as i32;
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        let fw = fb.width() as i32;
        let fh = fb.height() as i32;
        let fw_u = fw as usize;
        let pixels = fb.pixels_mut();

        for src_y in 0..h_u {
            // Top-row-first src → Y-up dst: src row 0 (visually top)
            // lands at dst_y + h - 1 (the visually-top dst row).
            let dy = sy + (h_u - 1 - src_y) as i32;
            if dy < 0 || dy >= fh {
                continue;
            }
            let dy_u = dy as usize;
            for src_x in 0..w_u {
                let dx = sx + src_x as i32;
                if dx < 0 || dx >= fw {
                    continue;
                }
                let ci = (src_y * w_u + src_x) * 3;

                let sa_r = alpha[ci] as f32 / 255.0;
                let sa_g = alpha[ci + 1] as f32 / 255.0;
                let sa_b = alpha[ci + 2] as f32 / 255.0;
                if sa_r == 0.0 && sa_g == 0.0 && sa_b == 0.0 {
                    continue;
                }

                let sc_r = color[ci] as f32 / 255.0;
                let sc_g = color[ci + 1] as f32 / 255.0;
                let sc_b = color[ci + 2] as f32 / 255.0;

                let di = (dy_u * fw_u + dx as usize) * 4;
                // Framebuffer holds premultiplied RGBA.  Per-channel
                // src-over is `dst = src + dst * (1 - src_a)` since src
                // is already premultiplied.  Alpha composites via
                // max-channel-alpha so the destination picks up full
                // opacity wherever any subpixel was painted — matches
                // "this pixel was drawn on" for subsequent SrcOver blits.
                let dc_r = pixels[di] as f32 / 255.0;
                let dc_g = pixels[di + 1] as f32 / 255.0;
                let dc_b = pixels[di + 2] as f32 / 255.0;
                let da = pixels[di + 3] as f32 / 255.0;

                let rc_r = sc_r + dc_r * (1.0 - sa_r);
                let rc_g = sc_g + dc_g * (1.0 - sa_g);
                let rc_b = sc_b + dc_b * (1.0 - sa_b);
                let src_a_max = sa_r.max(sa_g).max(sa_b);
                let ra = src_a_max + da * (1.0 - src_a_max);

                pixels[di] = (rc_r * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                pixels[di + 1] = (rc_g * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                pixels[di + 2] = (rc_b * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                pixels[di + 3] = (ra * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }

    fn has_lcd_mask_composite(&self) -> bool {
        true
    }

    fn draw_lcd_mask(
        &mut self,
        mask: &[u8],
        mask_w: u32,
        mask_h: u32,
        src_color: Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        // Resolve to the active target (base fb or topmost layer) with
        // the current CTM applied to the placement origin.  Both the
        // mask and the Framebuffer are Y-up (row 0 = bottom), so mask
        // row `my` maps directly to dst row `sy + my`.
        if mask.len() < (mask_w as usize) * (mask_h as usize) * 3 {
            return;
        }
        let t = &self.state.transform;
        let sx = dst_x * t.sx + dst_y * t.shx + t.tx;
        let sy = dst_x * t.shy + dst_y * t.sy + t.ty;
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        let fw = fb.width();
        let fh = fb.height();
        let origin_x = sx.round() as i32;
        let origin_y = sy.round() as i32;

        let sa = src_color.a.clamp(0.0, 1.0);
        let sr = src_color.r.clamp(0.0, 1.0);
        let sg = src_color.g.clamp(0.0, 1.0);
        let sb = src_color.b.clamp(0.0, 1.0);
        let fw_i = fw as i32;
        let fh_i = fh as i32;
        let mw_i = mask_w as i32;
        let mh_i = mask_h as i32;
        let pixels = fb.pixels_mut();

        for my in 0..mh_i {
            // Mask row `my` (Y-up: 0 = bottom) → dst row `origin_y + my`
            // in the Y-up framebuffer.  No flip.
            let dy = origin_y + my;
            if dy < 0 || dy >= fh_i {
                continue;
            }
            for mx in 0..mw_i {
                let dx = origin_x + mx;
                if dx < 0 || dx >= fw_i {
                    continue;
                }
                let mi = ((my * mw_i + mx) * 3) as usize;
                // Per-channel coverage × src alpha — partial-alpha src
                // (e.g. `text_dim` placeholder colour) fades proportionally.
                let cr = (mask[mi] as f32 / 255.0) * sa;
                let cg = (mask[mi + 1] as f32 / 255.0) * sa;
                let cb = (mask[mi + 2] as f32 / 255.0) * sa;
                if cr == 0.0 && cg == 0.0 && cb == 0.0 {
                    continue;
                }
                let di = ((dy * fw_i + dx) * 4) as usize;
                let dr = pixels[di] as f32 / 255.0;
                let dg = pixels[di + 1] as f32 / 255.0;
                let db = pixels[di + 2] as f32 / 255.0;
                let rr = sr * cr + dr * (1.0 - cr);
                let rg = sg * cg + dg * (1.0 - cg);
                let rbb = sb * cb + db * (1.0 - cb);
                pixels[di] = (rr * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                pixels[di + 1] = (rg * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                pixels[di + 2] = (rbb * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                // Alpha unchanged — we're writing onto an existing opaque
                // (or semi-transparent) surface without introducing new
                // transparency.
            }
        }
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
        // Scale the source image into a temporary Framebuffer at dst size,
        // then composite it onto the current render target using the CTM origin.
        if img_w == 0 || img_h == 0 || dst_w < 1.0 || dst_h < 1.0 {
            return;
        }

        let out_w = dst_w.round() as u32;
        let out_h = dst_h.round() as u32;
        let mut scaled = crate::framebuffer::Framebuffer::new(out_w, out_h);

        // Nearest-neighbour scale — sufficient for README screenshots / badges.
        // `data` is straight-alpha by the `draw_image_rgba` convention; AGG
        // framebuffers store **premultiplied** RGBA, so we premultiply each
        // sampled pixel on the way in so `composite_framebuffers` (which uses
        // premultiplied SrcOver) blends with correct intensity.
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
                    let a = data[si + 3] as u32;
                    if a == 255 {
                        px[di] = data[si];
                        px[di + 1] = data[si + 1];
                        px[di + 2] = data[si + 2];
                        px[di + 3] = 255;
                    } else {
                        // Premultiply: (c * a + 127) / 255 (round-half-up).
                        px[di] = (((data[si] as u32) * a + 127) / 255) as u8;
                        px[di + 1] = (((data[si + 1] as u32) * a + 127) / 255) as u8;
                        px[di + 2] = (((data[si + 2] as u32) * a + 127) / 255) as u8;
                        px[di + 3] = a as u8;
                    }
                }
            }
        }

        // Apply CTM translation to get screen-space origin.
        let (tx, ty) = {
            let t = self.transform();
            (t.tx, t.ty)
        };
        let screen_x = (tx + dst_x).round() as i32;
        let screen_y = (ty + dst_y).round() as i32;
        let fb = active_fb(&mut self.base_fb, &mut self.layer_stack);
        composite_framebuffers(fb, &scaled, screen_x, screen_y, 1.0);
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
