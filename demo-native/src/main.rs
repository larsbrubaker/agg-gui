//! Native WGL demo for agg-gui — Phase 3.
//!
//! Renders the Phase 3 demo scene (text rendering) via AGG → Framebuffer →
//! GL texture → full-screen quad. The framebuffer uses bottom-up (Y-up) row
//! order which matches OpenGL's texture layout, so no Y-flip is needed at
//! upload time.

use std::num::NonZeroU32;
use std::sync::Arc;

use agg_gui::{CompOp, Font, Framebuffer, GfxCtx};

const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{GlSurface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use glow::HasContext;
use raw_window_handle::HasWindowHandle;
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowAttributes;

// ---------------------------------------------------------------------------
// GL shaders — full-screen quad using gl_VertexID (no VBO required)
// ---------------------------------------------------------------------------

const VERT_SHADER: &str = r#"#version 330 core
out vec2 v_tex_coord;
void main() {
    // Triangle strip covering [-1, 1]^2 in clip space.
    // Vertex IDs 0–3 produce the four corners:
    //   0: (-1,-1)  1: ( 1,-1)  2: (-1, 1)  3: ( 1, 1)
    float x = float((gl_VertexID & 1) * 2) - 1.0;
    float y = float((gl_VertexID >> 1) * 2) - 1.0;
    gl_Position = vec4(x, y, 0.0, 1.0);
    // UV (0,0) = bottom-left, matching our Y-up framebuffer layout.
    // No flip needed — GL textures are also Y-up (row 0 = bottom).
    v_tex_coord = vec2((x + 1.0) * 0.5, (y + 1.0) * 0.5);
}
"#;

const FRAG_SHADER: &str = r#"#version 330 core
in vec2 v_tex_coord;
out vec4 frag_color;
uniform sampler2D u_texture;
void main() {
    frag_color = texture(u_texture, v_tex_coord);
}
"#;

// ---------------------------------------------------------------------------
// GlPresenter — uploads the framebuffer to a GL texture and draws the quad
// ---------------------------------------------------------------------------

struct GlPresenter {
    gl: glow::Context,
    program: glow::Program,
    vao: glow::VertexArray,
    texture: glow::Texture,
    texture_width: u32,
    texture_height: u32,
}

impl GlPresenter {
    unsafe fn new(gl: glow::Context) -> Self {
        // Compile shaders
        let program = gl.create_program().expect("create_program");

        let vert = gl.create_shader(glow::VERTEX_SHADER).unwrap();
        gl.shader_source(vert, VERT_SHADER);
        gl.compile_shader(vert);
        assert!(
            gl.get_shader_compile_status(vert),
            "vert: {}",
            gl.get_shader_info_log(vert)
        );

        let frag = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
        gl.shader_source(frag, FRAG_SHADER);
        gl.compile_shader(frag);
        assert!(
            gl.get_shader_compile_status(frag),
            "frag: {}",
            gl.get_shader_info_log(frag)
        );

        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);
        assert!(
            gl.get_program_link_status(program),
            "link: {}",
            gl.get_program_info_log(program)
        );
        gl.delete_shader(vert);
        gl.delete_shader(frag);

        // Empty VAO (vertices come from gl_VertexID)
        let vao = gl.create_vertex_array().unwrap();

        // Framebuffer texture
        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
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

        Self {
            gl,
            program,
            vao,
            texture,
            texture_width: 0,
            texture_height: 0,
        }
    }

    /// Upload pixel data from a framebuffer. Call every frame.
    unsafe fn update_texture(&mut self, fb: &Framebuffer) {
        let w = fb.width();
        let h = fb.height();
        self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        if w != self.texture_width || h != self.texture_height {
            // Reallocate texture storage on size change.
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA as i32,
                w as i32,
                h as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(fb.pixels()),
            );
            self.texture_width = w;
            self.texture_height = h;
        } else {
            // Reuse existing storage — faster than re-allocating.
            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                w as i32,
                h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(fb.pixels()),
            );
        }
    }

    /// Draw the full-screen quad.
    unsafe fn present(&self) {
        self.gl.clear(glow::COLOR_BUFFER_BIT);
        self.gl.use_program(Some(self.program));
        self.gl.bind_vertex_array(Some(self.vao));
        self.gl.active_texture(glow::TEXTURE0);
        self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        self.gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let event_loop = EventLoop::new().expect("EventLoop::new");

    let window_attributes = WindowAttributes::default()
        .with_title("agg-gui — Phase 3 Demo")
        .with_inner_size(LogicalSize::new(1280u32, 720u32));

    let template = ConfigTemplateBuilder::new().with_alpha_size(0);
    let display_builder =
        DisplayBuilder::new().with_window_attributes(Some(window_attributes));

    let (window, gl_config) = display_builder
        .build(&event_loop, template, |configs| {
            configs
                .reduce(|a, b| if b.num_samples() > a.num_samples() { b } else { a })
                .expect("no suitable GL config")
        })
        .expect("DisplayBuilder::build");

    let window = window.expect("window");
    let raw_window_handle = window.window_handle().expect("window_handle").as_raw();

    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
        .build(Some(raw_window_handle));

    let gl_display = gl_config.display();
    let not_current_gl_context = unsafe {
        gl_display
            .create_context(&gl_config, &context_attributes)
            .expect("create_context")
    };

    let size = window.inner_size();
    let surface_attributes = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(size.width.max(1)).unwrap(),
        NonZeroU32::new(size.height.max(1)).unwrap(),
    );

    let gl_surface = unsafe {
        gl_display
            .create_window_surface(&gl_config, &surface_attributes)
            .expect("create_window_surface")
    };

    let gl_context = not_current_gl_context
        .make_current(&gl_surface)
        .expect("make_current");

    let gl = unsafe {
        glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
    };

    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("parse CascadiaCode.ttf"));

    let mut presenter = unsafe { GlPresenter::new(gl) };
    let mut fb = Framebuffer::new(size.width.max(1), size.height.max(1));

    // Draw and upload the first frame immediately.
    render_and_upload(&mut fb, &mut presenter, &font);

    #[allow(deprecated)]
    event_loop
        .run(|event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);
            match event {
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    elwt.exit();
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(new_size),
                    ..
                } => {
                    if new_size.width > 0 && new_size.height > 0 {
                        gl_surface.resize(
                            &gl_context,
                            NonZeroU32::new(new_size.width).unwrap(),
                            NonZeroU32::new(new_size.height).unwrap(),
                        );
                        unsafe {
                            presenter.gl.viewport(
                                0,
                                0,
                                new_size.width as i32,
                                new_size.height as i32,
                            );
                        }
                        fb.resize(new_size.width, new_size.height);
                        render_and_upload(&mut fb, &mut presenter, &font);
                    }
                }
                Event::AboutToWait => {
                    render_and_upload(&mut fb, &mut presenter, &font);
                    unsafe { presenter.present() };
                    gl_surface.swap_buffers(&gl_context).expect("swap_buffers");
                }
                _ => {}
            }
        })
        .expect("event_loop.run");
}

