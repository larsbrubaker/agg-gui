//! `GlGfxCtx` — a hardware-accelerated [`DrawCtx`] implementation for
//! WebGL2 / OpenGL via `glow`.
//!
//! # Pipeline
//!
//! ```text
//! Widget::paint(&mut dyn DrawCtx)
//!   │  path building (move_to/line_to/…) + shape helpers (rect/circle/…)
//!   │  fill() / stroke() → tess2 tessellation → vertex buffer
//!   │  fill_text() → shape_and_flatten_text → tess2 → vertex buffer
//!   ↓
//! GL draw calls (solid-colour GLSL ES 3.0 shader)
//! ```
//!
//! # Coordinate system
//!
//! All incoming coordinates are in **Y-up pixel space**: origin at the
//! bottom-left of the viewport, positive Y upward.  The transform stack
//! (save/translate/restore) maps widget-local → screen-space Y-up before
//! any point is stored.  The vertex shader converts screen-space pixels →
//! GL NDC with `ndc = (pos / resolution) * 2 - 1`.

use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::gl_renderer::tessellate_fill;
use agg_gui::text::{Font, TextMetrics, shape_and_flatten_text};
use agg_gui::CompOp;
use agg_gui::{LineCap, LineJoin};
use agg_gui::TransAffine;
use glow::HasContext;

// ---------------------------------------------------------------------------
// Shaders
// ---------------------------------------------------------------------------

const SOLID_VERT: &str = r#"#version 300 es
precision mediump float;
layout(location = 0) in vec2 a_pos;
uniform vec2 u_resolution;
void main() {
    vec2 ndc = (a_pos / u_resolution) * 2.0 - 1.0;
    gl_Position = vec4(ndc, 0.0, 1.0);
}
"#;

const SOLID_FRAG: &str = r#"#version 300 es
precision mediump float;
uniform vec4 u_color;
out vec4 frag_color;
void main() {
    frag_color = u_color;
}
"#;

// ---------------------------------------------------------------------------
// GlGfxCtx
// ---------------------------------------------------------------------------

/// A [`DrawCtx`] that renders via `glow` (WebGL2 or native GL).
///
/// Create once per frame (or share via mutable reference) and pass to
/// [`App::paint`].  After `paint` returns, call [`GlGfxCtx::flush`] to
/// submit any buffered draw calls.  In the current implementation draw calls
/// are submitted immediately in `fill()` / `stroke()`, so `flush` is a no-op
/// placeholder.
pub struct GlGfxCtx {
    gl: Rc<glow::Context>,
    viewport: (f32, f32),

    // GL resources for the solid-colour pipeline
    prog: glow::Program,
    vao:  glow::VertexArray,
    vbo:  glow::Buffer,
    res_loc:   Option<glow::UniformLocation>,
    color_loc: Option<glow::UniformLocation>,

    // Drawing state
    fill_color:   Color,
    stroke_color: Color,
    line_width:   f64,
    global_alpha: f64,

    // State stack: each entry holds a saved (transform, clip) pair.
    // The last entry is the current state.
    // clip: GL scissor rect (x, y_down, w, h), or None = no scissor.
    state_stack: Vec<(TransAffine, Option<[i32; 4]>)>,

    // Path builder — contours stored in screen-space Y-up pixels.
    contours:        Vec<Vec<[f32; 2]>>,
    current_contour: Vec<[f32; 2]>,
    pen:             [f64; 2],

    // Font
    font:      Option<Arc<Font>>,
    font_size: f64,
}

impl GlGfxCtx {
    /// Create a new `GlGfxCtx` backed by `gl`.
    ///
    /// # Safety
    /// `gl` must be a valid WebGL2 / OpenGL context.
    pub unsafe fn new(gl: Rc<glow::Context>, width: f32, height: f32) -> Self {
        let prog = compile_program(&gl, SOLID_VERT, SOLID_FRAG);
        let res_loc   = gl.get_uniform_location(prog, "u_resolution");
        let color_loc = gl.get_uniform_location(prog, "u_color");

        let vao = gl.create_vertex_array().unwrap();
        let vbo = gl.create_buffer().unwrap();

        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        // a_pos layout: vec2 (8 bytes per vertex)
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
        gl.enable_vertex_attrib_array(0);
        gl.bind_vertex_array(None);

        // Enable alpha blending for all draws.
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);

