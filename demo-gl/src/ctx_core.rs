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

        let gradient = GradientPipeline::new(&gl);

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
            gradient,
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
            retained_layers: std::collections::HashMap::new(),
            current_fbo: None,
            lcd_arc_texture_cache: std::collections::HashMap::new(),
            lcb_prog,
            lcb_res_loc,
            lcb_color_sampler,
            lcb_alpha_sampler,
            lcb_channel_loc,
            fill_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            fill_linear_gradient: None,
            fill_radial_gradient: None,
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
        self.fill_linear_gradient = None;
        self.fill_radial_gradient = None;
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
            fill_linear_gradient: self.fill_linear_gradient.clone(),
            fill_radial_gradient: self.fill_radial_gradient.clone(),
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
        self.fill_linear_gradient = saved.fill_linear_gradient;
        self.fill_radial_gradient = saved.fill_radial_gradient;
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
}

mod layers;
mod lcd;
mod primitives;

impl GlGfxCtx {
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
