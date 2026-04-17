//! `GlGfxCtx` — a hardware-accelerated [`DrawCtx`] implementation for
//! WebGL2 / OpenGL via `glow`.
//!
//! This crate is a **rendering harness only** — it wires up GL resources,
//! the event loop, and frame presentation. All demo/UI code belongs in
//! `demo-ui`; platform entry-points (`demo-native`, `demo-wasm`) and this
//! crate should contain no widget or layout logic.
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

pub mod frame;
pub use frame::{begin_frame, sync_inspector, render_app_frame};

use std::rc::Rc;
use std::sync::{Arc, Weak};

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::gl_renderer::{GlyphCache, tessellate_fill};
use agg_gui::geometry::Rect;
use agg_gui::text::{Font, TextMetrics, shape_glyphs};
use agg_gui::CompOp;
use agg_gui::{LineCap, LineJoin};
use agg_gui::TransAffine;
use glow::HasContext;

// ---------------------------------------------------------------------------
// Shaders
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
const SOLID_VERT: &str = "#version 300 es\nprecision mediump float;\nlayout(location=0)in vec2 a_pos;uniform vec2 u_resolution;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);}";
#[cfg(not(target_arch = "wasm32"))]
const SOLID_VERT: &str = "#version 330 core\nlayout(location=0)in vec2 a_pos;uniform vec2 u_resolution;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);}";

#[cfg(target_arch = "wasm32")]
const SOLID_FRAG: &str = "#version 300 es\nprecision mediump float;\nuniform vec4 u_color;out vec4 frag_color;void main(){frag_color=u_color;}";
#[cfg(not(target_arch = "wasm32"))]
const SOLID_FRAG: &str = "#version 330 core\nuniform vec4 u_color;out vec4 frag_color;void main(){frag_color=u_color;}";

// ── Textured-quad pipeline (used by draw_image_rgba) ────────────────────────
//
// Same screen-space → NDC math as the solid pipeline, with an extra `a_uv`
// attribute and a single texture sampler binding.

#[cfg(target_arch = "wasm32")]
const TEX_VERT: &str = "#version 300 es\nprecision mediump float;\nlayout(location=0)in vec2 a_pos;layout(location=1)in vec2 a_uv;uniform vec2 u_resolution;out vec2 v_uv;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_uv=a_uv;}";
#[cfg(not(target_arch = "wasm32"))]
const TEX_VERT: &str = "#version 330 core\nlayout(location=0)in vec2 a_pos;layout(location=1)in vec2 a_uv;uniform vec2 u_resolution;out vec2 v_uv;void main(){vec2 ndc=(a_pos/u_resolution)*2.0-1.0;gl_Position=vec4(ndc,0.0,1.0);v_uv=a_uv;}";

#[cfg(target_arch = "wasm32")]
const TEX_FRAG: &str = "#version 300 es\nprecision mediump float;\nin vec2 v_uv;uniform sampler2D u_tex;out vec4 frag_color;void main(){frag_color=texture(u_tex,v_uv);}";
#[cfg(not(target_arch = "wasm32"))]
const TEX_FRAG: &str = "#version 330 core\nin vec2 v_uv;uniform sampler2D u_tex;out vec4 frag_color;void main(){frag_color=texture(u_tex,v_uv);}";

// ---------------------------------------------------------------------------
// GlGfxCtx
// ---------------------------------------------------------------------------