        Self {
            gl,
            viewport: (width, height),
            prog, vao, vbo,
            res_loc, color_loc,
            fill_color:   Color::rgba(0.0, 0.0, 0.0, 1.0),
            stroke_color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            line_width:   1.0,
            global_alpha: 1.0,
            state_stack:  vec![(TransAffine::new(), None)],
            contours:     Vec::new(),
            current_contour: Vec::new(),
            pen:          [0.0; 2],
            font:         None,
            font_size:    16.0,
        }
    }

    /// Resize the viewport (call when the canvas size changes).
    pub fn resize(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
    }

    // ---- internal helpers --------------------------------------------------

    fn ctm(&self) -> &TransAffine {
        &self.state_stack.last().expect("state stack never empty").0
    }

    fn current_clip(&self) -> Option<[i32; 4]> {
        self.state_stack.last().expect("state stack never empty").1
    }

    fn apply_scissor(&self) {
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
    fn transform_pt(&self, x: f64, y: f64) -> [f32; 2] {
        let (mut px, mut py) = (x, y);
        self.ctm().transform(&mut px, &mut py);
        [px as f32, py as f32]
    }

    /// Submit triangles with the given colour.
    ///
    /// `verts` is a flat slice of screen-space [f32;2] XY pairs.
    /// `indices` is a list of triangle vertex indices into `verts`.
    unsafe fn draw_triangles(&self, verts: &[[f32; 2]], indices: &[u32], color: Color) {
        if verts.is_empty() || indices.is_empty() { return; }

        let a = (color.a * self.global_alpha as f32).clamp(0.0, 1.0);
        let r = color.r;
        let g = color.g;
        let b = color.b;

        self.gl.use_program(Some(self.prog));
        if let Some(ref loc) = self.res_loc {
            self.gl.uniform_2_f32(Some(loc), self.viewport.0, self.viewport.1);
        }
        if let Some(ref loc) = self.color_loc {
            self.gl.uniform_4_f32(Some(loc), r, g, b, a);
        }

        self.gl.bind_vertex_array(Some(self.vao));
        self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
        self.gl.buffer_data_u8_slice(
            glow::ARRAY_BUFFER,
            bytemuck::cast_slice(verts),
            glow::STREAM_DRAW,
        );

        // Index buffer — create per-draw (simple but correct)
        let ibo = self.gl.create_buffer().unwrap();
        self.gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ibo));
        self.gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(indices),
            glow::STREAM_DRAW,
        );

        self.gl.draw_elements(glow::TRIANGLES, indices.len() as i32,
                              glow::UNSIGNED_INT, 0);

        self.gl.delete_buffer(ibo);
        self.gl.bind_vertex_array(None);
    }

    /// Tessellate all accumulated `contours` and draw as a fill.
    unsafe fn do_fill(&mut self) {
        let contours = std::mem::take(&mut self.contours);
        self.current_contour.clear();
        if contours.is_empty() { return; }

        if let Some((verts_flat, idx)) = tessellate_fill(&contours) {
            // verts_flat is interleaved [x0,y0,x1,y1,...] → convert to [[f32;2]]
            let verts: Vec<[f32; 2]> = verts_flat
                .chunks_exact(2)
                .map(|c| [c[0], c[1]])
                .collect();
            // Indices from tess2 are vertex numbers (not byte offsets)
            let color = self.fill_color;
            self.draw_triangles(&verts, &idx, color);
        }
    }

    /// Build stroke quads from contours and draw.
    unsafe fn do_stroke(&mut self) {
        let contours = std::mem::take(&mut self.contours);
        self.current_contour.clear();
        if contours.is_empty() { return; }

        let hw = (self.line_width * 0.5) as f32;
        let mut verts: Vec<[f32; 2]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for contour in &contours {
            if contour.len() < 2 { continue; }
            let n = contour.len();
            for i in 0..n {
                let a = contour[i];
                let b = contour[(i + 1) % n];
                if i + 1 == n && contour.first() != contour.last() {
                    // open path — skip wrap-around segment
                    break;
                }
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let len = (dx * dx + dy * dy).sqrt();
                if len < 1e-6 { continue; }
                let nx = -dy / len * hw;
                let ny =  dx / len * hw;

                let base = verts.len() as u32;
                verts.push([a[0] + nx, a[1] + ny]);
                verts.push([a[0] - nx, a[1] - ny]);
                verts.push([b[0] + nx, b[1] + ny]);
                verts.push([b[0] - nx, b[1] - ny]);
                indices.extend_from_slice(&[base, base+1, base+2, base+1, base+3, base+2]);
            }
        }

        let color = self.stroke_color;
        self.draw_triangles(&verts, &indices, color);
    }

    /// Flush the current contour into `contours`.
    fn flush_contour(&mut self) {
        if self.current_contour.len() >= 2 {
            let c = std::mem::take(&mut self.current_contour);
            self.contours.push(c);
        } else {
            self.current_contour.clear();
        }
    }

    /// Apply CTM to a Y-up position and push into the current contour.
    fn push_pt(&mut self, x: f64, y: f64) {
        let pt = self.transform_pt(x, y);
        self.current_contour.push(pt);
        self.pen = [x, y];
    }

    // Bézier flatteners --------------------------------------------------

    fn flatten_quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        let p0 = [self.pen[0] as f32, self.pen[1] as f32];
        let p1 = [cx as f32, cy as f32];
        let p2 = [x as f32, y as f32];
        subdivide_quad(p0, p1, p2, FLATNESS_SQ, &mut |px, py| {
            let pt = self.transform_pt(px as f64, py as f64);
            self.current_contour.push(pt);
        });
        let pt = self.transform_pt(x, y);
        self.current_contour.push(pt);
        self.pen = [x, y];
    }

    fn flatten_cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        let ctm = *self.ctm();
        let tfm = |lx: f64, ly: f64| -> [f32; 2] {
            let (mut px, mut py) = (lx, ly);
            ctm.transform(&mut px, &mut py);
            [px as f32, py as f32]
        };
        let sp0 = tfm(self.pen[0], self.pen[1]);
        let sp1 = tfm(cx1, cy1);
        let sp2 = tfm(cx2, cy2);
        let sp3 = tfm(x, y);
        let contour = &mut self.current_contour;
        subdivide_cubic_screen(sp0, sp1, sp2, sp3, FLATNESS_SQ, &mut |px, py| {
            contour.push([px, py]);
        });
        contour.push(sp3);
        self.pen = [x, y];
    }
}

