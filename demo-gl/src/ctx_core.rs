use super::*;
use crate::gl_support::compile_program;
use crate::shaders::*;

impl GlGfxCtx {
    /// Create a new `GlGfxCtx` backed by `gl`.  Call once; reuse every frame
    /// via [`reset`].
    ///
    /// # Safety
    /// `gl` must be a valid WebGL2 / OpenGL context.
    pub unsafe fn new(gl: Rc<glow::Context>, width: f32, height: f32) -> Self {
        let prog = match compile_program(&gl, SOLID_VERT, SOLID_FRAG) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("GlGfxCtx: shader error: {e}");
                panic!("GlGfxCtx shader compile/link failed");
            }
        };
        let res_loc = gl.get_uniform_location(prog, "u_resolution");
        let color_loc = gl.get_uniform_location(prog, "u_color");

        let vao = gl.create_vertex_array().expect("create VAO");
        let vbo = gl.create_buffer().expect("create VBO");
        let ibo = gl.create_buffer().expect("create IBO");

        // Bind VAO so attribute pointer and IBO binding are saved inside it.
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        // a_pos layout: vec2 f32 (8 bytes per vertex, stride=8, offset=0)
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
        gl.enable_vertex_attrib_array(0);
        // Bind IBO inside the VAO so the VAO remembers it.
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
        gl.bind_vertex_array(None);

        // ── AA solid pipeline (halo-strip alpha) ───────────────────────────
        let aa_prog = compile_program(&gl, AA_VERT, AA_FRAG).expect("aa shader compile/link");
        let aa_res_loc = gl.get_uniform_location(aa_prog, "u_resolution");
        let aa_color_loc = gl.get_uniform_location(aa_prog, "u_color");
        let aa_vao = gl.create_vertex_array().expect("create AA VAO");
        let aa_vbo = gl.create_buffer().expect("create AA VBO");
        let aa_ibo = gl.create_buffer().expect("create AA IBO");
        gl.bind_vertex_array(Some(aa_vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(aa_vbo));
        // Layout: vec2 pos + f32 alpha = 12 bytes per vertex.
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 12, 0);
        gl.vertex_attrib_pointer_f32(1, 1, glow::FLOAT, false, 12, 8);
        gl.enable_vertex_attrib_array(0);
        gl.enable_vertex_attrib_array(1);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(aa_ibo));
        gl.bind_vertex_array(None);

        // ── Textured-quad pipeline ─────────────────────────────────────────
        let tex_prog = compile_program(&gl, TEX_VERT, TEX_FRAG).expect("tex shader compile/link");
        let tex_res_loc = gl.get_uniform_location(tex_prog, "u_resolution");
        let tex_sampler_loc = gl.get_uniform_location(tex_prog, "u_tex");
        let layer_prog =
            compile_program(&gl, TEX_VERT, LAYER_FRAG).expect("layer shader compile/link");
        let layer_res_loc = gl.get_uniform_location(layer_prog, "u_resolution");
        let layer_sampler_loc = gl.get_uniform_location(layer_prog, "u_tex");
        let layer_alpha_loc = gl.get_uniform_location(layer_prog, "u_alpha");

        let tex_vao = gl.create_vertex_array().expect("create tex VAO");
        let tex_vbo = gl.create_buffer().expect("create tex VBO");
        gl.bind_vertex_array(Some(tex_vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(tex_vbo));
        // Layout: vec2 pos + vec2 uv = 16 bytes per vertex.
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
        gl.enable_vertex_attrib_array(0);
        gl.enable_vertex_attrib_array(1);
        gl.bind_vertex_array(None);

        // ── LCD subpixel pipeline ──────────────────────────────────────────
        let lcd_prog = compile_program(&gl, LCD_VERT, LCD_FRAG).expect("lcd shader compile/link");
        let lcd_res_loc = gl.get_uniform_location(lcd_prog, "u_resolution");
        let lcd_sampler_loc = gl.get_uniform_location(lcd_prog, "u_mask");
        let lcd_color_loc = gl.get_uniform_location(lcd_prog, "u_color");
        // WASM-only uniform; desktop shader doesn't declare it.  On a
        // desktop build this returns `None` which is harmless — the
        // desktop draw path doesn't call `uniform_1_i32` on it.
        let lcd_channel_loc = gl.get_uniform_location(lcd_prog, "u_channel");

        let lcd_vao = gl.create_vertex_array().expect("create lcd VAO");
        let lcd_vbo = gl.create_buffer().expect("create lcd VBO");
        gl.bind_vertex_array(Some(lcd_vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(lcd_vbo));
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);
        gl.enable_vertex_attrib_array(0);
        gl.enable_vertex_attrib_array(1);
        gl.bind_vertex_array(None);

        // ── LCD backbuffer pipeline ────────────────────────────────────────
        // Reuses `lcd_vao` / `lcd_vbo` for vertex data (same layout: vec2
        // pos + vec2 uv) — the only difference from the text LCD shader is
        // the fragment stage and the second sampler uniform.
        let lcb_prog =
            compile_program(&gl, LCD_VERT, LCB_FRAG).expect("lcd backbuffer shader compile/link");
        let lcb_res_loc = gl.get_uniform_location(lcb_prog, "u_resolution");
        let lcb_color_sampler = gl.get_uniform_location(lcb_prog, "u_color");
        let lcb_alpha_sampler = gl.get_uniform_location(lcb_prog, "u_alpha");
        let lcb_channel_loc = gl.get_uniform_location(lcb_prog, "u_channel");

        Self {
            gl,
            viewport: (width, height),
            prog,
            vao,
            vbo,
            ibo,
            res_loc,
            color_loc,
            aa_prog,
            aa_vao,
            aa_vbo,
            aa_ibo,
            aa_res_loc,
            aa_color_loc,
            tex_prog,
            tex_vao,
            tex_vbo,
            tex_res_loc,
            tex_sampler_loc,
            layer_prog,
            layer_res_loc,
            layer_sampler_loc,
            layer_alpha_loc,
            lcd_prog,
            lcd_vao,
            lcd_vbo,
            lcd_res_loc,
            lcd_sampler_loc,
            lcd_color_loc,
            lcd_channel_loc,
            texture_cache: std::collections::HashMap::new(),
            texture_cache_order: std::collections::VecDeque::new(),
            arc_texture_cache: std::collections::HashMap::new(),
            layer_stack: Vec::new(),
            current_fbo: None,
            lcd_arc_texture_cache: std::collections::HashMap::new(),
            lcb_prog,
            lcb_res_loc,
            lcb_color_sampler,
            lcb_alpha_sampler,
            lcb_channel_loc,
            fill_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            stroke_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            line_width: 1.0,
            line_join: LineJoin::Miter,
            line_cap: LineCap::Butt,
            fill_rule: FillRule::NonZero,
            miter_limit: 4.0,
            line_dash: Vec::new(),
            dash_offset: 0.0,
            global_alpha: 1.0,
            state_stack: vec![(TransAffine::new(), None)],
            path: PathStorage::new(),
            font: None,
            font_size: 16.0,
            glyph_cache: GlyphCache::new(),
            lcd_mode: false,
        }
    }

    /// Read the contents of the back buffer into a top-down RGBA8 buffer.
    /// Returns `(pixels, width, height)` where the first `width * 4` bytes are
    /// the TOP row (left-to-right, RGBA).  Intended for the Screenshot demo
    /// and the WASM download-blob path — not used in the rendering hot path.
    ///
    /// Must be called BEFORE the window's buffer swap for the current frame,
    /// otherwise the back buffer contents are undefined on some platforms.
    pub fn read_screenshot(&self) -> (Vec<u8>, u32, u32) {
        let w = self.viewport.0.round().max(1.0) as i32;
        let h = self.viewport.1.round().max(1.0) as i32;
        let total = (w * h * 4) as usize;
        let mut buf = vec![0u8; total];
        unsafe {
            self.gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            self.gl.read_pixels(
                0,
                0,
                w,
                h,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut buf),
            );
        }
        // Flip vertically: GL origin is bottom-left, PNG top-left.
        let stride = (w * 4) as usize;
        let mut flipped = vec![0u8; total];
        for y in 0..(h as usize) {
            let src_off = y * stride;
            let dst_off = (h as usize - 1 - y) * stride;
            flipped[dst_off..dst_off + stride].copy_from_slice(&buf[src_off..src_off + stride]);
        }
        (flipped, w as u32, h as u32)
    }

    /// Reset drawing state for a new frame.  Does NOT recreate GL resources.
    pub fn reset(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.current_fbo = None;
        self.fill_color = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.stroke_color = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.line_width = 1.0;
        self.fill_rule = FillRule::NonZero;
        self.miter_limit = 4.0;
        self.line_dash.clear();
        self.dash_offset = 0.0;
        self.global_alpha = 1.0;
        self.state_stack = vec![(TransAffine::new(), None)];
        self.path = PathStorage::new();
        self.font = None;
        self.font_size = 16.0;
        // Disable any lingering scissor from the previous frame.
        unsafe {
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.disable(glow::STENCIL_TEST);
        }
    }

    /// Set the LCD mode for this ctx.  Demo main loops call this each
    /// frame with `font_settings::lcd_enabled()` so direct-to-screen
    /// text picks up the global toggle.
    pub fn set_lcd_mode(&mut self, on: bool) {
        self.lcd_mode = on;
    }

    // ---- internal helpers --------------------------------------------------

    pub(crate) fn capture_draw_state(&self) -> SavedGlDrawState {
        SavedGlDrawState {
            viewport: self.viewport,
            fill_color: self.fill_color,
            stroke_color: self.stroke_color,
            line_width: self.line_width,
            line_join: self.line_join,
            line_cap: self.line_cap,
            fill_rule: self.fill_rule,
            miter_limit: self.miter_limit,
            line_dash: self.line_dash.clone(),
            dash_offset: self.dash_offset,
            global_alpha: self.global_alpha,
            state_stack: self.state_stack.clone(),
            font: self.font.clone(),
            font_size: self.font_size,
            lcd_mode: self.lcd_mode,
        }
    }

    pub(crate) fn restore_draw_state(&mut self, saved: SavedGlDrawState) {
        self.viewport = saved.viewport;
        self.fill_color = saved.fill_color;
        self.stroke_color = saved.stroke_color;
        self.line_width = saved.line_width;
        self.line_join = saved.line_join;
        self.line_cap = saved.line_cap;
        self.fill_rule = saved.fill_rule;
        self.miter_limit = saved.miter_limit;
        self.line_dash = saved.line_dash;
        self.dash_offset = saved.dash_offset;
        self.global_alpha = saved.global_alpha;
        self.state_stack = saved.state_stack;
        self.font = saved.font;
        self.font_size = saved.font_size;
        self.lcd_mode = saved.lcd_mode;
        self.path = PathStorage::new();
    }

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
        self.gl.delete_renderbuffer(layer.stencil);
        self.gl.delete_framebuffer(layer.fbo);
        self.gl.delete_texture(layer.texture);
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

    pub(crate) fn ctm(&self) -> &TransAffine {
        &self.state_stack.last().expect("state stack never empty").0
    }

    pub(crate) fn current_clip(&self) -> Option<[i32; 4]> {
        self.state_stack.last().expect("state stack never empty").1
    }

    pub(crate) fn apply_scissor(&self) {
        unsafe {
            match self.current_clip() {
                Some([x, y, w, h]) => {
                    self.gl.enable(glow::SCISSOR_TEST);
                    self.gl.scissor(x, y, w, h);
                }
                None => {
                    self.gl.disable(glow::SCISSOR_TEST);
                }
            }
        }
    }

    /// Transform a local Y-up point to screen-space Y-up pixels.
    #[inline]
    pub(crate) fn transform_pt(&self, x: f64, y: f64) -> [f32; 2] {
        let (mut px, mut py) = (x, y);
        self.ctm().transform(&mut px, &mut py);
        [px as f32, py as f32]
    }

    /// Upload a 3-channel LCD coverage mask into `tex` as an RGB texture.
    pub(crate) unsafe fn upload_lcd_texture(
        &self,
        tex: glow::Texture,
        w: u32,
        h: u32,
        data: &[u8],
    ) {
        let gl = &*self.gl;
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
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
            glow::RGB as i32,
            w as i32,
            h as i32,
            0,
            glow::RGB,
            glow::UNSIGNED_BYTE,
            Some(data),
        );
    }

    /// Draw the LCD-mask quad with dual-source blending.  `tex` is a
    /// pre-uploaded RGB mask of size `mask_w × mask_h`.  The quad's
    /// bottom-left lands at `(dst_x, dst_y)` in local coords after the
    /// current CTM is applied.  Mask rows are Y-up so UV (0, 0) maps
    /// to the bottom of the quad.
    pub(crate) unsafe fn draw_lcd_quad(
        &self,
        tex: glow::Texture,
        mask_w: u32,
        mask_h: u32,
        src_color: agg_gui::Color,
        dst_x: f64,
        dst_y: f64,
    ) {
        let gl = &*self.gl;
        let ctm = *self.ctm();
        // LCD coverage masks bake a carefully-phased R/G/B subpixel pattern
        // at 1:1 texel-to-pixel resolution.  If the quad lands at fractional
        // pixel coordinates, each fragment samples a texel whose subpixel
        // phase is offset from the destination's — the 3× filter's chroma
        // structure smears across pixel boundaries and text reads as blurry.
        // Snap the origin to the integer pixel grid so every texel maps to
        // exactly one screen pixel.  (Mirrors the CPU path in
        // `gfx_ctx::draw_lcd_mask`, which `.round()`s for the same reason.)
        let bl_x = (dst_x * ctm.sx + dst_y * ctm.shx + ctm.tx).round();
        let bl_y = (dst_x * ctm.shy + dst_y * ctm.sy + ctm.ty).round();
        let tr_x = bl_x + mask_w as f64;
        let tr_y = bl_y + mask_h as f64;

        let verts: [f32; 16] = [
            bl_x as f32,
            bl_y as f32,
            0.0,
            0.0,
            tr_x as f32,
            bl_y as f32,
            1.0,
            0.0,
            tr_x as f32,
            tr_y as f32,
            1.0,
            1.0,
            bl_x as f32,
            tr_y as f32,
            0.0,
            1.0,
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

        gl.use_program(Some(self.lcd_prog));
        gl.uniform_2_f32(self.lcd_res_loc.as_ref(), self.viewport.0, self.viewport.1);
        gl.uniform_1_i32(self.lcd_sampler_loc.as_ref(), 0);
        let a = (src_color.a as f64 * self.global_alpha) as f32;
        gl.uniform_4_f32(
            self.lcd_color_loc.as_ref(),
            src_color.r,
            src_color.g,
            src_color.b,
            a,
        );
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));

        // Dual-source blend for per-channel src-over.  Keep the
        // alpha-channel-preserving factors so the framebuffer's A
        // stays at 1 (matches `begin_frame`).
        #[cfg(not(target_arch = "wasm32"))]
        gl.blend_func_separate(
            glow::SRC1_COLOR,
            glow::ONE_MINUS_SRC1_COLOR,
            glow::ZERO,
            glow::ONE,
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

        // Desktop: single draw — dual-source blend does per-channel work.
        // WASM: 3 draws — each pass writes ONE channel via `glColorMask`
        // and selects that channel's coverage via `u_channel`.  Standard
        // `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` blend computes per-channel
        // src-over.  Alpha channel write disabled on all 3 passes so
        // framebuffer alpha stays at 1.
        #[cfg(not(target_arch = "wasm32"))]
        {
            gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0);
        }
        #[cfg(target_arch = "wasm32")]
        {
            for ch in 0..3i32 {
                gl.uniform_1_i32(self.lcd_channel_loc.as_ref(), ch);
                gl.color_mask(ch == 0, ch == 1, ch == 2, false);
                gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0);
            }
            gl.color_mask(true, true, true, true);
        }
        gl.bind_vertex_array(None);

        // Restore standard alpha blend state.
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ZERO,
            glow::ONE,
        );
    }

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

    /// Submit triangles with the given colour.
    ///
    /// `verts` is a slice of screen-space [f32;2] XY pairs.
    /// `indices` is a list of triangle vertex indices into `verts`.
    pub(crate) unsafe fn draw_triangles(&self, verts: &[[f32; 2]], indices: &[u32], color: Color) {
        if verts.is_empty() || indices.is_empty() {
            return;
        }

        let a = (color.a * self.global_alpha as f32).clamp(0.0, 1.0);

        self.gl.use_program(Some(self.prog));
        if let Some(ref loc) = self.res_loc {
            self.gl
                .uniform_2_f32(Some(loc), self.viewport.0, self.viewport.1);
        }
        if let Some(ref loc) = self.color_loc {
            self.gl
                .uniform_4_f32(Some(loc), color.r, color.g, color.b, a);
        }

        // Bind VAO (restores attribute pointer, VBO association, and IBO binding).
        self.gl.bind_vertex_array(Some(self.vao));

        // Upload vertex data.
        self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        self.gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(verts),
            glow::STREAM_DRAW,
        );

        // Upload index data to the persistent IBO (already bound in the VAO).
        self.gl
            .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ibo));
        self.gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(indices),
            glow::STREAM_DRAW,
        );

        self.gl
            .draw_elements(glow::TRIANGLES, indices.len() as i32, glow::UNSIGNED_INT, 0);

        self.gl.bind_vertex_array(None);
    }

    /// Submit AA triangles — vertices are `[x, y, alpha]` triples.
    ///
    /// Interior triangles and halo-strip triangles are blended uniformly via
    /// the AA solid shader: per-vertex `a_alpha` is interpolated across the
    /// primitive, so halo quads with inner=1.0 / outer=0.0 produce an
    /// analytic edge-coverage ramp one pixel wide.
    pub(crate) unsafe fn submit_aa_triangles(
        &self,
        verts: &[[f32; 3]],
        indices: &[u32],
        color: Color,
    ) {
        if verts.is_empty() || indices.is_empty() {
            return;
        }
        let a = (color.a * self.global_alpha as f32).clamp(0.0, 1.0);

        self.gl.use_program(Some(self.aa_prog));
        if let Some(ref loc) = self.aa_res_loc {
            self.gl
                .uniform_2_f32(Some(loc), self.viewport.0, self.viewport.1);
        }
        if let Some(ref loc) = self.aa_color_loc {
            self.gl
                .uniform_4_f32(Some(loc), color.r, color.g, color.b, a);
        }

        self.gl.bind_vertex_array(Some(self.aa_vao));

        self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.aa_vbo));
        self.gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(verts),
            glow::STREAM_DRAW,
        );

        self.gl
            .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.aa_ibo));
        self.gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(indices),
            glow::STREAM_DRAW,
        );

        self.gl
            .draw_elements(glow::TRIANGLES, indices.len() as i32, glow::UNSIGNED_INT, 0);

        self.gl.bind_vertex_array(None);
    }

    /// Tessellate the accumulated AGG path and draw as a fill with analytic
    /// edge AA.
    ///
    /// The path is flattened and transformed through AGG before
    /// `tessellate_path_aa` attaches a 1-pixel halo strip along every original
    /// polygon boundary.
    pub(crate) unsafe fn do_fill(&mut self) {
        use agg_gui::gl_renderer::tessellate_path_aa;

        let transform = *self.ctm();
        let fill_rule = self.fill_rule;
        let tess = {
            let mut curves = ConvCurve::new(&mut self.path);
            let mut transformed = ConvTransform::new(&mut curves, transform);
            tessellate_path_aa(&mut transformed, 1.0, fill_rule)
        };

        if let Some((verts, idx)) = tess {
            let color = self.fill_color;
            self.submit_aa_triangles(&verts, &idx, color);
        }
    }

    /// Stroke the accumulated AGG path with analytic edge AA.
    ///
    /// Single battle-tested pipeline:
    ///   AGG `PathStorage` → `ConvStroke` (proper miter/round/bevel joins +
    ///   butt/round/square caps) → [`tessellate_path_aa`] (AGG
    ///   VertexSource → tess2 → interior triangles + edge-flag halo strips).
    pub(crate) unsafe fn do_stroke(&mut self) {
        use agg_gui::gl_renderer::tessellate_path_aa;

        let transform = *self.ctm();
        let width = self.line_width;
        let join = self.line_join;
        let cap = self.line_cap;
        let miter_limit = self.miter_limit;
        let dashes = self.line_dash.clone();
        let dash_offset = self.dash_offset;
        let tess = {
            let mut curves = ConvCurve::new(&mut self.path);
            if dashes.is_empty() {
                let mut stroke = ConvStroke::new(&mut curves);
                stroke.set_width(width);
                stroke.set_line_join(join);
                stroke.set_line_cap(cap);
                stroke.set_miter_limit(miter_limit);
                let mut transformed = ConvTransform::new(&mut stroke, transform);
                tessellate_path_aa(&mut transformed, 1.0, FillRule::NonZero)
            } else {
                let mut dash = ConvDash::new(&mut curves);
                configure_dashes(&mut dash, &dashes, dash_offset);
                let mut stroke = ConvStroke::new(dash);
                stroke.set_width(width);
                stroke.set_line_join(join);
                stroke.set_line_cap(cap);
                stroke.set_miter_limit(miter_limit);
                let mut transformed = ConvTransform::new(&mut stroke, transform);
                tessellate_path_aa(&mut transformed, 1.0, FillRule::NonZero)
            }
        };

        if let Some((verts, idx)) = tess {
            let color = self.stroke_color;
            self.submit_aa_triangles(&verts, &idx, color);
        }
    }
}

fn configure_dashes<VS: agg_rust::basics::VertexSource>(
    dash: &mut agg_rust::conv_dash::ConvDash<VS>,
    dashes: &[f64],
    dash_offset: f64,
) {
    let mut chunks = dashes.chunks_exact(2);
    for pair in &mut chunks {
        dash.add_dash(pair[0], pair[1]);
    }
    if let Some(&last) = chunks.remainder().first() {
        dash.add_dash(last, last);
    }
    dash.dash_start(dash_offset);
}
