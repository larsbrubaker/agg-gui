//! WebGL2 resources for Phase D (WASM).
//!
//! # Two-part design (mirrors the native cube_widget approach)
//!
//! - **`GlCubeWidget`** lives inside the widget tree. `paint()` draws a dark
//!   placeholder into the AGG framebuffer and records its framebuffer rect to
//!   `CUBE_SCREEN_RECT` (thread_local) for the GL renderer to consume.
//!
//! - **`GlPresenter`** uploads the AGG RGBA framebuffer as a WebGL2 texture
//!   and blits it to the canvas with a fullscreen quad (GLSL ES 3.0).
//!
//! - **`CubeGlRenderer`** draws a rotating 3D cube into the viewport sub-rect
//!   that corresponds to `CUBE_SCREEN_RECT`, on top of the blit.
//!
//! - **`GlState`** owns the `glow::Context` and both renderers; stored in a
//!   `thread_local!` in `lib.rs`.
//!
//! # UV / flip convention
//!
//! AGG renders Y-up. `Framebuffer::pixels_flipped()` returns rows top-to-bottom
//! (as HTML canvas ImageData expects). When uploaded to a WebGL2 texture without
//! UNPACK_FLIP_Y_WEBGL, `data[0]` lands at `t=0` (GL's bottom). To display it
//! correctly, the fullscreen quad uses `UV.y = 0` at the top of the screen
//! (NDC `y = +1`) so that `t=0` (data row 0 = top of image) maps to the top.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::{Color, GlPaint, Rect, Size};
use agg_gui::event::{Event, EventResult};
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::widget::Widget;
use glow::HasContext;

// ---------------------------------------------------------------------------
// Shared screen-rect channel (GlCubeWidget → CubeGlRenderer)
// ---------------------------------------------------------------------------

thread_local! {
    /// Written each frame by `GlCubeWidget::paint`.
    pub static CUBE_SCREEN_RECT: Cell<Rect> = Cell::new(Rect::default());
}

// ---------------------------------------------------------------------------
// GlCubeWidget — widget-tree placeholder
// ---------------------------------------------------------------------------

/// Widget that renders a rotating 3-D cube via `DrawCtx::gl_paint`.
///
/// The `CubeGlRenderer` is created lazily on the first `gl_paint()` call so no
/// GL context is needed at widget construction time.  On the software path
/// `gl_paint` is a no-op; only the dark placeholder rectangle is visible.
pub struct GlCubeWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    /// Created lazily on first GL paint call.
    renderer: Option<CubeGlRenderer>,
}

impl GlCubeWidget {
    pub fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), renderer: None }
    }
}

impl Widget for GlCubeWidget {
    fn type_name(&self) -> &'static str { "GlCubeWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, available: Size) -> Size { available }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let t = ctx.transform();
        let screen_rect = Rect::new(t.tx, t.ty, self.bounds.width, self.bounds.height);
        CUBE_SCREEN_RECT.with(|r| r.set(screen_rect));