const FLATNESS_SQ: f64 = 0.25; // 0.5px flatness

// ---------------------------------------------------------------------------
// DrawCtx impl
// ---------------------------------------------------------------------------

impl DrawCtx for GlGfxCtx {
    // ── State ────────────────────────────────────────────────────────────────

    fn set_fill_color(&mut self, c: Color) { self.fill_color = c; }
    fn set_stroke_color(&mut self, c: Color) { self.stroke_color = c; }
    fn set_line_width(&mut self, w: f64) { self.line_width = w; }
    fn set_line_join(&mut self, _: LineJoin) {}
    fn set_line_cap(&mut self, _: LineCap) {}
    fn set_blend_mode(&mut self, _: CompOp) {}
    fn set_global_alpha(&mut self, a: f64) { self.global_alpha = a; }

    // ── Font ─────────────────────────────────────────────────────────────────

    fn set_font(&mut self, font: Arc<Font>) { self.font = Some(font); }
    fn set_font_size(&mut self, size: f64) { self.font_size = size; }

    // ── Clipping ─────────────────────────────────────────────────────────────

    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        // Transform the clip rect corners through the CTM to screen space.
        let (mut x0, mut y0) = (x, y);
        let (mut x1, mut y1) = (x + w, y + h);
        self.ctm().transform(&mut x0, &mut y0);
        self.ctm().transform(&mut x1, &mut y1);
        let (lx, rx) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
        let (by, ty2) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
        // Y-up → GL Y-down: scissor y is measured from the bottom.
        let gl_x = lx.floor() as i32;
        let gl_y = (self.viewport.1 as f64 - ty2).floor() as i32;
        let gl_w = (rx - lx).ceil() as i32;
        let gl_h = (ty2 - by).ceil() as i32;
        let scissor = [gl_x, gl_y, gl_w, gl_h];
        self.state_stack.last_mut().unwrap().1 = Some(scissor);
        unsafe {
            self.gl.enable(glow::SCISSOR_TEST);
            self.gl.scissor(gl_x, gl_y, gl_w, gl_h);
        }
    }

    fn reset_clip(&mut self) {
        self.state_stack.last_mut().unwrap().1 = None;
        unsafe { self.gl.disable(glow::SCISSOR_TEST); }
    }

    // ── Clear ─────────────────────────────────────────────────────────────────

    fn clear(&mut self, color: Color) {
        unsafe {
            self.gl.clear_color(
                color.r as f32 / 255.0,
                color.g as f32 / 255.0,
                color.b as f32 / 255.0,
                color.a as f32 / 255.0,
            );
            self.gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    // ── Path building ────────────────────────────────────────────────────────

    fn begin_path(&mut self) {
        self.contours.clear();
        self.current_contour.clear();
    }

    fn move_to(&mut self, x: f64, y: f64) {
        self.flush_contour();
        self.push_pt(x, y);
    }

    fn line_to(&mut self, x: f64, y: f64) {
        self.push_pt(x, y);
    }

    fn cubic_to(&mut self, cx1: f64, cy1: f64, cx2: f64, cy2: f64, x: f64, y: f64) {
        self.flatten_cubic_to(cx1, cy1, cx2, cy2, x, y);
    }

    fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.flatten_quad_to(cx, cy, x, y);
    }

    fn arc_to(&mut self, cx: f64, cy: f64, r: f64, start_angle: f64, end_angle: f64, ccw: bool) {
        let mut da = end_angle - start_angle;
        if ccw && da > 0.0 { da -= std::f64::consts::TAU; }
        if !ccw && da < 0.0 { da += std::f64::consts::TAU; }
        let steps = ((da.abs() * r).abs().max(1.0) as usize).min(256).max(4);
        let step = da / steps as f64;
        for i in 0..=steps {
            let a = start_angle + step * i as f64;
            let px = cx + r * a.cos();
            let py = cy + r * a.sin();
            if i == 0 { self.flush_contour(); self.push_pt(px, py); }
            else { self.push_pt(px, py); }
        }
    }

    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        let ctm = *self.ctm();
        let scale = (ctm.sx * ctm.sx + ctm.shy * ctm.shy).sqrt();
        let segments = (std::f64::consts::TAU * r * scale).max(12.0).min(128.0) as usize;
        let contour: Vec<[f32; 2]> = (0..segments).map(|i| {
            let angle = i as f64 / segments as f64 * std::f64::consts::TAU;
            let lx = cx + r * angle.cos();
            let ly = cy + r * angle.sin();
            self.transform_pt(lx, ly)
        }).collect();
        if contour.len() >= 3 {
            self.contours.push(contour);
        }
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        // 4-point contour (CTM-transformed corners), CCW winding.
        let bl = self.transform_pt(x,     y);
        let br = self.transform_pt(x + w, y);
        let tr = self.transform_pt(x + w, y + h);
        let tl = self.transform_pt(x,     y + h);
        self.contours.push(vec![bl, br, tr, tl]);
    }

    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let r = r.min(w * 0.5).min(h * 0.5);
        let seg = 8usize;
        let mut contour: Vec<[f32; 2]> = Vec::with_capacity(seg * 4 + 4);
        use std::f64::consts::FRAC_PI_2;
        // Four corner arcs, CCW winding, starting from bottom-left corner.
        let corners: [(f64, f64, f64, f64); 4] = [
            (x + r,     y + r,      -FRAC_PI_2*2.0, -FRAC_PI_2),   // bottom-left
            (x + w - r, y + r,      -FRAC_PI_2,      0.0),          // bottom-right
            (x + w - r, y + h - r,   0.0,             FRAC_PI_2),   // top-right
            (x + r,     y + h - r,   FRAC_PI_2,       FRAC_PI_2*2.0),// top-left
        ];
        for &(cx2, cy2, start, end) in &corners {
            for i in 0..=seg {
                let t = i as f64 / seg as f64;
                let angle = start + t * (end - start);
                let lx = cx2 + r * angle.cos();
                let ly = cy2 + r * angle.sin();
                contour.push(self.transform_pt(lx, ly));
            }
        }
        if contour.len() >= 3 {
            self.contours.push(contour);
        }
    }

    fn close_path(&mut self) {
        // Close by adding the first point of the current contour.
        if let Some(&first) = self.current_contour.first() {
            self.current_contour.push(first);
        }
        self.flush_contour();
    }

    // ── Path drawing ─────────────────────────────────────────────────────────

    fn fill(&mut self) {
        self.flush_contour();
        unsafe { self.do_fill(); }
    }

    fn stroke(&mut self) {
        self.flush_contour();
        unsafe { self.do_stroke(); }
    }

    fn fill_and_stroke(&mut self) {
        self.flush_contour();
        // Save contours for both operations.
        let saved = self.contours.clone();
        unsafe { self.do_fill(); }
        self.contours = saved;
        unsafe { self.do_stroke(); }
    }

    // ── Text ─────────────────────────────────────────────────────────────────

    fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        let font = match self.font.clone() {
            Some(f) => f,
            None => return,
        };
        // shape_and_flatten_text returns points in local (x,y) coordinate space.
        // We then apply CTM to each point to get screen-space coords.
        let local_contours = shape_and_flatten_text(&font, text, self.font_size, x, y, 0.5);
        let screen_contours: Vec<Vec<[f32; 2]>> = local_contours
            .iter()
            .map(|c| c.iter().map(|&p| self.transform_pt(p[0] as f64, p[1] as f64)).collect())
            .collect();
        if let Some((verts_flat, idx)) = tessellate_fill(&screen_contours) {
            let verts: Vec<[f32; 2]> = verts_flat.chunks_exact(2)
                .map(|c| [c[0], c[1]])
                .collect();
            let color = self.fill_color;
            unsafe { self.draw_triangles(&verts, &idx, color); }
        }
    }

    fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {
        // GSV (Glyph-Stroke-Vector) font is AGG-specific; not available in GL path.
        // Silently ignore — this is only used in placeholder widgets.
    }

    fn measure_text(&self, text: &str) -> Option<TextMetrics> {
        let font = self.font.as_ref()?;
        // Delegate to the same measurement used in GfxCtx.
        Some(agg_gui::text::measure_text_metrics(font, text, self.font_size))
    }

    // ── Transform ────────────────────────────────────────────────────────────

    fn transform(&self) -> TransAffine {
        *self.ctm()
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
        self.state_stack.last_mut().unwrap().0
            .premultiply(&TransAffine::new_translation(tx, ty));
    }

    fn rotate(&mut self, radians: f64) {
        self.state_stack.last_mut().unwrap().0
            .premultiply(&TransAffine::new_rotation(radians));
    }

    fn scale(&mut self, sx: f64, sy: f64) {
        self.state_stack.last_mut().unwrap().0
            .premultiply(&TransAffine::new_scaling(sx, sy));
    }

    fn set_transform(&mut self, m: TransAffine) {
        self.state_stack.last_mut().unwrap().0 = m;
    }

    fn reset_transform(&mut self) {
        self.state_stack.last_mut().unwrap().0 = TransAffine::new();
    }
}

