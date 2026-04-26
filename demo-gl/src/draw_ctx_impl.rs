use super::*;
use crate::gl_support::{compute_gl_scissor, texture_key};

// ---------------------------------------------------------------------------
// DrawCtx impl
// ---------------------------------------------------------------------------

impl DrawCtx for GlGfxCtx {
    // ── State ────────────────────────────────────────────────────────────────

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
    fn set_blend_mode(&mut self, _: CompOp) {}
    fn set_global_alpha(&mut self, a: f64) {
        self.global_alpha = a;
    }
    fn set_fill_rule(&mut self, rule: FillRule) {
        self.fill_rule = rule;
    }

    // ── Font ─────────────────────────────────────────────────────────────────

    fn set_font(&mut self, font: Arc<Font>) {
        self.font = Some(font);
    }
    fn set_font_size(&mut self, size: f64) {
        self.font_size = size;
    }

    // ── Clipping ─────────────────────────────────────────────────────────────

    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        // Transform the clip rect corners through the CTM to screen space.
        let (mut x0, mut y0) = (x, y);
        let (mut x1, mut y1) = (x + w, y + h);
        self.ctm().transform(&mut x0, &mut y0);
        self.ctm().transform(&mut x1, &mut y1);
        let (lx, rx) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
        let (by, ty2) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
        let [gl_x, gl_y, gl_w, gl_h] = compute_gl_scissor(lx, by, rx, ty2);

        // Intersect with the existing scissor so parent clips constrain children.
        // (Replacing outright lets children escape their parent's clip region.)
        let [ix, iy, iw, ih] = if let Some([ex, ey, ew, eh]) = self.current_clip() {
            let nx1 = gl_x.max(ex);
            let ny1 = gl_y.max(ey);
            let nx2 = gl_x.saturating_add(gl_w).min(ex.saturating_add(ew));
            let ny2 = gl_y.saturating_add(gl_h).min(ey.saturating_add(eh));
            [
                nx1,
                ny1,
                nx2.saturating_sub(nx1).max(0),
                ny2.saturating_sub(ny1).max(0),
            ]
        } else {
            [gl_x, gl_y, gl_w, gl_h]
        };