fn render_and_upload(fb: &mut Framebuffer, presenter: &mut GlPresenter, font: &Arc<Font>) {
    let w = fb.width();
    let h = fb.height();
    {
        let mut ctx = GfxCtx::new(fb);
        draw_phase3_demo(&mut ctx, w, h, font);
    }
    unsafe { presenter.update_texture(fb) };
}

// ---------------------------------------------------------------------------
// Phase 3 demo scene — text rendering showcase
// ---------------------------------------------------------------------------

fn draw_phase3_demo(ctx: &mut GfxCtx, width: u32, height: u32, font: &Arc<Font>) {
    use agg_gui::Color;

    let w = width as f64;
    let h = height as f64;

    ctx.set_font(Arc::clone(font));
    ctx.clear(Color::rgb(0.94, 0.94, 0.96));

    let pad = (w.min(h) * 0.03).max(10.0);
    let gap = pad * 0.6;
    let col_w = (w - pad * 2.0 - gap) / 2.0;
    let row_h = (h - pad * 2.0 - gap) / 2.0;

    let panels = [
        (pad,               pad + row_h + gap, col_w, row_h), // top-left
        (pad + col_w + gap, pad + row_h + gap, col_w, row_h), // top-right
        (pad,               pad,               col_w, row_h), // bottom-left
        (pad + col_w + gap, pad,               col_w, row_h), // bottom-right
    ];

    for &(px, py, pw, ph) in &panels {
        draw_card(ctx, px, py, pw, ph);
    }

    { let (px, py, pw, ph) = panels[0]; draw_sizes_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[1]; draw_measure_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[2]; draw_multiline_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[3]; draw_buttons_panel(ctx, px, py, pw, ph, font); }

    let lsize = (w * 0.012).clamp(9.0, 13.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
    ctx.fill_text_gsv("agg-gui  Phase 3 — Text", pad, pad * 0.4, lsize);
}

fn draw_card(ctx: &mut GfxCtx, x: f64, y: f64, w: f64, h: f64) {
    use agg_gui::Color;
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.08));
    ctx.set_blend_mode(CompOp::Multiply);
    ctx.begin_path();
    ctx.rounded_rect(x + 2.0, y - 2.0, w, h, 10.0);
    ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 10.0);
    ctx.fill();
}

fn panel_title_gsv(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, title: &str) {
    use agg_gui::Color;
    let size = (pw * 0.055).clamp(10.0, 16.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.55));
    ctx.fill_text_gsv(title, px + pw * 0.05, py + ph * 0.86, size);
}

fn draw_sizes_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, _font: &Arc<Font>) {
    use agg_gui::Color;
    panel_title_gsv(ctx, px, py, pw, ph, "Font Sizes");

    let margin = pw * 0.06;
    let sizes: &[(f64, &str)] = &[
        (10.0, "Caption — 10px  The quick brown fox"),
        (13.0, "Body — 13px  The quick brown fox"),
        (18.0, "Subhead — 18px  The quick"),
        (24.0, "Heading — 24px  agg-gui"),
        (34.0, "Display — 34px  Aa"),
    ];

    let mut y = py + ph * 0.82;
    let baseline_adv = ph * 0.155;

    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    for &(size, label) in sizes.iter() {
        ctx.set_font_size(size);
        ctx.fill_text(label, px + margin, y);
        y -= baseline_adv;
    }
}