        // 2-D placeholder — visible on software path; on the GL path the cube
        // renders inline on top of it via ctx.gl_paint() below.
        ctx.set_fill_color(Color::rgb(0.08, 0.08, 0.12));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        ctx.gl_paint(screen_rect, self);
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Lazy-init GL painter: creates the renderer on first call, then draws.
impl GlPaint for GlCubeWidget {
    fn gl_paint(
        &mut self,
        gl:          &dyn std::any::Any,
        screen_rect: Rect,
        full_w:      i32,
        full_h:      i32,
        parent_clip: Option<[i32; 4]>,
    ) {
        if let Some(gl_ctx) = gl.downcast_ref::<glow::Context>() {
            let renderer = self.renderer.get_or_insert_with(|| {
                unsafe { CubeGlRenderer::new(gl_ctx) }
            });
            unsafe { renderer.draw_gl(gl_ctx, screen_rect, full_w, full_h, parent_clip) };
        }
    }
}

// ---------------------------------------------------------------------------
// Blit shaders — AGG framebuffer → fullscreen quad (GLSL ES 3.0)
// ---------------------------------------------------------------------------

const BLIT_VERT: &str = r#"#version 300 es
precision mediump float;
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_uv;
out vec2 v_uv;
void main() {
    gl_Position = vec4(a_pos, 0.0, 1.0);
    v_uv = a_uv;
}
"#;

const BLIT_FRAG: &str = r#"#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_tex;
out vec4 frag_color;
void main() {
    frag_color = texture(u_tex, v_uv);
}
"#;

// Fullscreen quad: (NDC x, NDC y, UV u, UV v)
// UV.y = 0 at NDC y = +1 (top) because data[0] lands at GL t=0 (bottom-of-texture)
// when uploaded without UNPACK_FLIP_Y, so we invert the mapping via the quad.
#[rustfmt::skip]
const QUAD_VERTS: &[f32] = &[
    // x     y     u    v
    -1.0,  1.0,  0.0, 0.0,   // top-left
     1.0,  1.0,  1.0, 0.0,   // top-right
    -1.0, -1.0,  0.0, 1.0,   // bottom-left
     1.0, -1.0,  1.0, 1.0,   // bottom-right
];

const QUAD_IDX: &[u16] = &[0, 2, 1,  1, 2, 3];

// ---------------------------------------------------------------------------
// GlPresenter — AGG framebuffer → WebGL2 texture → fullscreen quad
// ---------------------------------------------------------------------------

pub struct GlPresenter {
    program: glow::Program,
    vao:     glow::VertexArray,
    _vbo:    glow::Buffer,
    _ibo:    glow::Buffer,
    texture: glow::Texture,
    tex_loc: Option<glow::UniformLocation>,
}

impl GlPresenter {
    pub unsafe fn new(gl: &glow::Context) -> Self {
        let program = compile_program(gl, BLIT_VERT, BLIT_FRAG);
        let tex_loc = gl.get_uniform_location(program, "u_tex");

        let vao = gl.create_vertex_array().unwrap();
        let vbo = gl.create_buffer().unwrap();
        let ibo = gl.create_buffer().unwrap();

        gl.bind_vertex_array(Some(vao));

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(QUAD_VERTS),
            glow::STATIC_DRAW,
        );

        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
        gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(QUAD_IDX),
            glow::STATIC_DRAW,
        );

        let stride = (4 * std::mem::size_of::<f32>()) as i32;
        // a_pos (vec2)
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(0);
        // a_uv (vec2)
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride,
            (2 * std::mem::size_of::<f32>()) as i32);
        gl.enable_vertex_attrib_array(1);

        gl.bind_vertex_array(None);

        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        gl.bind_texture(glow::TEXTURE_2D, None);

        Self { program, vao, _vbo: vbo, _ibo: ibo, texture, tex_loc }
    }

    /// Upload `pixels` (RGBA, top-to-bottom from `pixels_flipped()`) as a
    /// WebGL2 texture and draw it over the whole canvas.
    pub unsafe fn present(
        &mut self,
        gl: &glow::Context,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) {
        gl.viewport(0, 0, width as i32, height as i32);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::BLEND);
        gl.clear_color(0.0, 0.0, 0.0, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT);

        // Upload RGBA bytes to texture (re-allocate each frame; fine for a demo).
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA as i32,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            Some(pixels),
        );

        // Draw fullscreen quad.
        gl.use_program(Some(self.program));
        gl.uniform_1_i32(self.tex_loc.as_ref(), 0);
        gl.bind_vertex_array(Some(self.vao));
        gl.draw_elements(glow::TRIANGLES, 6, glow::UNSIGNED_SHORT, 0);
        gl.bind_vertex_array(None);
        gl.bind_texture(glow::TEXTURE_2D, None);
    }
}

// ---------------------------------------------------------------------------
// Cube shaders (GLSL ES 3.0)
// ---------------------------------------------------------------------------

const CUBE_VERT: &str = r#"#version 300 es
precision mediump float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_color;
uniform mat4 u_mvp;
out vec3 v_color;
void main() {
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    v_color = a_color;
}
"#;

const CUBE_FRAG: &str = r#"#version 300 es
precision mediump float;
in vec3 v_color;
out vec4 frag_color;
void main() {
    frag_color = vec4(v_color, 1.0);
}
"#;

