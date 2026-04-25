//! Shared `GlCubeWidget` and `BarGridGlRenderer` — the demo's 3D
//! Animation widget.
//!
//! **This is the single source of truth.**  Both `demo-native` and
//! `demo-wasm` consume this module verbatim — the bar grid you see
//! running on the desktop binary and the WASM-on-page deployment are
//! produced by the *same* compiled code path.  The platform crates
//! contain only OS-shell glue (event loop, window creation, disk vs
//! `localStorage` for state, etc.) — never any demo content.
//!
//! # Two-part design
//!
//! - **`GlCubeWidget`** lives inside the widget tree.  During `paint()`
//!   it fills its rect with the active theme background and records its
//!   framebuffer rect to `CUBE_SCREEN_RECT` (thread_local).  The GL
//!   pass then renders the bar grid into that rect via the lazily-
//!   constructed `BarGridGlRenderer`.
//!
//! - **`BarGridGlRenderer`** does the GPU work: a single `BoxGeometry`
//!   drawn 128 times via instanced rendering (16 columns × 8 rows = 128
//!   bars in one draw call).  A vertex-shader sine wave drives Y
//!   displacement per instance based on grid position + time, so the
//!   surface looks like an undulating field.  Per-bar colour comes from
//!   a fragment-shader gradient over the (u, v) grid coords, with a
//!   brightness boost on wave peaks.  Palette derived from the active
//!   theme each frame so light/dark toggle recolours bars without a
//!   shader rebuild.
//!
//! # GLSL version
//!
//! Both target backends speak GLSL ES 3.0 (`#version 300 es`) — that's
//! the WebGL 2 baseline and also valid on desktop GL 3.3+.  Same shader
//! source compiles unchanged on either platform.
//!
//! # Time
//!
//! `web_time::Instant` is the wasm-safe replacement for stdlib's
//! `std::time::Instant`, which panics on `wasm32-unknown-unknown`
//! because the browser sandbox has no monotonic wall clock.  Works
//! transparently on native too — same call sites, identical semantics.

use std::cell::Cell;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::widget::Widget;
use agg_gui::{GlPaint, Rect, Size};
use glow::HasContext;

// ---------------------------------------------------------------------------
// Shared screen-rect channel (widget → render loop, legacy/debug use)
// ---------------------------------------------------------------------------

thread_local! {
    /// Set each frame by `GlCubeWidget::paint`.  External code that
    /// needs the widget's screen rect without being on the paint call
    /// stack reads this.
    pub static CUBE_SCREEN_RECT: Cell<Rect> = Cell::new(Rect::default());
}

// ---------------------------------------------------------------------------
// GlCubeWidget — widget-tree placeholder + GL paint dispatch
// ---------------------------------------------------------------------------

/// Widget that renders the bar-grid scene via `DrawCtx::gl_paint`.
///
/// On the GL path the bars appear inline at the correct painter-order
/// depth, so windows painted after it naturally overdraw it.  On the
/// software path `gl_paint` is a no-op and only the theme-coloured
/// placeholder rectangle is visible.
pub struct GlCubeWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Created lazily on the first `gl_paint()` call so no GL context
    /// is needed at widget construction time.
    renderer: Option<BarGridGlRenderer>,
}

impl Default for GlCubeWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl GlCubeWidget {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            renderer: None,
        }
    }
}