// ---------------------------------------------------------------------------
// Bézier flattening helpers
// ---------------------------------------------------------------------------

fn subdivide_quad<F: FnMut(f32, f32)>(
    p0: [f32; 2], p1: [f32; 2], p2: [f32; 2],
    flatness_sq: f64,
    emit: &mut F,
) {
    let mx = (p0[0] + 2.0 * p1[0] + p2[0]) * 0.25;
    let my = (p0[1] + 2.0 * p1[1] + p2[1]) * 0.25;
    let mid_x = (p0[0] + p2[0]) * 0.5;
    let mid_y = (p0[1] + p2[1]) * 0.5;
    let dx = (mx - mid_x) as f64;
    let dy = (my - mid_y) as f64;
    if dx * dx + dy * dy <= flatness_sq { return; }
    let q0 = [(p0[0]+p1[0])*0.5, (p0[1]+p1[1])*0.5];
    let q1 = [(p1[0]+p2[0])*0.5, (p1[1]+p2[1])*0.5];
    let mid = [(q0[0]+q1[0])*0.5, (q0[1]+q1[1])*0.5];
    subdivide_quad(p0, q0, mid, flatness_sq, emit);
    emit(mid[0], mid[1]);
    subdivide_quad(mid, q1, p2, flatness_sq, emit);
}