#[rustfmt::skip]
const CUBE_VERTS: &[f32] = &[
    // position             color (R, G, B)
    -1.0, -1.0, -1.0,    0.20, 0.28, 0.62,
     1.0, -1.0, -1.0,    0.27, 0.52, 0.92,
     1.0,  1.0, -1.0,    0.16, 0.36, 0.72,
    -1.0,  1.0, -1.0,    0.40, 0.60, 0.95,
    -1.0, -1.0,  1.0,    0.88, 0.45, 0.12,
     1.0, -1.0,  1.0,    0.95, 0.58, 0.20,
     1.0,  1.0,  1.0,    0.72, 0.36, 0.08,
    -1.0,  1.0,  1.0,    0.98, 0.68, 0.30,
];

// u16 indices — all 8 vertices fit in u16, avoids OES_element_index_uint.
#[rustfmt::skip]
const CUBE_IDX: &[u16] = &[
    0,1,2, 2,3,0,   // back
    4,5,6, 6,7,4,   // front
    0,4,7, 7,3,0,   // left
    1,5,6, 6,2,1,   // right
    3,2,6, 6,7,3,   // top
    0,1,5, 5,4,0,   // bottom
];

// ---------------------------------------------------------------------------
// CubeGlRenderer
// ---------------------------------------------------------------------------

pub struct CubeGlRenderer {
    program:  glow::Program,
    vao:      glow::VertexArray,
    _vbo:     glow::Buffer,
    _ibo:     glow::Buffer,
    mvp_loc:  Option<glow::UniformLocation>,
    rotation: f32,
}

impl CubeGlRenderer {
    pub unsafe fn new(gl: &glow::Context) -> Self {
        let program = compile_program(gl, CUBE_VERT, CUBE_FRAG);
        let mvp_loc = gl.get_uniform_location(program, "u_mvp");

        let vao = gl.create_vertex_array().unwrap();
        let vbo = gl.create_buffer().unwrap();
        let ibo = gl.create_buffer().unwrap();

        gl.bind_vertex_array(Some(vao));

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(CUBE_VERTS),
            glow::STATIC_DRAW,
        );

        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
        gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(CUBE_IDX),
            glow::STATIC_DRAW,
        );

        let stride = (6 * std::mem::size_of::<f32>()) as i32;
        // a_pos (vec3)
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(0);
        // a_color (vec3)
        gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride,
            (3 * std::mem::size_of::<f32>()) as i32);
        gl.enable_vertex_attrib_array(1);

        gl.bind_vertex_array(None);

        Self { program, vao, _vbo: vbo, _ibo: ibo, mvp_loc, rotation: 0.0 }
    }

    /// Draw the cube into `fb_rect` (Y-up framebuffer coordinates).
    ///
    /// Must be called *after* `GlPresenter::present()` has already written the
    /// AGG blit to the default framebuffer (the cube is drawn on top of it).
    pub unsafe fn draw_gl(
        &mut self,
        gl:          &glow::Context,
        fb_rect:     Rect,
        full_w:      i32,
        full_h:      i32,
        parent_clip: Option<[i32; 4]>,
    ) {
        if fb_rect.width < 1.0 || fb_rect.height < 1.0 { return; }

        // GL viewport/scissor use window coordinates: Y=0 at BOTTOM-LEFT.
        // Our Y-up screen coords match directly — no flip needed.
        let gl_x = fb_rect.x as i32;
        let gl_y = fb_rect.y as i32;
        let gl_w = fb_rect.width  as i32;
        let gl_h = fb_rect.height as i32;

        // Intersect with the parent framework clip so collapsed/clipped windows
        // correctly hide this GL content.
        let [sx, sy, sw, sh] = if let Some([px, py, pw, ph]) = parent_clip {
            let x1 = gl_x.max(px);
            let y1 = gl_y.max(py);
            let x2 = (gl_x + gl_w).min(px + pw);
            let y2 = (gl_y + gl_h).min(py + ph);
            [x1, y1, (x2 - x1).max(0), (y2 - y1).max(0)]
        } else {
            [gl_x, gl_y, gl_w, gl_h]
        };
        if sw <= 0 || sh <= 0 { return; }

        gl.viewport(gl_x, gl_y, gl_w, gl_h);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(sx, sy, sw, sh);

        // Clear only depth — colour comes from the AGG texture drawn by GlPresenter.
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.clear(glow::DEPTH_BUFFER_BIT);

        gl.use_program(Some(self.program));
        gl.bind_vertex_array(Some(self.vao));

        let aspect = gl_w as f32 / gl_h.max(1) as f32;
        let proj  = perspective(60_f32.to_radians(), aspect, 0.1, 100.0);
        let view  = translate_mat4([0.0, 0.0, -4.0]);
        let model = mat4_mul(
            rotate_y(self.rotation.to_radians()),
            rotate_x((self.rotation * 0.4).to_radians()),
        );
        let mvp = mat4_mul(proj, mat4_mul(view, model));

        if let Some(loc) = self.mvp_loc.as_ref() {
            gl.uniform_matrix_4_f32_slice(Some(loc), false, &mvp);
        }

        gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0);

        // Restore full viewport and clean up.
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::DEPTH_TEST);
        gl.bind_vertex_array(None);
        gl.viewport(0, 0, full_w, full_h);

        self.rotation = (self.rotation + 0.5) % 360.0;
    }
}