impl Widget for GlCubeWidget {
    fn type_name(&self) -> &'static str {
        "GlCubeWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        available
    }

    /// 3-D cube is a continuous animation — every frame advances the sine
    /// field.  When the cube is visible (enclosing Window / tab /
    /// CollapsingHeader paints it) this keeps the host loop rendering;
    /// when it's hidden, the tree-walk visibility gate short-circuits
    /// before reaching here, so the loop goes idle.
    fn needs_paint(&self) -> bool {
        true
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let t = ctx.transform();
        let screen_rect = Rect::new(t.tx, t.ty, self.bounds.width, self.bounds.height);
        CUBE_SCREEN_RECT.with(|r| r.set(screen_rect));

        // Theme-aware placeholder fill — `BarGridGlRenderer` only clears
        // depth in `draw_gl`, so anywhere the bar geometry doesn't cover
        // (gaps between bars, area above the wave field) shows this
        // fill through.  Using `window_fill` keeps the widget integrated
        // with the active theme without a shader rebuild.
        ctx.set_fill_color(ctx.visuals().window_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        ctx.gl_paint(screen_rect, self);
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Lazy-init GL painter: creates the renderer on first call, then draws.
impl GlPaint for GlCubeWidget {
    fn gl_paint(
        &mut self,
        gl: &dyn std::any::Any,
        screen_rect: Rect,
        full_w: i32,
        full_h: i32,
        parent_clip: Option<[i32; 4]>,
    ) {
        if let Some(gl_ctx) = gl.downcast_ref::<glow::Context>() {
            let renderer = self
                .renderer
                .get_or_insert_with(|| unsafe { BarGridGlRenderer::new(gl_ctx) });
            unsafe { renderer.draw_gl(gl_ctx, screen_rect, full_w, full_h, parent_clip) };
        }
    }
}

// ---------------------------------------------------------------------------
// BarGridGlRenderer — instanced bar grid (16 × 8 = 128 bars / 1 draw call)
// ---------------------------------------------------------------------------

const GRID_COLS: u32 = 16;
const GRID_ROWS: u32 = 8;

/// Bar geometry — unit-square box that rises from y=0 to y=1.  Origin
/// at the **base** so the vertex shader's Y scaling grows the bar
/// upward rather than expanding it from its centre.
const BAR_HALF: f32 = 0.45; // ⇒ 0.9 wide on a 1.0 grid pitch (gutter = 0.1)
const BAR_WAVE_SPEED: f64 = 1.4;

const BAR_VERT: &str = r#"#version 300 es
precision mediump float;
// ── Per-vertex (shared across all instances) ────────────────────────────
layout(location = 0) in vec3 a_pos;     // box vertex, base at y=0
layout(location = 1) in vec3 a_normal;  // face normal
// ── Per-instance ────────────────────────────────────────────────────────
layout(location = 2) in vec2 a_grid;    // (column, row), integer in [0,N)

uniform mat4  u_view_proj;
uniform float u_phase;
uniform vec2  u_grid_size;

out vec3  v_world_pos;
out vec3  v_normal;
out vec2  v_uv;
out float v_height;

void main() {
    const float freq  = 0.55;
    // Wave range: bars never collapse below `MAX_H * 0.05` so the
    // top face stays a measurable distance above the bottom face —
    // without this the two coplanar surfaces z-fight at minimum
    // height (visible as flicker / shimmer at the wave troughs).
    // 5% of max is small enough to read as "the bar bottomed out"
    // while keeping enough depth separation that the GPU's depth
    // test resolves cleanly.
    const float MAX_H = 2.10;
    const float MIN_H = MAX_H * 0.4;

    float wave_unit = sin(a_grid.x * freq + a_grid.y * freq + u_phase)
                      * 0.5 + 0.5;            // sin in [-1, 1]  →  [0, 1]
    float height    = mix(MIN_H, MAX_H, wave_unit);

    vec3 local = vec3(a_pos.x, a_pos.y * height, a_pos.z);
    vec3 world = local + vec3(
        a_grid.x - (u_grid_size.x - 1.0) * 0.5,
        0.0,
        a_grid.y - (u_grid_size.y - 1.0) * 0.5
    );

    gl_Position = u_view_proj * vec4(world, 1.0);
    v_world_pos = world;
    v_normal    = a_normal;
    v_uv        = a_grid / max(u_grid_size - vec2(1.0), vec2(1.0));
    // Normalise to [0, 1] over the full wave range so the fragment
    // shader's peak-brightening `pow(v_height, 2.0)` weights peaks
    // (not the absolute world Y, which would skew when MIN_H grows).
    v_height    = wave_unit;
}
"#;

const BAR_FRAG: &str = r#"#version 300 es
precision mediump float;
in vec3  v_world_pos;
in vec3  v_normal;
in vec2  v_uv;
in float v_height;
out vec4 frag_color;

uniform vec3 u_light_dir;
uniform vec3 u_col_left;
uniform vec3 u_col_right;
uniform vec3 u_col_accent;
uniform vec3 u_peak_color;

void main() {
    vec3 base = mix(u_col_left, u_col_right, v_uv.x);
    base = mix(base, u_col_accent, v_uv.y * 0.35);

    base = mix(base, u_peak_color, pow(v_height, 2.0) * 0.25);

    float n_dot_l = max(dot(normalize(v_normal), u_light_dir), 0.0);
    float lit     = 0.45 + 0.55 * n_dot_l;

    frag_color = vec4(base * lit, 1.0);
}
"#;

/// 24 vertices (4 per face) so each face carries its own flat normal —
/// gives clean shaded edges without the smoothing artifacts a shared-
/// vertex box would produce under per-vertex lighting.
fn bar_box_verts() -> Vec<f32> {
    let h = BAR_HALF;
    let face = |verts: [[f32; 3]; 4], n: [f32; 3]| -> Vec<f32> {
        let mut out = Vec::with_capacity(24);
        for v in verts {
            out.extend_from_slice(&[v[0], v[1], v[2], n[0], n[1], n[2]]);
        }
        out
    };
    let mut v = Vec::with_capacity(24 * 6);
    v.extend(face(
        [[-h, 1.0, -h], [h, 1.0, -h], [h, 1.0, h], [-h, 1.0, h]],
        [0.0, 1.0, 0.0],
    ));
    v.extend(face(
        [[-h, 0.0, h], [h, 0.0, h], [h, 0.0, -h], [-h, 0.0, -h]],
        [0.0, -1.0, 0.0],
    ));
    v.extend(face(
        [[-h, 0.0, h], [h, 0.0, h], [h, 1.0, h], [-h, 1.0, h]],
        [0.0, 0.0, 1.0],
    ));
    v.extend(face(
        [[h, 0.0, -h], [-h, 0.0, -h], [-h, 1.0, -h], [h, 1.0, -h]],
        [0.0, 0.0, -1.0],
    ));
    v.extend(face(
        [[h, 0.0, h], [h, 0.0, -h], [h, 1.0, -h], [h, 1.0, h]],
        [1.0, 0.0, 0.0],
    ));
    v.extend(face(
        [[-h, 0.0, -h], [-h, 0.0, h], [-h, 1.0, h], [-h, 1.0, -h]],
        [-1.0, 0.0, 0.0],
    ));
    v
}

/// 6 faces × 2 triangles × 3 indices = 36.  All under u16 max (24
/// verts), so indices fit in `u16` — avoids the WebGL
/// `OES_element_index_uint` extension and works identically on desktop.
fn bar_box_indices() -> Vec<u16> {
    let mut idx = Vec::with_capacity(36);
    for face in 0..6u16 {
        let b = face * 4;
        idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    idx
}

/// One vec2 per instance — (col, row).  16 × 8 = 128 entries.
fn bar_instance_data() -> Vec<f32> {
    let mut out = Vec::with_capacity((GRID_COLS * GRID_ROWS) as usize * 2);
    for row in 0..GRID_ROWS {
        for col in 0..GRID_COLS {
            out.push(col as f32);
            out.push(row as f32);
        }
    }
    out
}

fn bar_wave_phase(elapsed_secs: f64) -> f32 {
    (elapsed_secs * BAR_WAVE_SPEED).rem_euclid(std::f64::consts::TAU) as f32
}

pub struct BarGridGlRenderer {
    program: glow::Program,
    vao: glow::VertexArray,
    _vbo: glow::Buffer,
    _ibo: glow::Buffer,
    _instance_vbo: glow::Buffer,
    vp_loc: Option<glow::UniformLocation>,
    phase_loc: Option<glow::UniformLocation>,
    grid_size_loc: Option<glow::UniformLocation>,
    light_dir_loc: Option<glow::UniformLocation>,
    col_left_loc: Option<glow::UniformLocation>,
    col_right_loc: Option<glow::UniformLocation>,
    col_accent_loc: Option<glow::UniformLocation>,
    peak_color_loc: Option<glow::UniformLocation>,
    /// `web_time::Instant` so this code compiles + runs on both native
    /// and `wasm32-unknown-unknown` from a single source.
    start: web_time::Instant,
}

impl BarGridGlRenderer {
    /// Initialise GL resources.  Must be called while a GL context is current.
    pub unsafe fn new(gl: &glow::Context) -> Self {
        let program = compile_program(gl, BAR_VERT, BAR_FRAG);

        let vp_loc = gl.get_uniform_location(program, "u_view_proj");
        let phase_loc = gl.get_uniform_location(program, "u_phase");
        let grid_size_loc = gl.get_uniform_location(program, "u_grid_size");
        let light_dir_loc = gl.get_uniform_location(program, "u_light_dir");
        let col_left_loc = gl.get_uniform_location(program, "u_col_left");
        let col_right_loc = gl.get_uniform_location(program, "u_col_right");
        let col_accent_loc = gl.get_uniform_location(program, "u_col_accent");
        let peak_color_loc = gl.get_uniform_location(program, "u_peak_color");

        let vao = gl.create_vertex_array().unwrap();
        let vbo = gl.create_buffer().unwrap();
        let ibo = gl.create_buffer().unwrap();

        gl.bind_vertex_array(Some(vao));

        let verts = bar_box_verts();
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&verts),
            glow::STATIC_DRAW,
        );

        let indices = bar_box_indices();
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
        gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(&indices),
            glow::STATIC_DRAW,
        );

        let vert_stride = (6 * std::mem::size_of::<f32>()) as i32;
        gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, vert_stride, 0);
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(
            1,
            3,
            glow::FLOAT,
            false,
            vert_stride,
            (3 * std::mem::size_of::<f32>()) as i32,
        );
        gl.enable_vertex_attrib_array(1);

        let instance_vbo = gl.create_buffer().unwrap();
        let instances = bar_instance_data();
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(instance_vbo));
        gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(&instances),
            glow::STATIC_DRAW,
        );
        let inst_stride = (2 * std::mem::size_of::<f32>()) as i32;
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, inst_stride, 0);
        gl.enable_vertex_attrib_array(2);
        // Divisor = 1 → attribute advances once per instance, not per
        // vertex.  Both desktop GL 3.3+ and WebGL 2 support this
        // natively, no extension dance.
        gl.vertex_attrib_divisor(2, 1);

        gl.bind_vertex_array(None);

        Self {
            program,
            vao,
            _vbo: vbo,
            _ibo: ibo,
            _instance_vbo: instance_vbo,
            vp_loc,
            phase_loc,
            grid_size_loc,
            light_dir_loc,
            col_left_loc,
            col_right_loc,
            col_accent_loc,
            peak_color_loc,
            start: web_time::Instant::now(),
        }
    }

    /// Draw the bar grid into the framebuffer area given by `fb_rect`.
    /// `fb_rect` — Y-up framebuffer coordinates.  `full_w`, `full_h` —
    /// full viewport dimensions for restoring after.
    pub unsafe fn draw_gl(
        &mut self,
        gl: &glow::Context,
        fb_rect: Rect,
        full_w: i32,
        full_h: i32,
        parent_clip: Option<[i32; 4]>,
    ) {
        if fb_rect.width < 1.0 || fb_rect.height < 1.0 {
            return;
        }

        let gl_x = fb_rect.x as i32;
        let gl_y = fb_rect.y as i32;
        let gl_w = fb_rect.width as i32;
        let gl_h = fb_rect.height as i32;

        // Intersect widget scissor with the parent clip so collapsed
        // windows (and any other parent clip) correctly hide GL content.
        let [sx, sy, sw, sh] = if let Some([px, py, pw, ph]) = parent_clip {
            let x1 = gl_x.max(px);
            let y1 = gl_y.max(py);
            let x2 = (gl_x + gl_w).min(px + pw);
            let y2 = (gl_y + gl_h).min(py + ph);
            [x1, y1, (x2 - x1).max(0), (y2 - y1).max(0)]
        } else {
            [gl_x, gl_y, gl_w, gl_h]
        };
        if sw <= 0 || sh <= 0 {
            return;
        }

        gl.viewport(gl_x, gl_y, gl_w, gl_h);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(sx, sy, sw, sh);

        // Only clear depth — colour comes from the AGG/GL content beneath.
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LESS);
        gl.clear(glow::DEPTH_BUFFER_BIT);

        gl.use_program(Some(self.program));
        gl.bind_vertex_array(Some(self.vao));

        // ── Camera ───────────────────────────────────────────────────────
        // High angle, looking down at the grid centre, offset to −X so
        // the field reads as a perspective sweep rather than top-down.
        let aspect = gl_w as f32 / gl_h.max(1) as f32;
        let proj = perspective(35_f32.to_radians(), aspect, 0.5, 100.0);
        let view = look_at([-7.0, 8.5, 11.0], [0.0, 0.5, 0.0], [0.0, 1.0, 0.0]);
        let view_proj = mat4_mul(proj, view);

        if let Some(loc) = self.vp_loc.as_ref() {
            gl.uniform_matrix_4_f32_slice(Some(loc), false, &view_proj);
        }
        if let Some(loc) = self.phase_loc.as_ref() {
            gl.uniform_1_f32(
                Some(loc),
                bar_wave_phase(self.start.elapsed().as_secs_f64()),
            );
        }
        if let Some(loc) = self.grid_size_loc.as_ref() {
            gl.uniform_2_f32(Some(loc), GRID_COLS as f32, GRID_ROWS as f32);
        }
        if let Some(loc) = self.light_dir_loc.as_ref() {
            let l = normalize3([0.55, 0.85, 0.45]);
            gl.uniform_3_f32(Some(loc), l[0], l[1], l[2]);
        }

        // Theme-driven palette — read every frame so a light/dark
        // toggle recolours the bars on the next paint.
        let palette = bar_palette_for_theme();
        if let Some(loc) = self.col_left_loc.as_ref() {
            gl.uniform_3_f32(Some(loc), palette.left[0], palette.left[1], palette.left[2]);
        }
        if let Some(loc) = self.col_right_loc.as_ref() {
            gl.uniform_3_f32(
                Some(loc),
                palette.right[0],
                palette.right[1],
                palette.right[2],
            );
        }
        if let Some(loc) = self.col_accent_loc.as_ref() {
            gl.uniform_3_f32(
                Some(loc),
                palette.accent[0],
                palette.accent[1],
                palette.accent[2],
            );
        }
        if let Some(loc) = self.peak_color_loc.as_ref() {
            gl.uniform_3_f32(Some(loc), palette.peak[0], palette.peak[1], palette.peak[2]);
        }

        // Single instanced draw call: 36 indices × 128 instances.
        let instance_count = (GRID_COLS * GRID_ROWS) as i32;
        gl.draw_elements_instanced(glow::TRIANGLES, 36, glow::UNSIGNED_SHORT, 0, instance_count);

        gl.disable(glow::SCISSOR_TEST);
        gl.disable(glow::DEPTH_TEST);
        gl.bind_vertex_array(None);
        gl.viewport(0, 0, full_w, full_h);
    }
}