/// Cubic subdivision in screen space (points already CTM-transformed).
fn subdivide_cubic_screen<F: FnMut(f32, f32)>(
    p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2],
    flatness_sq: f64,
    emit: &mut F,
) {
    let ux = 3.0*p1[0] - 2.0*p0[0] - p3[0];
    let uy = 3.0*p1[1] - 2.0*p0[1] - p3[1];
    let vx = 3.0*p2[0] - 2.0*p3[0] - p0[0];
    let vy = 3.0*p2[1] - 2.0*p3[1] - p0[1];
    let u  = ux*ux + uy*uy;
    let v  = vx*vx + vy*vy;
    if (if u>v{u}else{v}) as f64 <= flatness_sq * 16.0 { return; }
    let q0 = [(p0[0]+p1[0])*0.5, (p0[1]+p1[1])*0.5];
    let q1 = [(p1[0]+p2[0])*0.5, (p1[1]+p2[1])*0.5];
    let q2 = [(p2[0]+p3[0])*0.5, (p2[1]+p3[1])*0.5];
    let r0 = [(q0[0]+q1[0])*0.5, (q0[1]+q1[1])*0.5];
    let r1 = [(q1[0]+q2[0])*0.5, (q1[1]+q2[1])*0.5];
    let mid = [(r0[0]+r1[0])*0.5, (r0[1]+r1[1])*0.5];
    subdivide_cubic_screen(p0, q0, r0, mid, flatness_sq, emit);
    emit(mid[0], mid[1]);
    subdivide_cubic_screen(mid, r1, q2, p3, flatness_sq, emit);
}

// ---------------------------------------------------------------------------
// GL helper
// ---------------------------------------------------------------------------

unsafe fn compile_program(gl: &glow::Context, vert_src: &str, frag_src: &str) -> glow::Program {
    let prog = gl.create_program().expect("create_program");
    for (src, kind) in [(vert_src, glow::VERTEX_SHADER), (frag_src, glow::FRAGMENT_SHADER)] {
        let sh = gl.create_shader(kind).unwrap();
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        assert!(gl.get_shader_compile_status(sh),
            "shader compile error: {}", gl.get_shader_info_log(sh));
        gl.attach_shader(prog, sh);
        gl.delete_shader(sh);
    }
    gl.link_program(prog);
    assert!(gl.get_program_link_status(prog),
        "program link error: {}", gl.get_program_info_log(prog));
    prog
}
