use super::*;

impl GlGfxCtx {
    /// Get-or-upload a single 3-byte/pixel plane (colour or alpha) of an
    /// LCD backbuffer, cached on its `Arc` pointer.  Matches the pattern
    /// of `draw_lcd_mask_arc` so GPU memory stays bounded automatically
    /// as widget backbuffer caches turn over.
    pub(crate) unsafe fn lcd_plane_get_or_upload(
        &mut self,
        data: &std::sync::Arc<Vec<u8>>,
        w: u32,
        h: u32,
    ) -> glow::Texture {
        let key = std::sync::Arc::as_ptr(data) as usize;
        if let Some(entry) = self.lcd_arc_texture_cache.get(&key) {
            if entry.weak.upgrade().is_some() && entry.w == w && entry.h == h {
                return entry.texture;
            }
        }
        let tex = self
            .gl
            .create_texture()
            .expect("create lcd backbuffer texture");
        self.upload_lcd_texture(tex, w, h, data.as_slice());
        if let Some(old) = self.lcd_arc_texture_cache.insert(
            key,
            ArcTextureEntry {
                weak: std::sync::Arc::downgrade(data),
                texture: tex,
                w,
                h,
            },
        ) {
            self.gl.delete_texture(old.texture);
        }
        self.lcd_arc_texture_cache.retain(|_, e| {
            if e.weak.upgrade().is_some() {
                true
            } else {
                self.gl.delete_texture(e.texture);
                false
            }
        });
        tex
    }

    /// Composite a two-plane LCD backbuffer (colour + alpha textures,
    /// both top-row-first RGB8) onto the destination with per-channel
    /// src-over via dual-source blend.  Preserves LCD chroma through
    /// the cache round-trip — see `LCB_FRAG` for the math.
    pub(crate) unsafe fn draw_lcd_backbuffer_quad(
        &self,
        color_tex: glow::Texture,
        alpha_tex: glow::Texture,
        w: u32,
        h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        let gl = &*self.gl;
        let ctm = *self.ctm();
        // Snap origin to the integer pixel grid — subpixel phase pattern
        // is only valid at 1:1 texel-to-pixel mapping.  Same rationale
        // as `draw_lcd_quad` for text masks.
        let bl_x = (dst_x * ctm.sx + dst_y * ctm.shx + ctm.tx).round();
        let bl_y = (dst_x * ctm.shy + dst_y * ctm.sy + ctm.ty).round();
        let tr_x = bl_x + dst_w;
        let tr_y = bl_y + dst_h;
        let _ = w;
        let _ = h;

        // Cached planes are TOP-ROW-FIRST (the cache layout), so UV v=0
        // corresponds to the visually-top row of the image.  Our Y-up
        // quad has bl at low Y; sample `v=1` (bottom row of image data,
        // which the UV v=1 maps to in GL texture space) at bl and `v=0`
        // at tr.  Matches `draw_image_rgba_arc`'s convention.
        let verts: [f32; 16] = [
            bl_x as f32,
            bl_y as f32,
            0.0,
            1.0,
            tr_x as f32,
            bl_y as f32,
            1.0,
            1.0,
            tr_x as f32,
            tr_y as f32,
            1.0,
            0.0,
            bl_x as f32,
            tr_y as f32,
            0.0,
            0.0,
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

        gl.use_program(Some(self.lcb_prog));
        gl.uniform_2_f32(self.lcb_res_loc.as_ref(), self.viewport.0, self.viewport.1);
        gl.uniform_1_i32(self.lcb_color_sampler.as_ref(), 0);
        gl.uniform_1_i32(self.lcb_alpha_sampler.as_ref(), 1);

        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(color_tex));
        gl.active_texture(glow::TEXTURE1);
        gl.bind_texture(glow::TEXTURE_2D, Some(alpha_tex));

        // Dual-source blend: fragment's `out_color` is the "source" and
        // `out_coverage` supplies the per-channel blend factors.  With
        // `sfactor=ONE, dfactor=ONE_MINUS_SRC1_COLOR` each destination
        // channel computes `dst = out_color + dst * (1 - out_coverage)`.
        // Alpha channel gets the max-alpha (passed as out_color.a and
        // out_coverage.a) so the fb's alpha accumulates correctly.
        #[cfg(not(target_arch = "wasm32"))]
        gl.blend_func_separate(
            glow::ONE,
            glow::ONE_MINUS_SRC1_COLOR,
            glow::ONE,
            glow::ONE_MINUS_SRC1_ALPHA,
        );
        // WebGL 2 path: no dual-source → 3-pass color-masked.  Each
        // pass uses standard `ONE, ONE_MINUS_SRC_ALPHA` blend (premult
        // src-over with src alpha = that channel's alpha).  `u_channel`
        // tells the shader which channel to emit.
        #[cfg(target_arch = "wasm32")]
        gl.blend_func_separate(
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );

        gl.bind_vertex_array(Some(self.lcd_vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.lcd_vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&verts),
            glow::STREAM_DRAW,
        );
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ibo));
        gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(&idx),
            glow::STREAM_DRAW,
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0);
        }
        #[cfg(target_arch = "wasm32")]
        {
            for ch in 0..3i32 {
                gl.uniform_1_i32(self.lcb_channel_loc.as_ref(), ch);
                gl.color_mask(ch == 0, ch == 1, ch == 2, false);
                gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0);
            }
            gl.color_mask(true, true, true, true);
        }
        gl.bind_vertex_array(None);

        // Rebind the default texture unit so later draws don't leak
        // bindings off unit 1.
        gl.active_texture(glow::TEXTURE0);

        // Restore standard alpha blend state.
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ZERO,
            glow::ONE,
        );
    }

}