/// Implement `GlPaint` so external callers can hand a renderer to
/// `ctx.gl_paint()` without knowing anything about `glow` — the
/// downcast happens here.
impl GlPaint for BarGridGlRenderer {
    fn gl_paint(
        &mut self,
        gl: &dyn std::any::Any,
        screen_rect: Rect,
        full_w: i32,
        full_h: i32,
        parent_clip: Option<[i32; 4]>,
    ) {
        if let Some(gl) = gl.downcast_ref::<glow::Context>() {
            unsafe { self.draw_gl(gl, screen_rect, full_w, full_h, parent_clip) };
        }
    }
}

// ---------------------------------------------------------------------------
// Shader compilation helper
// ---------------------------------------------------------------------------

unsafe fn compile_program(gl: &glow::Context, vert_src: &str, frag_src: &str) -> glow::Program {
    let program = gl.create_program().expect("create_program");
    for (src, kind) in [
        (vert_src, glow::VERTEX_SHADER),
        (frag_src, glow::FRAGMENT_SHADER),
    ] {
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
// Theme-driven palette
// ---------------------------------------------------------------------------

struct BarPalette {
    left: [f32; 3],
    right: [f32; 3],
    accent: [f32; 3],
    peak: [f32; 3],
}

/// Pick a gradient palette based on the active theme.  Two presets:
/// **dark** uses vivid colours that read well against a dark canvas
/// and brightens peaks toward white; **light** uses richer (more
/// saturated) colours and darkens peaks toward the theme's text colour
/// so the highlight stays legible against a light background.
/// Detection by background-colour luminance — works for any future
/// theme additions without an enum match.
fn bar_palette_for_theme() -> BarPalette {
    let v = agg_gui::current_visuals();
    let bg = v.bg_color;
    let lum = 0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b;
    let dark = lum < 0.5;

    if dark {
        BarPalette {
            left: [0.18, 0.55, 0.95],
            right: [0.92, 0.32, 0.62],
            accent: [1.00, 0.78, 0.30],
            peak: [1.00, 1.00, 1.00],
        }
    } else {
        BarPalette {
            left: [0.10, 0.42, 0.85],
            right: [0.78, 0.18, 0.45],
            accent: [0.95, 0.55, 0.10],
            peak: [v.text_color.r, v.text_color.g, v.text_color.b],
        }
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
            out[col * 4 + row] = a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3];
        }
    }
    out
}

fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    let nf = 1.0 / (near - far);
    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        (far + near) * nf,
        -1.0,
        0.0,
        0.0,
        2.0 * far * near * nf,
        0.0,
    ]
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalize3(sub3(target, eye));
    let s = normalize3(cross3(f, up));
    let u = cross3(s, f);
    [
        s[0],
        u[0],
        -f[0],
        0.0,
        s[1],
        u[1],
        -f[1],
        0.0,
        s[2],
        u[2],
        -f[2],
        0.0,
        -dot3(s, eye),
        -dot3(u, eye),
        dot3(f, eye),
        1.0,
    ]
}

#[inline]
fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
#[inline]
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / len, v[1] / len, v[2] / len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_wave_phase_stays_bounded_for_long_running_animation() {
        let short_elapsed = 12.345;
        let cycles = 100_000.0;
        let period = std::f64::consts::TAU / BAR_WAVE_SPEED;
        let long_elapsed = short_elapsed + period * cycles;

        let short_phase = bar_wave_phase(short_elapsed);
        let long_phase = bar_wave_phase(long_elapsed);

        assert!(long_phase >= 0.0);
        assert!(long_phase < std::f32::consts::TAU);
        assert!(
            (short_phase - long_phase).abs() < 0.0001,
            "phase drifted after many cycles: short={short_phase}, long={long_phase}"
        );
    }
}