fn draw_measure_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, _font: &Arc<Font>) {
    use agg_gui::Color;
    panel_title_gsv(ctx, px, py, pw, ph, "Measure Text");

    let margin = pw * 0.06;
    let font_size = (pw * 0.08).clamp(14.0, 26.0);
    ctx.set_font_size(font_size);

    let samples = ["Hello", "World!", "agg-gui", "Rust"];
    let col_w = (pw - margin * 2.0) / samples.len() as f64;
    let base_y = py + ph * 0.5;

    for (i, &word) in samples.iter().enumerate() {
        let x = px + margin + col_w * i as f64;

        let m = ctx.measure_text(word).unwrap_or_default();

        // Baseline
        ctx.set_stroke_color(Color::rgba(0.6, 0.6, 0.65, 0.5));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, base_y - 2.0);
        ctx.line_to(x + m.width, base_y - 2.0);
        ctx.stroke();

        // Ascent line
        ctx.set_stroke_color(Color::rgba(0.2, 0.5, 0.9, 0.35));
        ctx.begin_path();
        ctx.move_to(x, base_y + m.ascent);
        ctx.line_to(x + m.width, base_y + m.ascent);
        ctx.stroke();

        // Descent line
        ctx.set_stroke_color(Color::rgba(0.9, 0.3, 0.3, 0.35));
        ctx.begin_path();
        ctx.move_to(x, base_y - m.descent);
        ctx.line_to(x + m.width, base_y - m.descent);
        ctx.stroke();

        // Bounding box
        ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.07));
        ctx.begin_path();
        ctx.rect(x, base_y - m.descent, m.width, m.ascent + m.descent);
        ctx.fill();

        // The word itself
        ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.88));
        ctx.fill_text(word, x, base_y);
    }

    // Legend
    let lsize = (pw * 0.032).clamp(7.0, 10.0);
    let ly = py + ph * 0.22;
    let lx = px + margin;
    ctx.set_font_size(lsize);
    ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.7));
    ctx.fill_text("— ascent", lx, ly);
    ctx.set_fill_color(Color::rgba(0.9, 0.3, 0.3, 0.7));
    ctx.fill_text("— descent", lx, ly - lsize * 1.5);
    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.55, 0.7));
    ctx.fill_text("— baseline", lx, ly - lsize * 3.0);
}

fn draw_multiline_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    use agg_gui::Color;
    panel_title_gsv(ctx, px, py, pw, ph, "Multi-line");

    let margin = pw * 0.06;
    let font_size = (pw * 0.055).clamp(11.0, 16.0);
    ctx.set_font_size(font_size);
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));

    let line_h = font.line_height_px(font_size) * 1.25;
    let x = px + margin;

    let lines = [
        "agg-gui renders text by",
        "shaping with rustybuzz,",
        "extracting outlines via",
        "ttf-parser, and feeding",
        "Bezier curves into AGG.",
        "",
        "No glyph atlas. Kerning",
        "and hinting are preserved.",
    ];

    let mut y = py + ph * 0.82;
    for line in lines.iter() {
        if !line.is_empty() {
            ctx.fill_text(line, x, y);
        }
        y -= line_h;
    }
}

fn draw_buttons_panel(ctx: &mut GfxCtx, px: f64, py: f64, pw: f64, ph: f64, _font: &Arc<Font>) {
    use agg_gui::Color;
    panel_title_gsv(ctx, px, py, pw, ph, "Text + Graphics");

    let margin = pw * 0.07;
    let btn_h = ph * 0.16;
    let btn_r = btn_h * 0.35;
    let bx = px + margin;
    let bw = pw - margin * 2.0;

    let buttons: &[(&str, Color, Color)] = &[
        ("Primary Action",  Color::rgb(0.22, 0.45, 0.88), Color::white()),
        ("Secondary",       Color::rgba(0.22, 0.45, 0.88, 0.12), Color::rgb(0.22, 0.45, 0.88)),
        ("Destructive",     Color::rgb(0.88, 0.25, 0.18), Color::white()),
        ("Disabled",        Color::rgba(0.0, 0.0, 0.0, 0.08), Color::rgba(0.0,0.0,0.0,0.3)),
    ];

    let spacing = (ph * 0.74) / buttons.len() as f64;
    let font_size = (btn_h * 0.38).clamp(10.0, 16.0);
    ctx.set_font_size(font_size);

    for (i, &(label, bg, fg)) in buttons.iter().enumerate() {
        let by = py + ph * 0.78 - i as f64 * spacing;

        ctx.set_fill_color(bg);
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.begin_path();
        ctx.rounded_rect(bx, by - btn_h * 0.5, bw, btn_h, btn_r);
        ctx.fill();

        if let Some(m) = ctx.measure_text(label) {
            let tx = bx + (bw - m.width) * 0.5;
            let ty = by - m.ascent * 0.45 + m.descent * 0.45;
            ctx.set_fill_color(fg);
            ctx.fill_text(label, tx, ty);
        }
    }
}