        self.state_stack.last_mut().unwrap().1 = Some([ix, iy, iw, ih]);
        unsafe {
            self.gl.enable(glow::SCISSOR_TEST);
            self.gl.scissor(ix, iy, iw, ih);
        }
    }

    fn reset_clip(&mut self) {
        self.state_stack.last_mut().unwrap().1 = None;
        unsafe {
            self.gl.disable(glow::SCISSOR_TEST);
        }
    }

    // ── Clear ─────────────────────────────────────────────────────────────────

    fn clear(&mut self, color: Color) {
        unsafe {
            // Color fields are already [0, 1] f32 — no conversion needed.
            self.gl.clear_color(color.r, color.g, color.b, color.a);
            self.gl
                .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    // ── Path building ────────────────────────────────────────────────────────

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

    // ── Path drawing ─────────────────────────────────────────────────────────

    fn fill(&mut self) {
        unsafe {
            self.do_fill();
        }
    }

    fn stroke(&mut self) {
        unsafe {
            self.do_stroke();
        }
    }

    fn fill_and_stroke(&mut self) {
        unsafe {
            self.do_fill();
        }
        unsafe {
            self.do_stroke();
        }
    }

    fn draw_triangles_aa(&mut self, vertices: &[[f32; 3]], indices: &[u32], color: Color) {
        // The Lion demo and other callers tessellate once at load time and
        // submit the cached triangles + halo every frame — route straight
        // into the existing AA-solid GL pipeline.  Apply the current CTM
        // to each vertex's XY; alpha passes through unchanged.
        if vertices.is_empty() || indices.is_empty() {
            return;
        }
        let ctm = *self.ctm();
        let transformed: Vec<[f32; 3]> = vertices
            .iter()
            .map(|v| {
                let (mut x, mut y) = (v[0] as f64, v[1] as f64);
                ctm.transform(&mut x, &mut y);
                [x as f32, y as f32, v[2]]
            })
            .collect();
        unsafe {
            self.submit_aa_triangles(&transformed, indices, color);
        }
    }

    // ── Text ─────────────────────────────────────────────────────────────────

    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.fill_text_impl(text, x, y);
    }

    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {
        // GSV (Glyph-Stroke-Vector) font is AGG-specific; not available in GL path.
        // Silently ignore — this is only used in placeholder widgets.
    }

    // ── Image blitting (textured quad) ───────────────────────────────────────
    //
    // `has_image_blit()` returns `true` now that `draw_image_rgba` has an
    // internal texture cache keyed by (ptr, len, head/tail bytes).  `Label`'s
    // backbuffer path activates — text is rasterised once via AGG, uploaded
    // as a GL texture, and blitted every subsequent frame.  Cache evicts LRU
    // at `TEX_CACHE_MAX` entries.  Widgets that rebuild their Label every
    // layout (e.g. inspector `TreeRow`) pay one re-raster + re-upload per
    // layout — acceptable since those labels remain small and few.
    fn has_image_blit(&self) -> bool {
        true
    }

    #[cfg(target_arch = "wasm32")]
    fn has_lcd_mask_composite(&self) -> bool {
        // WebGL 2 base spec lacks dual-source blending, so we can't do
        // per-channel src-over on the GPU — the `LCD_FRAG`/`LCB_FRAG`
        // WASM-path fallback shaders collapse the three coverage
        // channels to a single average/max and composite via the
        // standard `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` blend.  That's
        // grayscale-equivalent output (no subpixel chroma) but it
        // still routes text through **AGG-rasterised masks** instead
        // of the tessellated-glyph path — net visibly sharper text
        // on WebGL, which is what enabling the LCD pipeline at all
        // gets us here.
        //
        // True subpixel chroma on WebGL 2 requires the
        // `WEBGL_blend_func_extended` extension for dual-source
        // blending — future work: query the extension at ctx init,
        // store a flag, report it here, and branch shader selection +
        // blend-func setup on that flag.  Every modern browser
        // (Chrome, Firefox, Safari) ships the extension, so it's just
        // plumbing.
        true
    }
    #[cfg(not(target_arch = "wasm32"))]
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
        // Slice path — upload a throwaway texture.  Used only by code
        // that doesn't have an `Arc` to key a cache on.  Label's hot
        // path goes through `draw_lcd_mask_arc` below, which reuses
        // the uploaded GL texture across frames.
        if mask.is_empty() || mask_w == 0 || mask_h == 0 {
            return;
        }
        if mask.len() < (mask_w as usize) * (mask_h as usize) * 3 {
            return;
        }
        unsafe {
            let tex = self.gl.create_texture().expect("create LCD texture");
            self.upload_lcd_texture(tex, mask_w, mask_h, mask);
            self.draw_lcd_quad(tex, mask_w, mask_h, src_color, dst_x, dst_y);
            self.gl.delete_texture(tex);
        }
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
        if mask.is_empty() || mask_w == 0 || mask_h == 0 {
            return;
        }
        if mask.len() < (mask_w as usize) * (mask_h as usize) * 3 {
            return;
        }
        let key = std::sync::Arc::as_ptr(mask) as usize;

        // Sweep expired entries opportunistically — each miss walks the
        // map once; each hit bumps a counter and sweeps every Nth call.
        // Keeps GPU memory bounded when the CPU mask cache evicts.
        let tex = match self.lcd_arc_texture_cache.get(&key) {
            Some(entry) if entry.weak.upgrade().is_some() => entry.texture,
            _ => unsafe {
                let tex = self.gl.create_texture().expect("create LCD texture");
                self.upload_lcd_texture(tex, mask_w, mask_h, mask.as_slice());
                // Insert fresh entry, dropping any stale one for this key.
                if let Some(old) = self.lcd_arc_texture_cache.insert(
                    key,
                    ArcTextureEntry {
                        weak: std::sync::Arc::downgrade(mask),
                        texture: tex,
                        w: mask_w,
                        h: mask_h,
                    },
                ) {
                    self.gl.delete_texture(old.texture);
                }
                // Cheap periodic sweep of dropped Arcs.
                self.lcd_arc_texture_cache.retain(|_, e| {
                    if e.weak.upgrade().is_some() {
                        true
                    } else {
                        self.gl.delete_texture(e.texture);
                        false
                    }
                });
                tex
            },
        };
        unsafe {
            self.draw_lcd_quad(tex, mask_w, mask_h, src_color, dst_x, dst_y);
        }
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
        if w == 0 || h == 0 || color.is_empty() || alpha.is_empty() {
            return;
        }
        let needed = (w as usize) * (h as usize) * 3;
        if color.len() < needed || alpha.len() < needed {
            return;
        }

        // Get-or-upload each plane independently; both share the
        // lcd_arc_texture_cache so GPU memory is released as soon as the
        // owning widget's `Arc` is dropped.  Colour and alpha planes have
        // distinct pointer identities → distinct cache entries, no
        // collisions.
        let color_tex = unsafe { self.lcd_plane_get_or_upload(color, w, h) };
        let alpha_tex = unsafe { self.lcd_plane_get_or_upload(alpha, w, h) };

        unsafe {
            self.draw_lcd_backbuffer_quad(color_tex, alpha_tex, w, h, dst_x, dst_y, dst_w, dst_h);
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
        if img_w == 0 || img_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 {
            return;
        }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 {
            return;
        }

        // Honour whatever CTM the caller has set — sub-pixel positions are
        // legitimate (smooth scrolling, animation).  Callers that need
        // pixel-perfect 1:1 blits (e.g. `Label` backbuffers, the pixel-
        // alignment test) must explicitly call `ctx.snap_to_pixel()` first.
        let bl = self.transform_pt(dst_x, dst_y);
        let br = self.transform_pt(dst_x + dst_w, dst_y);
        let tr = self.transform_pt(dst_x + dst_w, dst_y + dst_h);
        let tl = self.transform_pt(dst_x, dst_y + dst_h);
        let verts: [f32; 24] = [
            bl[0], bl[1], 0.0, 1.0, br[0], br[1], 1.0, 1.0, tr[0], tr[1], 1.0, 0.0, bl[0], bl[1],
            0.0, 1.0, tr[0], tr[1], 1.0, 0.0, tl[0], tl[1], 0.0, 0.0,
        ];

        // Cache key blends pointer, length, dimensions, and the first+last
        // few bytes.  Pointer changes when `Label` rebuilds its pixel cache
        // (drops old `Vec<u8>`, allocates new), so the key naturally
        // invalidates.  Head/tail-byte hash guards against the (rare) case
        // where a new allocation lands at the freed pointer address.
        let key = texture_key(data, img_w, img_h);
        let existing = self.texture_cache.get(&key).map(|&(t, _, _)| t);

        unsafe {
            let gl = Rc::clone(&self.gl);
            let tex = match existing {
                Some(t) => {
                    // LRU touch — move key to back.
                    if let Some(pos) = self.texture_cache_order.iter().position(|&k| k == key) {
                        self.texture_cache_order.remove(pos);
                    }
                    self.texture_cache_order.push_back(key);
                    t
                }
                None => {
                    let tex = gl.create_texture().expect("create texture");
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MIN_FILTER,
                        glow::LINEAR as i32,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MAG_FILTER,
                        glow::LINEAR as i32,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_WRAP_S,
                        glow::CLAMP_TO_EDGE as i32,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_WRAP_T,
                        glow::CLAMP_TO_EDGE as i32,
                    );
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA as i32,
                        img_w as i32,
                        img_h as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        Some(data),
                    );
                    self.texture_cache.insert(key, (tex, img_w, img_h));
                    self.texture_cache_order.push_back(key);
                    // LRU evict to cap.
                    const TEX_CACHE_MAX: usize = 512;
                    while self.texture_cache.len() > TEX_CACHE_MAX {
                        if let Some(old) = self.texture_cache_order.pop_front() {
                            if let Some((old_tex, _, _)) = self.texture_cache.remove(&old) {
                                gl.delete_texture(old_tex);
                            }
                        } else {
                            break;
                        }
                    }
                    tex
                }
            };

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.use_program(Some(self.tex_prog));
            gl.uniform_2_f32(self.tex_res_loc.as_ref(), self.viewport.0, self.viewport.1);
            gl.uniform_1_i32(self.tex_sampler_loc.as_ref(), 0);
            gl.bind_vertex_array(Some(self.tex_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.tex_vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&verts),
                glow::DYNAMIC_DRAW,
            );
            gl.enable(glow::BLEND);
            // Preserve framebuffer alpha just like begin_frame(). On WebGL the
            // browser composites the canvas alpha over the page, so image
            // blits must not switch later translucent UI draws into an
            // alpha-punching blend mode.
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ZERO,
                glow::ONE,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.bind_vertex_array(None);
        }
    }

    /// Arc-keyed fast path.  `Label` backbuffers flow through here — pointer
    /// identity of the `Arc<Vec<u8>>` is stable as long as the crate-level
    /// pixel cache retains the entry, so the same `Arc` yields cache hits
    /// across frames AND across re-created `Label` instances that request the
    /// same text/font/size/colour.  Dead entries (Arc dropped → `Weak`
    /// upgrade fails) are swept and their textures batch-deleted each call.
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
        if img_w == 0 || img_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 {
            return;
        }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 {
            return;
        }

        // Honour the caller's CTM — no implicit snapping.  Callers that need
        // pixel-perfect 1:1 blits call `ctx.snap_to_pixel()` before the draw.
        let bl = self.transform_pt(dst_x, dst_y);
        let br = self.transform_pt(dst_x + dst_w, dst_y);
        let tr = self.transform_pt(dst_x + dst_w, dst_y + dst_h);
        let tl = self.transform_pt(dst_x, dst_y + dst_h);
        let verts: [f32; 24] = [
            bl[0], bl[1], 0.0, 1.0, br[0], br[1], 1.0, 1.0, tr[0], tr[1], 1.0, 0.0, bl[0], bl[1],
            0.0, 1.0, tr[0], tr[1], 1.0, 0.0, tl[0], tl[1], 0.0, 0.0,
        ];

        let key = Arc::as_ptr(data) as *const u8 as usize;

        unsafe {
            let gl = Rc::clone(&self.gl);

            // Sweep dead entries — one entry per dead weak ref.  O(n) but the
            // cache is bounded by the L1 LRU cap, and we only remove; no
            // heavy work.  Batching all GL deletes in one frame keeps GL
            // driver chatter low.
            let dead_keys: Vec<usize> = self
                .arc_texture_cache
                .iter()
                .filter(|(_, e)| e.weak.strong_count() == 0)
                .map(|(k, _)| *k)
                .collect();
            for k in dead_keys {
                if let Some(e) = self.arc_texture_cache.remove(&k) {
                    gl.delete_texture(e.texture);
                }
            }

            // Look up by pointer — also verify via Weak::upgrade to guard
            // against pointer recycling (old entry died, new Arc happened to
            // allocate at the same address).
            let existing = self
                .arc_texture_cache
                .get(&key)
                .and_then(|e| match e.weak.upgrade() {
                    Some(a) if Arc::ptr_eq(&a, data) && e.w == img_w && e.h == img_h => {
                        Some(e.texture)
                    }
                    _ => None,
                });

            let tex = match existing {
                Some(t) => t,
                None => {
                    // Evict the stale entry (if any) so we can insert fresh.
                    if let Some(old) = self.arc_texture_cache.remove(&key) {
                        gl.delete_texture(old.texture);
                    }
                    let tex = gl.create_texture().expect("create texture");
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    // NEAREST filter — the Arc-keyed path is the "pre-
                    // rasterized bitmap, blit 1:1" lane (Label backbuffers,
                    // pixel-test bitmaps).  LINEAR at integer-aligned quads
                    // *should* return exact texel values everywhere, but the
                    // native desktop GL driver implements sub-texel rounding
                    // differently from WebGL — enough to visibly fuzz 1-px
                    // alternating stripes.  NEAREST skips the filter entirely
                    // and is guaranteed exact: each screen pixel's fragment
                    // fetches one texel, no interpolation, no driver-specific
                    // rounding.  Callers who genuinely want smooth interp
                    // (scaled markdown images, screenshot zoom) should go
                    // through the `&[u8]` path which stays LINEAR.
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MIN_FILTER,
                        glow::NEAREST as i32,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_MAG_FILTER,
                        glow::NEAREST as i32,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_WRAP_S,
                        glow::CLAMP_TO_EDGE as i32,
                    );
                    gl.tex_parameter_i32(
                        glow::TEXTURE_2D,
                        glow::TEXTURE_WRAP_T,
                        glow::CLAMP_TO_EDGE as i32,
                    );
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                    gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA as i32,
                        img_w as i32,
                        img_h as i32,
                        0,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        Some(data.as_slice()),
                    );
                    self.arc_texture_cache.insert(
                        key,
                        ArcTextureEntry {
                            weak: Arc::downgrade(data),
                            texture: tex,
                            w: img_w,
                            h: img_h,
                        },
                    );
                    tex
                }
            };

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.use_program(Some(self.tex_prog));
            gl.uniform_2_f32(self.tex_res_loc.as_ref(), self.viewport.0, self.viewport.1);
            gl.uniform_1_i32(self.tex_sampler_loc.as_ref(), 0);
            gl.bind_vertex_array(Some(self.tex_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.tex_vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&verts),
                glow::DYNAMIC_DRAW,
            );
            gl.enable(glow::BLEND);
            // Keep the default framebuffer alpha opaque on WebGL; see the
            // slice blit path above for the failure mode this avoids.
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ZERO,
                glow::ONE,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.bind_vertex_array(None);
        }
    }

    fn measure_text(&self, text: &str) -> Option<TextMetrics> {
        let font = self.font.as_ref()?;
        // Delegate to the same measurement used in GfxCtx.
        Some(agg_gui::text::measure_text_metrics(
            font,
            text,
            self.font_size,
        ))
    }

    // ── Transform ────────────────────────────────────────────────────────────

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
            // Re-apply scissor to whatever state we restored to.
            self.apply_scissor();
        }
    }

    fn translate(&mut self, tx: f64, ty: f64) {
        self.state_stack
            .last_mut()
            .unwrap()
            .0
            .premultiply(&TransAffine::new_translation(tx, ty));
    }

    fn rotate(&mut self, radians: f64) {
        self.state_stack
            .last_mut()
            .unwrap()
            .0
            .premultiply(&TransAffine::new_rotation(radians));
    }

    fn scale(&mut self, sx: f64, sy: f64) {
        self.state_stack
            .last_mut()
            .unwrap()
            .0
            .premultiply(&TransAffine::new_scaling(sx, sy));
    }

    fn set_transform(&mut self, m: TransAffine) {
        self.state_stack.last_mut().unwrap().0 = m;
    }

    fn reset_transform(&mut self) {
        self.state_stack.last_mut().unwrap().0 = TransAffine::new();
    }

    fn supports_compositing_layers(&self) -> bool {
        true
    }

    fn push_layer(&mut self, width: f64, height: f64) {
        unsafe {
            self.push_gl_layer(width, height, 1.0);
        }
    }

    fn push_layer_with_alpha(&mut self, width: f64, height: f64, alpha: f64) {
        unsafe {
            self.push_gl_layer(width, height, alpha);
        }
    }

    fn composite_retained_layer(&mut self, key: u64, width: f64, height: f64, alpha: f64) -> bool {
        unsafe { self.composite_retained_gl_layer(key, width, height, alpha) }
    }

    fn push_retained_layer_with_alpha(&mut self, key: u64, width: f64, height: f64, alpha: f64) {
        unsafe {
            self.push_retained_gl_layer(key, width, height, alpha);
        }
    }

    fn set_layer_rounded_clip(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        unsafe {
            self.set_rounded_layer_clip(x, y, w, h, r);
        }
    }

    fn pop_layer(&mut self) {
        unsafe {
            self.pop_gl_layer();
        }
    }

    /// Execute GPU content inline at the correct painter-order depth.
    ///
    /// Passes `&*self.gl` as `&dyn Any` — the caller downcasts to
    /// `glow::Context`.  Viewport dimensions come from `self.viewport`.
    fn gl_paint(&mut self, screen_rect: agg_gui::Rect, painter: &mut dyn agg_gui::GlPaint) {
        self.apply_scissor();
        let full_w = self.viewport.0 as i32;
        let full_h = self.viewport.1 as i32;
        // Pass the current framework scissor so the painter can intersect its own
        // scissor with it — this ensures parent clips (collapsed windows, etc.)
        // correctly hide GPU-rendered content.
        let parent_clip = self.current_clip();
        painter.gl_paint(
            self.gl.as_ref() as &dyn std::any::Any,
            screen_rect,
            full_w,
            full_h,
            parent_clip,
        );
        // Re-apply our scissor after — the painter may have disabled it.
        self.apply_scissor();
    }
}
