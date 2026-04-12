//! `GlCubeWidget` and `CubeGlRenderer` — rotating 3D cube via OpenGL.
//!
//! # Two-part design
//!
//! - **`GlCubeWidget`** lives inside the widget tree. During `paint()` it
//!   draws a dark placeholder in the AGG framebuffer and records the widget's
//!   framebuffer rect to `CUBE_SCREEN_RECT` (a thread_local).
//!
//! - **`CubeGlRenderer`** lives in `main`. After the AGG pass is uploaded to
//!   a GL texture, `main` calls `CubeGlRenderer::draw_gl()` with the rect
//!   captured by the widget. The renderer manages its own GL resources and
//!   rotation state.
//!
//! # Coordinate system
//!
//! `CUBE_SCREEN_RECT` is in **Y-up framebuffer** coordinates (AGG convention).
//! `draw_gl` converts to **Y-down GL viewport** coordinates.
//!
//! # Reference
//!
//! Cube geometry uses the same vertex-colour approach as the plan; tess2 is
//! used for the 2D GUI shapes (fills/strokes) — the cube's 8-vertex geometry
//! does not need tessellation.

use std::cell::Cell;

use agg_gui::{Color, Rect, Size};
use agg_gui::event::{Event, EventResult};
use agg_gui::gfx_ctx::GfxCtx;
use agg_gui::widget::Widget;
use glow::HasContext;

// ---------------------------------------------------------------------------
// Shared screen-rect channel (widget → render loop)
// ---------------------------------------------------------------------------

thread_local! {
    /// Set each frame by GlCubeWidget::paint(). Read by CubeGlRenderer::draw_gl().
    pub static CUBE_SCREEN_RECT: Cell<Rect> = Cell::new(Rect::default());
}

// ---------------------------------------------------------------------------
// GlCubeWidget — the Widget-tree half (placeholder + rect capture)
// ---------------------------------------------------------------------------

pub struct GlCubeWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
}

impl GlCubeWidget {
    pub fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new() }
    }
}

impl Widget for GlCubeWidget {
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size { available }

    fn paint(&mut self, ctx: &mut GfxCtx) {
        // Capture screen rect from the accumulated transform.
        let t = ctx.transform();
        CUBE_SCREEN_RECT.with(|r| r.set(Rect::new(
            t.tx, t.ty, self.bounds.width, self.bounds.height,
        )));

        // Dark placeholder (the GL cube will be painted on top after AGG upload).
        ctx.set_fill_color(Color::rgb(0.08, 0.08, 0.12));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // Subtle label
        ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 0.20));
        let cx = self.bounds.width * 0.5 - 8.0;
        let cy = self.bounds.height * 0.5 - 5.0;
        ctx.fill_text_gsv("3D", cx, cy, 10.0);
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ---------------------------------------------------------------------------
// CubeGlRenderer — the GL-draw half (lives in main, not in widget tree)
// ---------------------------------------------------------------------------

const CUBE_VERT: &str = r#"#version 330 core
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_color;
uniform mat4 u_mvp;
out vec3 v_color;
void main() {
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    v_color = a_color;
}
"#;

const CUBE_FRAG: &str = r#"#version 330 core
in vec3 v_color;
out vec4 frag_color;
void main() {
    frag_color = vec4(v_color, 1.0);
}
"#;

#[rustfmt::skip]
const CUBE_VERTS: &[f32] = &[
    // position            color (R, G, B)
    -1.0, -1.0, -1.0,    0.20, 0.28, 0.62,
     1.0, -1.0, -1.0,    0.27, 0.52, 0.92,
     1.0,  1.0, -1.0,    0.16, 0.36, 0.72,
    -1.0,  1.0, -1.0,    0.40, 0.60, 0.95,
    -1.0, -1.0,  1.0,    0.88, 0.45, 0.12,
     1.0, -1.0,  1.0,    0.95, 0.58, 0.20,
     1.0,  1.0,  1.0,    0.72, 0.36, 0.08,
    -1.0,  1.0,  1.0,    0.98, 0.68, 0.30,
];

#[rustfmt::skip]
const CUBE_IDX: &[u32] = &[
    0,1,2, 2,3,0,   // back
    4,5,6, 6,7,4,   // front
    0,4,7, 7,3,0,   // left
    1,5,6, 6,2,1,   // right
    3,2,6, 6,7,3,   // top
    0,1,5, 5,4,0,   // bottom
];

pub struct CubeGlRenderer {
    program:  glow::Program,
    vao:      glow::VertexArray,
    _vbo:     glow::Buffer,
    _ibo:     glow::Buffer,
    mvp_loc:  Option<glow::UniformLocation>,
    rotation: f32,   // degrees
}

impl CubeGlRenderer {
    /// Initialize GL resources. Must be called while a GL context is current.
    pub unsafe fn new(gl: &glow::Context) -> Self {
        let program = gl.create_program().expect("create_program");
        for (src, kind) in [
            (CUBE_VERT, glow::VERTEX_SHADER),
            (CUBE_FRAG, glow::FRAGMENT_SHADER),
        ] {
            let s = gl.create_shader(kind).unwrap();
            gl.shader_source(s, src);
            gl.compile_shader(s);
            assert!(gl.get_shader_compile_status(s), "{}", gl.get_shader_info_log(s));
            gl.attach_shader(program, s);
            gl.delete_shader(s);
        }
        gl.link_program(program);
        assert!(gl.get_program_link_status(program), "{}", gl.get_program_info_log(program));

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
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(
            1, 3, glow::FLOAT, false, stride,
            (3 * std::mem::size_of::<f32>()) as i32,
        );
        gl.enable_vertex_attrib_array(1);

        gl.bind_vertex_array(None);

        Self { program, vao, _vbo: vbo, _ibo: ibo, mvp_loc, rotation: 0.0 }
    }

    /// Draw the cube into the framebuffer area given by `fb_rect`.
    ///
    /// `fb_rect` — Y-up framebuffer coordinates.
    /// `viewport_h` — total framebuffer height in pixels.
    /// `full_w`, `full_h` — full viewport dimensions for restoring after.
    pub unsafe fn draw_gl(
        &mut self,
        gl:         &glow::Context,
        fb_rect:    Rect,
        viewport_h: f64,
        full_w:     i32,
        full_h:     i32,
    ) {
        if fb_rect.width < 1.0 || fb_rect.height < 1.0 { return; }

        // Y-up → GL (Y-down) conversion
        let gl_x = fb_rect.x as i32;
        let gl_y = (viewport_h - fb_rect.y - fb_rect.height) as i32;
        let gl_w = fb_rect.width as i32;
        let gl_h = fb_rect.height as i32;

        gl.viewport(gl_x, gl_y, gl_w, gl_h);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(gl_x, gl_y, gl_w, gl_h);

        // Only clear depth — colour comes from the AGG texture underneath.
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

        gl.draw_elements(glow::TRIANGLES, 36, glow::UNSIGNED_INT, 0);

        // Restore full viewport
        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::DEPTH_TEST);
        gl.bind_vertex_array(None);
        gl.viewport(0, 0, full_w, full_h);

        self.rotation = (self.rotation + 0.5) % 360.0;
    }
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
    let f = 1.0 / (fov_y * 0.5).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect, 0.0, 0.0,                   0.0,
        0.0,        f,   0.0,                   0.0,
        0.0,        0.0, (far + near) * nf,     -1.0,
        0.0,        0.0, 2.0 * far * near * nf,  0.0,
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