// ---------------------------------------------------------------------------
// GlState — owns the WebGL context
// ---------------------------------------------------------------------------

pub struct GlState {
    gl:        Rc<glow::Context>,
    presenter: GlPresenter,
}

impl GlState {
    pub unsafe fn new(gl: glow::Context) -> Self {
        let gl = Rc::new(gl);
        let presenter = GlPresenter::new(&gl);
        Self { gl, presenter }
    }

    /// Reference-counted clone of the GL context (cheap Rc increment).
    pub fn gl_rc(&self) -> Rc<glow::Context> {
        Rc::clone(&self.gl)
    }

    /// Legacy full render pass (AGG texture blit).  Kept for reference.
    #[allow(dead_code)]
    pub unsafe fn render_legacy(
        &mut self,
        pixels: &[u8],
        width:  u32,
        height: u32,
    ) {
        self.presenter.present(&self.gl, pixels, width, height);
    }
}

// ---------------------------------------------------------------------------
// Shared GL helpers
// ---------------------------------------------------------------------------

unsafe fn compile_program(gl: &glow::Context, vert_src: &str, frag_src: &str) -> glow::Program {
    let program = gl.create_program().expect("create_program");
    for (src, kind) in [(vert_src, glow::VERTEX_SHADER), (frag_src, glow::FRAGMENT_SHADER)] {
        let shader = gl.create_shader(kind).unwrap();
        gl.shader_source(shader, src);
        gl.compile_shader(shader);
        assert!(
            gl.get_shader_compile_status(shader),
            "shader compile failed: {}",
            gl.get_shader_info_log(shader),
        );
        gl.attach_shader(program, shader);
        gl.delete_shader(shader);
    }
    gl.link_program(program);
    assert!(
        gl.get_program_link_status(program),
        "program link failed: {}",
        gl.get_program_info_log(program),
    );
    program
}

// ---------------------------------------------------------------------------
// Math helpers (column-major 4×4, OpenGL convention)
// ---------------------------------------------------------------------------

type Mat4 = [f32; 16];

fn mat4_mul(a: Mat4, b: Mat4) -> Mat4 {
    let mut out = [0f32; 16];
    for row in 0..4 {
        for col in 0..4 {
            out[col * 4 + row] =
                a[0 * 4 + row] * b[col * 4]
              + a[1 * 4 + row] * b[col * 4 + 1]
              + a[2 * 4 + row] * b[col * 4 + 2]
              + a[3 * 4 + row] * b[col * 4 + 3];
        }
    }
    out
}

fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f  = 1.0 / (fov_y * 0.5).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect, 0.0, 0.0,                    0.0,
        0.0,        f,   0.0,                    0.0,
        0.0,        0.0, (far + near) * nf,      -1.0,
        0.0,        0.0, 2.0 * far * near * nf,   0.0,
    ]
}

fn translate_mat4([tx, ty, tz]: [f32; 3]) -> Mat4 {
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        tx,  ty,  tz,  1.0,
    ]
}

fn rotate_y(a: f32) -> Mat4 {
    let (s, c) = a.sin_cos();
    [c, 0.0, -s, 0.0,  0.0, 1.0, 0.0, 0.0,  s, 0.0, c, 0.0,  0.0, 0.0, 0.0, 1.0]
}

fn rotate_x(a: f32) -> Mat4 {
    let (s, c) = a.sin_cos();
    [1.0, 0.0, 0.0, 0.0,  0.0, c, s, 0.0,  0.0, -s, c, 0.0,  0.0, 0.0, 0.0, 1.0]
}