/// One entry in the Arc-keyed GL texture cache.  The `Weak` serves as the
/// liveness sentinel: when all strong refs to the underlying `Vec<u8>` have
/// been dropped (typically because the L1 pixel cache evicted its entry),
/// `weak.upgrade()` returns `None` and the next sweep deletes the texture.
struct ArcTextureEntry {
    weak:    Weak<Vec<u8>>,
    texture: glow::Texture,
    w:       u32,
    h:       u32,
}

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

    // GL resources for the solid-colour pipeline (created once, reused every frame)
    prog: glow::Program,
    vao:  glow::VertexArray,
    vbo:  glow::Buffer,
    ibo:  glow::Buffer,     // persistent index buffer — no per-draw alloc
    res_loc:   Option<glow::UniformLocation>,
    color_loc: Option<glow::UniformLocation>,

    // Textured-quad pipeline (draw_image_rgba — markdown images, screenshots,
    // AGG-rasterised Label backbuffers).
    tex_prog: glow::Program,
    tex_vao:  glow::VertexArray,
    tex_vbo:  glow::Buffer,
    tex_res_loc: Option<glow::UniformLocation>,
    tex_sampler_loc: Option<glow::UniformLocation>,

    // Texture cache keyed on (ptr, len, w, h, head/tail byte hash).  Used by
    // the generic `draw_image_rgba(&[u8], …)` path (markdown images, screenshot
    // display, image widgets).  LRU eviction keeps memory bounded.
    texture_cache:       std::collections::HashMap<u64, (glow::Texture, u32, u32)>,
    texture_cache_order: std::collections::VecDeque<u64>,

    // Arc-pointer-keyed texture cache for `draw_image_rgba_arc` — the hot path
    // for `Label` backbuffers (which now live in the crate-level `image_cache`
    // as `Arc<Vec<u8>>`).  Holds a `Weak<Vec<u8>>` per entry; when the Arc is
    // dropped by its owner (the L1 pixel cache, via LRU eviction) the `Weak`
    // fails to upgrade and the next sweep deletes the GL texture.  This is the
    // Rust equivalent of MatterCAD's `ConditionalWeakTable<byte[], ImageTexturePlugin>`
    // finalizer → deferred-delete pattern.
    arc_texture_cache: std::collections::HashMap<usize, ArcTextureEntry>,

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

    // Glyph vertex cache — survives frame resets, populated on first use.
    glyph_cache: GlyphCache,
}

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
        let res_loc   = gl.get_uniform_location(prog, "u_resolution");
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

        // ── Textured-quad pipeline ─────────────────────────────────────────
        let tex_prog = compile_program(&gl, TEX_VERT, TEX_FRAG)
            .expect("tex shader compile/link");
        let tex_res_loc     = gl.get_uniform_location(tex_prog, "u_resolution");
        let tex_sampler_loc = gl.get_uniform_location(tex_prog, "u_tex");

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

        Self {
            gl,
            viewport: (width, height),
            prog, vao, vbo, ibo,
            res_loc, color_loc,
            tex_prog, tex_vao, tex_vbo,
            tex_res_loc, tex_sampler_loc,
            texture_cache:       std::collections::HashMap::new(),
            texture_cache_order: std::collections::VecDeque::new(),
            arc_texture_cache:   std::collections::HashMap::new(),
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
            glyph_cache:  GlyphCache::new(),
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
                0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut buf),
            );
        }
        // Flip vertically: GL origin is bottom-left, PNG top-left.
        let stride = (w * 4) as usize;
        let mut flipped = vec![0u8; total];
        for y in 0..(h as usize) {
            let src_off = y * stride;
            let dst_off = (h as usize - 1 - y) * stride;
            flipped[dst_off..dst_off + stride]
                .copy_from_slice(&buf[src_off..src_off + stride]);
        }
        (flipped, w as u32, h as u32)
    }

    /// Reset drawing state for a new frame.  Does NOT recreate GL resources.
    pub fn reset(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
        self.fill_color   = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.stroke_color = Color::rgba(0.0, 0.0, 0.0, 1.0);
        self.line_width   = 1.0;
        self.global_alpha = 1.0;
        self.state_stack  = vec![(TransAffine::new(), None)];
        self.contours.clear();
        self.current_contour.clear();
        self.pen          = [0.0; 2];
        self.font         = None;
        self.font_size    = 16.0;
        // Disable any lingering scissor from the previous frame.
        unsafe { self.gl.disable(glow::SCISSOR_TEST); }
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
    /// `verts` is a slice of screen-space [f32;2] XY pairs.
    /// `indices` is a list of triangle vertex indices into `verts`.
    unsafe fn draw_triangles(&self, verts: &[[f32; 2]], indices: &[u32], color: Color) {
        if verts.is_empty() || indices.is_empty() { return; }

        let a = (color.a * self.global_alpha as f32).clamp(0.0, 1.0);

        self.gl.use_program(Some(self.prog));
        if let Some(ref loc) = self.res_loc {
            self.gl.uniform_2_f32(Some(loc), self.viewport.0, self.viewport.1);
        }
        if let Some(ref loc) = self.color_loc {
            self.gl.uniform_4_f32(Some(loc), color.r, color.g, color.b, a);
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
        self.gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(self.ibo));
        self.gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            bytemuck::cast_slice(indices),
            glow::STREAM_DRAW,
        );

        self.gl.draw_elements(glow::TRIANGLES, indices.len() as i32,
                              glow::UNSIGNED_INT, 0);

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
        let (verts, indices) = build_stroke_quads(&contours, hw);
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
        let [gl_x, gl_y, gl_w, gl_h] = compute_gl_scissor(lx, by, rx, ty2);

        // Intersect with the existing scissor so parent clips constrain children.
        // (Replacing outright lets children escape their parent's clip region.)
        let [ix, iy, iw, ih] = if let Some([ex, ey, ew, eh]) = self.current_clip() {
            let nx1 = gl_x.max(ex);
            let ny1 = gl_y.max(ey);
            let nx2 = gl_x.saturating_add(gl_w).min(ex.saturating_add(ew));
            let ny2 = gl_y.saturating_add(gl_h).min(ey.saturating_add(eh));
            [nx1, ny1, nx2.saturating_sub(nx1).max(0), ny2.saturating_sub(ny1).max(0)]
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
        unsafe { self.gl.disable(glow::SCISSOR_TEST); }
    }

    // ── Clear ─────────────────────────────────────────────────────────────────

    fn clear(&mut self, color: Color) {
        unsafe {
            // Color fields are already [0, 1] f32 — no conversion needed.
            self.gl.clear_color(color.r, color.g, color.b, color.a);
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
        let mut contour: Vec<[f32; 2]> = (0..segments).map(|i| {
            let angle = i as f64 / segments as f64 * std::f64::consts::TAU;
            let lx = cx + r * angle.cos();
            let ly = cy + r * angle.sin();
            self.transform_pt(lx, ly)
        }).collect();
        if contour.len() >= 3 {
            // Close the contour so do_stroke draws the segment that joins the
            // last arc point back to the first (otherwise the circle has a gap).
            let first = contour[0];
            contour.push(first);
            self.contours.push(contour);
        }
    }

    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        // 5-point closed contour (CTM-transformed corners + repeated first),
        // CCW winding.  The closing point ensures do_stroke draws all four
        // sides including the left edge (the tl→bl wrap-around segment).
        let bl = self.transform_pt(x,     y);
        let br = self.transform_pt(x + w, y);
        let tr = self.transform_pt(x + w, y + h);
        let tl = self.transform_pt(x,     y + h);
        self.contours.push(vec![bl, br, tr, tl, bl]);
    }

    fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let r = r.min(w * 0.5).min(h * 0.5);
        let seg = 8usize;
        let mut contour: Vec<[f32; 2]> = Vec::with_capacity(seg * 4 + 5);
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
            // Close the contour so do_stroke draws the left-side segment
            // that joins the last arc point back to the starting point.
            let first = contour[0];
            contour.push(first);
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

        // Shape the text string to get per-glyph IDs and advances.
        // Rustybuzz shaping is cheap relative to tessellation.
        let shaped    = shape_glyphs(&font, text, self.font_size);
        let font_size = self.font_size;
        // Snapshot the CTM so we can apply it inside the loop without holding
        // an immutable borrow of `self` while `glyph_cache` is mutably borrowed.
        let ctm = *self.ctm();

        let mut all_verts: Vec<[f32; 2]> = Vec::new();
        let mut all_idx:   Vec<u32>      = Vec::new();
        let mut pen_x = x;

        for glyph in &shaped {
            // Glyph origin in widget-local pixel space (before CTM).
            let gx = pen_x + glyph.x_offset;
            let gy = y     + glyph.y_offset;

            // Use the fallback font for outline lookup when the glyph was
            // resolved from it — glyph_id is an index into that font's table.
            let render_font = glyph.fallback_font.as_deref().unwrap_or(&font);

            if let Some(cached) = self.glyph_cache.get_or_insert(render_font, glyph.glyph_id, font_size) {
                // Offset each cached glyph-local vert by the glyph's screen
                // position, then apply the CTM to get screen-space pixels.
                // This is correct for any affine CTM including rotation/scale.
                let base = all_verts.len() as u32;
                for &[vx, vy] in &cached.verts {
                    let (mut px, mut py) = (gx + vx as f64, gy + vy as f64);
                    ctm.transform(&mut px, &mut py);
                    all_verts.push([px as f32, py as f32]);
                }
                all_idx.extend(cached.indices.iter().map(|&i| i + base));
            }

            pen_x += glyph.x_advance;
        }

        if !all_verts.is_empty() {
            let color = self.fill_color;
            unsafe { self.draw_triangles(&all_verts, &all_idx, color); }
        }
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
    fn has_image_blit(&self) -> bool { true }

    fn draw_image_rgba(
        &mut self,
        data:  &[u8],
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        if img_w == 0 || img_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 { return; }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 { return; }

        // Honour whatever CTM the caller has set — sub-pixel positions are
        // legitimate (smooth scrolling, animation).  Callers that need
        // pixel-perfect 1:1 blits (e.g. `Label` backbuffers, the pixel-
        // alignment test) must explicitly call `ctx.snap_to_pixel()` first.
        let bl = self.transform_pt(dst_x,         dst_y);
        let br = self.transform_pt(dst_x + dst_w, dst_y);
        let tr = self.transform_pt(dst_x + dst_w, dst_y + dst_h);
        let tl = self.transform_pt(dst_x,         dst_y + dst_h);
        let verts: [f32; 24] = [
            bl[0], bl[1], 0.0, 1.0,
            br[0], br[1], 1.0, 1.0,
            tr[0], tr[1], 1.0, 0.0,
            bl[0], bl[1], 0.0, 1.0,
            tr[0], tr[1], 1.0, 0.0,
            tl[0], tl[1], 0.0, 0.0,
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
                    if let Some(pos) = self.texture_cache_order.iter()
                        .position(|&k| k == key)
                    {
                        self.texture_cache_order.remove(pos);
                    }
                    self.texture_cache_order.push_back(key);
                    t
                }
                None => {
                    let tex = gl.create_texture().expect("create texture");
                    gl.active_texture(glow::TEXTURE0);
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                    gl.tex_image_2d(
                        glow::TEXTURE_2D, 0, glow::RGBA as i32,
                        img_w as i32, img_h as i32, 0,
                        glow::RGBA, glow::UNSIGNED_BYTE, Some(data),
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
                        } else { break; }
                    }
                    tex
                }
            };

            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.use_program(Some(self.tex_prog));
            gl.uniform_2_f32(
                self.tex_res_loc.as_ref(),
                self.viewport.0, self.viewport.1,
            );
            gl.uniform_1_i32(self.tex_sampler_loc.as_ref(), 0);
            gl.bind_vertex_array(Some(self.tex_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.tex_vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(&verts),
                glow::DYNAMIC_DRAW,
            );
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
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
        data:  &Arc<Vec<u8>>,
        img_w: u32,
        img_h: u32,
        dst_x: f64,
        dst_y: f64,
        dst_w: f64,
        dst_h: f64,
    ) {
        if img_w == 0 || img_h == 0 || dst_w <= 0.0 || dst_h <= 0.0 { return; }
        if data.len() < (img_w as usize) * (img_h as usize) * 4 { return; }

        // Honour the caller's CTM — no implicit snapping.  Callers that need
        // pixel-perfect 1:1 blits call `ctx.snap_to_pixel()` before the draw.
        let bl = self.transform_pt(dst_x,         dst_y);
        let br = self.transform_pt(dst_x + dst_w, dst_y);
        let tr = self.transform_pt(dst_x + dst_w, dst_y + dst_h);
        let tl = self.transform_pt(dst_x,         dst_y + dst_h);
        let verts: [f32; 24] = [
            bl[0], bl[1], 0.0, 1.0,
            br[0], br[1], 1.0, 1.0,
            tr[0], tr[1], 1.0, 0.0,
            bl[0], bl[1], 0.0, 1.0,
            tr[0], tr[1], 1.0, 0.0,
            tl[0], tl[1], 0.0, 0.0,
        ];

        let key = Arc::as_ptr(data) as *const u8 as usize;

        unsafe {
            let gl = Rc::clone(&self.gl);

            // Sweep dead entries — one entry per dead weak ref.  O(n) but the
            // cache is bounded by the L1 LRU cap, and we only remove; no
            // heavy work.  Batching all GL deletes in one frame keeps GL
            // driver chatter low.
            let dead_keys: Vec<usize> = self.arc_texture_cache.iter()
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
            let existing = self.arc_texture_cache.get(&key).and_then(|e| {
                match e.weak.upgrade() {
                    Some(a) if Arc::ptr_eq(&a, data) && e.w == img_w && e.h == img_h
                        => Some(e.texture),
                    _   => None,
                }
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
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                    gl.tex_image_2d(
                        glow::TEXTURE_2D, 0, glow::RGBA as i32,
                        img_w as i32, img_h as i32, 0,
                        glow::RGBA, glow::UNSIGNED_BYTE, Some(data.as_slice()),
                    );
                    self.arc_texture_cache.insert(key, ArcTextureEntry {
                        weak:    Arc::downgrade(data),
                        texture: tex,
                        w:       img_w,
                        h:       img_h,
                    });
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
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.bind_vertex_array(None);
        }
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
// Glyph helper
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// GL helper
// ---------------------------------------------------------------------------

unsafe fn compile_program(
    gl: &glow::Context,
    vert_src: &str,
    frag_src: &str,
) -> Result<glow::Program, String> {
    let prog = gl.create_program().map_err(|e| format!("create_program: {e}"))?;
    for (src, kind) in [(vert_src, glow::VERTEX_SHADER), (frag_src, glow::FRAGMENT_SHADER)] {
        let sh = gl.create_shader(kind).map_err(|e| format!("create_shader: {e}"))?;
        gl.shader_source(sh, src);
        gl.compile_shader(sh);
        if !gl.get_shader_compile_status(sh) {
            let log = gl.get_shader_info_log(sh);
            gl.delete_shader(sh);
            gl.delete_program(prog);
            return Err(format!("shader compile error: {log}"));
        }
        gl.attach_shader(prog, sh);
        gl.delete_shader(sh);
    }
    gl.link_program(prog);
    if !gl.get_program_link_status(prog) {
        let log = gl.get_program_info_log(prog);
        gl.delete_program(prog);
        return Err(format!("program link error: {log}"));
    }
    Ok(prog)
}

// ---------------------------------------------------------------------------
// Shared frame overlays (identical on native and WASM)
// ---------------------------------------------------------------------------

/// Compute a cache key for an RGBA image slice.  Blends pointer, length,
/// dimensions, and first/last 8 bytes so a freed-and-reused pointer with
/// fresh content produces a different key.  Cheap: no full-buffer hash.
fn texture_key(data: &[u8], w: u32, h: u32) -> u64 {
    let mut k: u64 = 0xcbf29ce484222325;
    let mix = |acc: u64, v: u64| -> u64 {
        acc.wrapping_mul(0x100000001b3).wrapping_add(v)
    };
    k = mix(k, data.as_ptr() as usize as u64);
    k = mix(k, data.len() as u64);
    k = mix(k, w as u64);
    k = mix(k, h as u64);
    if data.len() >= 16 {
        for &b in &data[..8]                    { k = mix(k, b as u64); }
        for &b in &data[data.len() - 8..]       { k = mix(k, b as u64); }
    } else {
        for &b in data                          { k = mix(k, b as u64); }
    }
    k
}

/// Draw the inspector hover overlay: teal fill + inset stroke + size label.
///
/// Called after every `App::paint` on both native and WASM so the Chrome-style
/// widget highlight is identical on both platforms.
pub fn draw_hover_overlay(ctx: &mut GlGfxCtx, rect: Rect) {
    if rect.width < 1.0 || rect.height < 1.0 { return; }
    let sw   = 1.5_f64;
    let half = sw * 0.5;
    // Teal fill — full widget bounds.
    ctx.set_fill_color(Color::rgba(0.05, 0.65, 0.85, 0.18));
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.fill();
    // Teal border — inset by half stroke-width so the outer edge never falls
    // below x=0 / y=0 (which would be clipped by the GL viewport).
    ctx.set_stroke_color(Color::rgba(0.05, 0.65, 0.85, 0.80));
    ctx.set_line_width(sw);
    ctx.begin_path();
    ctx.rect(
        rect.x + half,
        rect.y + half,
        (rect.width  - sw).max(0.0),
        (rect.height - sw).max(0.0),
    );
    ctx.stroke();
    // Size label
    let label = format!("{:.0} × {:.0}", rect.width, rect.height);
    ctx.set_fill_color(Color::rgba(0.05, 0.65, 0.85, 1.00));
    ctx.fill_text_gsv(&label, rect.x + 2.0, rect.y + rect.height + 2.0, 9.0);
}

/// Draw a "WxH  X.Xms" status bar in the bottom-left corner of the viewport.
///
/// `frame_ms` is the render time of the *previous* frame (so the display does
/// not include its own drawing cost).  Both native and WASM use this function
/// to keep the status overlay visually identical.
pub fn draw_status_overlay(
    ctx:      &mut GlGfxCtx,
    font:     Arc<Font>,
    w:        u32,
    h:        u32,
    frame_ms: f64,
) {
    let status = format!("{}×{}   {:.1}ms", w, h, frame_ms);
    ctx.set_font(font);
    ctx.set_font_size(11.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.30));
    ctx.fill_text(&status, 12.0, 6.0);
}

// ---------------------------------------------------------------------------
// Stroke helpers
// ---------------------------------------------------------------------------

/// Expand `contours` into stroke quads (two triangles per segment) with the
/// given half-width.
///
/// Contours with `first == last` are **closed**: all segments are drawn,
/// including the wrap-around from the last interior point back to the first.
/// Contours with `first != last` are **open**: the wrap-around segment is
/// skipped.  Shape helpers (`rect`, `rounded_rect`, `circle`) produce closed
/// contours so that every side is always stroked.
fn build_stroke_quads(
    contours: &[Vec<[f32; 2]>],
    hw: f32,
) -> (Vec<[f32; 2]>, Vec<u32>) {
    let mut verts: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for contour in contours {
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

    (verts, indices)
}

// ---------------------------------------------------------------------------
// Scissor helpers
// ---------------------------------------------------------------------------

/// Convert a Y-up screen-space bounding box to a GL scissor rectangle.
///
/// `gl.scissor(x, y, w, h)` uses window coordinates where y=0 is the
/// **bottom** of the framebuffer — identical to Y-up screen space.  So the
/// GL scissor `y` is simply `by` (the bottom of the clip rect in Y-up).
///
/// A common mistake is to compute `viewport_height − top` (a Y-down
/// conversion), which shifts the scissor downward by `top − bottom` pixels
/// and clips the top rows of any widget painted near the top of a clipped
/// region.
fn compute_gl_scissor(lx: f64, by: f64, rx: f64, ty2: f64) -> [i32; 4] {
    // Clamp before casting: STRETCH-anchored children inside a ScrollView receive
    // f64::MAX/2 as available height during the measure pass, which would overflow
    // i32 when fed into GL scissor calls.
    const LO: f64 = i32::MIN as f64;
    const HI: f64 = i32::MAX as f64;
    let gl_x = lx.floor().clamp(LO, HI) as i32;
    let gl_y = by.floor().clamp(LO, HI) as i32;
    let gl_w = (rx - lx).ceil().clamp(0.0, HI) as i32;
    let gl_h = (ty2 - by).ceil().clamp(0.0, HI) as i32;
    [gl_x, gl_y, gl_w, gl_h]
}

#[cfg(test)]
mod tests {
    use super::{build_stroke_quads, compute_gl_scissor};

    /// Before the fix, `rect()` / `rounded_rect()` built open contours.
    /// `build_stroke_quads` skips the wrap-around for open paths, so the left
    /// side (last→first segment) was never emitted.  This test documents the
    /// broken behaviour so it fails before the fix and passes after.
    #[test]
    fn test_stroke_open_rect_missing_left_side() {
        // 4-point open rect: first != last → wrap-around (left side) is skipped.
        let contour = vec![
            [0.0f32,  0.0f32],   // bl
            [10.0,    0.0],       // br
            [10.0,    10.0],      // tr
            [0.0,     10.0],      // tl — tl→bl (left side) will be skipped!
        ];
        let (verts, _) = build_stroke_quads(&[contour], 0.5);
        let segments = verts.len() / 4;
        assert_eq!(segments, 3, "open rect produces only 3 segments (left side missing)");
    }

    /// After closing the contour (first == last), all four sides are drawn.
    #[test]
    fn test_stroke_closed_rect_has_all_four_sides() {
        // 5-point closed rect: last point repeats first → wrap-around runs.
        let contour = vec![
            [0.0f32,  0.0f32],   // bl
            [10.0,    0.0],       // br
            [10.0,    10.0],      // tr
            [0.0,     10.0],      // tl
            [0.0,     0.0],       // bl repeated — closes the path
        ];
        let (verts, _) = build_stroke_quads(&[contour], 0.5);
        let segments = verts.len() / 4;
        assert_eq!(segments, 4, "closed rect must produce 4 segments (all sides)");
    }

    /// The inspector tree area spans Y-up [184, 650] in a 720-tall viewport.
    /// The GL scissor must cover exactly that band (y=184 from bottom, h=466).
    /// Before the fix, `viewport_height − top = 720 − 650 = 70` was used as
    /// gl_y, placing the scissor 114 px too low and clipping the top rows of
    /// the tree (Stack, TabView, Container, Buttons), leaving a gray band.
    #[test]
    fn test_scissor_y_uses_y_up_bottom_not_y_down_top() {
        // Clip rect [184, 650] in screen Y-up (inspector tree area, 720-px viewport).
        let [_gl_x, gl_y, _gl_w, gl_h] = compute_gl_scissor(0.0, 184.0, 320.0, 650.0);
        assert_eq!(gl_y, 184, "gl_y must equal the Y-up bottom of the clip, not viewport_h − top");
        assert_eq!(gl_h, 466);
    }

    /// Row 0 of the tree (Stack widget) sits at screen Y-up [630, 650].
    /// The scissor must include this band so the row is visible.
    #[test]
    fn test_scissor_covers_top_tree_rows() {
        let [_, gl_y, _, gl_h] = compute_gl_scissor(0.0, 184.0, 320.0, 650.0);
        let scissor_top = gl_y + gl_h; // highest Y covered (Y-up)
        assert!(
            scissor_top >= 650,
            "scissor top ({scissor_top}) must reach y=650 to include top tree rows"
        );
    }
}
