use super::*;

impl GlGfxCtx {
    pub(crate) unsafe fn push_gl_layer(&mut self, width: f64, height: f64, alpha: f64) {
        let width = width.ceil().max(1.0) as i32;
        let height = height.ceil().max(1.0) as i32;
        let saved = self.capture_draw_state();
        let origin_x = self.ctm().tx;
        let origin_y = self.ctm().ty;
        let parent_fbo = self.current_fbo;

        let gl = &*self.gl;
        let texture = gl.create_texture().expect("create layer texture");
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
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
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            None,
        );

        let fbo = gl.create_framebuffer().expect("create layer framebuffer");
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );

        let stencil = gl
            .create_renderbuffer()
            .expect("create layer depth-stencil");
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(stencil));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH24_STENCIL8, width, height);
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_STENCIL_ATTACHMENT,
            glow::RENDERBUFFER,
            Some(stencil),
        );

        debug_assert_eq!(
            gl.check_framebuffer_status(glow::FRAMEBUFFER),
            glow::FRAMEBUFFER_COMPLETE
        );

        self.layer_stack.push(GlLayerEntry {
            fbo,
            texture,
            stencil,
            width,
            height,
            origin_x,
            origin_y,
            alpha: alpha.clamp(0.0, 1.0),
            parent_fbo,
            saved,
            retained_key: None,
        });
        self.current_fbo = Some(fbo);
        self.viewport = (width as f32, height as f32);
        self.state_stack = vec![(TransAffine::new(), None)];
        self.path = PathStorage::new();

        gl.viewport(0, 0, width, height);
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::STENCIL_TEST);
        gl.stencil_mask(0xFF);
        gl.color_mask(true, true, true, true);
        gl.clear_color(0.0, 0.0, 0.0, 0.0);
        gl.clear_stencil(0);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
        gl.enable(glow::BLEND);
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
    }

    pub(crate) unsafe fn pop_gl_layer(&mut self) {
        let Some(layer) = self.layer_stack.pop() else {
            return;
        };
        self.current_fbo = layer.parent_fbo;
        self.gl
            .bind_framebuffer(glow::FRAMEBUFFER, layer.parent_fbo);
        self.restore_draw_state(layer.saved.clone());
        self.gl.viewport(
            0,
            0,
            self.viewport.0.round() as i32,
            self.viewport.1.round() as i32,
        );
        self.gl.disable(glow::STENCIL_TEST);
        self.apply_scissor();
        self.composite_layer_texture(&layer);
        self.apply_scissor();
        if layer.retained_key.is_none() {
            self.gl.delete_renderbuffer(layer.stencil);
            self.gl.delete_framebuffer(layer.fbo);
            self.gl.delete_texture(layer.texture);
        }
    }

    pub(crate) unsafe fn push_retained_gl_layer(
        &mut self,
        key: u64,
        width: f64,
        height: f64,
        alpha: f64,
    ) {
        let width = width.ceil().max(1.0) as i32;
        let height = height.ceil().max(1.0) as i32;
        self.ensure_retained_layer(key, width, height);

        let retained = self
            .retained_layers
            .get(&key)
            .expect("retained layer exists");
        let fbo = retained.fbo;
        let texture = retained.texture;
        let stencil = retained.stencil;
        let saved = self.capture_draw_state();
        let origin_x = self.ctm().tx;
        let origin_y = self.ctm().ty;
        let parent_fbo = self.current_fbo;

        self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        self.layer_stack.push(GlLayerEntry {
            fbo,
            texture,
            stencil,
            width,
            height,
            origin_x,
            origin_y,
            alpha: alpha.clamp(0.0, 1.0),
            parent_fbo,
            saved,
            retained_key: Some(key),
        });
        self.current_fbo = Some(fbo);
        self.viewport = (width as f32, height as f32);
        self.state_stack = vec![(TransAffine::new(), None)];
        self.path = PathStorage::new();

        self.gl.viewport(0, 0, width, height);
        self.gl.disable(glow::SCISSOR_TEST);
        self.gl.disable(glow::STENCIL_TEST);
        self.gl.stencil_mask(0xFF);
        self.gl.color_mask(true, true, true, true);
        self.gl.clear_color(0.0, 0.0, 0.0, 0.0);
        self.gl.clear_stencil(0);
        self.gl.clear_depth_f32(1.0);
        self.gl
            .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);
        self.gl.enable(glow::BLEND);
        self.gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
    }

    pub(crate) unsafe fn composite_retained_gl_layer(
        &self,
        key: u64,
        width: f64,
        height: f64,
        alpha: f64,
    ) -> bool {
        let Some(retained) = self.retained_layers.get(&key) else {
            return false;
        };
        if retained.width != width.ceil().max(1.0) as i32
            || retained.height != height.ceil().max(1.0) as i32
        {
            return false;
        }
        let layer = GlLayerEntry {
            fbo: retained.fbo,
            texture: retained.texture,
            stencil: retained.stencil,
            width: retained.width,
            height: retained.height,
            origin_x: self.ctm().tx,
            origin_y: self.ctm().ty,
            alpha: alpha.clamp(0.0, 1.0),
            parent_fbo: self.current_fbo,
            saved: self.capture_draw_state(),
            retained_key: Some(key),
        };
        self.composite_layer_texture(&layer);
        true
    }

    unsafe fn ensure_retained_layer(&mut self, key: u64, width: i32, height: i32) {
        let needs_new = self
            .retained_layers
            .get(&key)
            .map(|l| l.width != width || l.height != height)
            .unwrap_or(true);
        if !needs_new {
            return;
        }
        if let Some(old) = self.retained_layers.remove(&key) {
            self.gl.delete_renderbuffer(old.stencil);
            self.gl.delete_framebuffer(old.fbo);
            self.gl.delete_texture(old.texture);
        }

        let gl = &*self.gl;
        let texture = gl.create_texture().expect("create retained layer texture");
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
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
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            None,
        );
        let fbo = gl.create_framebuffer().expect("create retained layer fbo");
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let stencil = gl
            .create_renderbuffer()
            .expect("create retained depth-stencil");
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(stencil));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH24_STENCIL8, width, height);
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_STENCIL_ATTACHMENT,
            glow::RENDERBUFFER,
            Some(stencil),
        );
        debug_assert_eq!(
            gl.check_framebuffer_status(glow::FRAMEBUFFER),
            glow::FRAMEBUFFER_COMPLETE
        );
        gl.bind_framebuffer(glow::FRAMEBUFFER, self.current_fbo);
        self.retained_layers.insert(
            key,
            RetainedGlLayer {
                fbo,
                texture,
                stencil,
                width,
                height,
            },
        );
    }

    pub(crate) unsafe fn set_rounded_layer_clip(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        if self.layer_stack.is_empty() {
            return;
        }

        self.gl.clear_stencil(0);
        self.gl.clear(glow::STENCIL_BUFFER_BIT);
        self.gl.enable(glow::STENCIL_TEST);
        self.gl.stencil_mask(0xFF);
        self.gl.stencil_func(glow::ALWAYS, 1, 0xFF);
        self.gl
            .stencil_op(glow::REPLACE, glow::REPLACE, glow::REPLACE);
        self.gl.color_mask(false, false, false, false);

        let saved_fill = self.fill_color;
        let saved_alpha = self.global_alpha;
        self.fill_color = Color::rgba(1.0, 1.0, 1.0, 1.0);
        self.global_alpha = 1.0;
        self.path = PathStorage::new();
        let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
        let mut rr = RoundedRect::new(x, y, x + w, y + h, r);
        rr.normalize_radius();
        self.path.concat_path(&mut rr, 0);
        self.do_fill();
        self.fill_color = saved_fill;
        self.global_alpha = saved_alpha;

        self.gl.color_mask(true, true, true, true);
        self.gl.stencil_func(glow::EQUAL, 1, 0xFF);
        self.gl.stencil_op(glow::KEEP, glow::KEEP, glow::KEEP);
        self.gl.stencil_mask(0x00);
    }

    unsafe fn composite_layer_texture(&self, layer: &GlLayerEntry) {
        if layer.alpha <= 0.001 {
            return;
        }

        let gl = &*self.gl;
        let x0 = layer.origin_x as f32;
        let y0 = layer.origin_y as f32;
        let x1 = x0 + layer.width as f32;
        let y1 = y0 + layer.height as f32;
        let verts: [f32; 24] = [
            x0, y0, 0.0, 0.0, x1, y0, 1.0, 0.0, x1, y1, 1.0, 1.0, x0, y0, 0.0, 0.0, x1, y1, 1.0,
            1.0, x0, y1, 0.0, 1.0,
        ];

        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(layer.texture));
        gl.use_program(Some(self.layer_prog));
        gl.uniform_2_f32(
            self.layer_res_loc.as_ref(),
            self.viewport.0,
            self.viewport.1,
        );
        gl.uniform_1_i32(self.layer_sampler_loc.as_ref(), 0);
        gl.uniform_1_f32(self.layer_alpha_loc.as_ref(), layer.alpha as f32);
        gl.bind_vertex_array(Some(self.tex_vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.tex_vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&verts),
            glow::DYNAMIC_DRAW,
        );
        gl.enable(glow::BLEND);
        let parent_is_layer = self.current_fbo.is_some();
        if parent_is_layer {
            gl.blend_func_separate(
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
        } else {
            gl.blend_func_separate(glow::ONE, glow::ONE_MINUS_SRC_ALPHA, glow::ZERO, glow::ONE);
        }
        gl.draw_arrays(glow::TRIANGLES, 0, 6);
        gl.bind_vertex_array(None);
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ZERO,
            glow::ONE,
        );
    }

}
