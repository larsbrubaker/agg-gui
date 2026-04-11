//! Native WGL demo for agg-gui — Phase 1.
//!
//! Renders the Phase 1 demo scene via AGG → Framebuffer → GL texture →
//! full-screen quad. The framebuffer uses bottom-up (Y-up) row order which
//! matches OpenGL's texture layout, so no Y-flip is needed at upload time.

use std::num::NonZeroU32;

use agg_gui::{Framebuffer, GfxCtx};

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
        .with_title("agg-gui — Phase 1 Demo")
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

    let mut presenter = unsafe { GlPresenter::new(gl) };
    let mut fb = Framebuffer::new(size.width.max(1), size.height.max(1));

    // Draw and upload the first frame immediately.
    render_and_upload(&mut fb, &mut presenter);

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
                        render_and_upload(&mut fb, &mut presenter);
                    }
                }
                Event::AboutToWait => {
                    render_and_upload(&mut fb, &mut presenter);
                    unsafe { presenter.present() };
                    gl_surface.swap_buffers(&gl_context).expect("swap_buffers");
                }
                _ => {}
            }
        })
        .expect("event_loop.run");
}

fn render_and_upload(fb: &mut Framebuffer, presenter: &mut GlPresenter) {
    let w = fb.width();
    let h = fb.height();
    {
        let mut ctx = GfxCtx::new(fb);
        draw_phase1_demo(&mut ctx, w, h);
    }
    unsafe { presenter.update_texture(fb) };
}

// ---------------------------------------------------------------------------
// Phase 1 demo scene — proves Y-up coordinates and CCW rotations
// ---------------------------------------------------------------------------
// (Shared logic with demo-wasm/src/lib.rs — kept in sync manually until
//  we add a demo-shared crate in a future refactor)

fn draw_phase1_demo(ctx: &mut GfxCtx, width: u32, height: u32) {
    use agg_gui::Color;
    use std::f64::consts::FRAC_PI_2;

    let w = width as f64;
    let h = height as f64;
    let cx = w / 2.0;
    let cy = h / 2.0;

    // --- Background ---
    ctx.clear(Color::rgb(0.12, 0.12, 0.14));

    // --- Coordinate grid (subtle) ---
    ctx.set_stroke_color(Color::rgba(1.0, 1.0, 1.0, 0.08));
    ctx.set_line_width(1.0);
    for i in 1..8 {
        let x = w * i as f64 / 8.0;
        ctx.begin_path();
        ctx.move_to(x, 0.0);
        ctx.line_to(x, h);
        ctx.stroke();
        let y = h * i as f64 / 8.0;
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(w, y);
        ctx.stroke();
    }

    // --- Y-axis indicator ---
    let ax = 80.0;
    let ay_base = h * 0.2;
    let ay_tip = h * 0.8;
    ctx.set_stroke_color(Color::rgb(0.3, 0.9, 0.3));
    ctx.set_line_width(2.5);
    ctx.begin_path();
    ctx.move_to(ax, ay_base);
    ctx.line_to(ax, ay_tip);
    ctx.stroke();
    ctx.set_fill_color(Color::rgb(0.3, 0.9, 0.3));
    ctx.begin_path();
    ctx.move_to(ax, ay_tip + 14.0);
    ctx.line_to(ax - 9.0, ay_tip - 2.0);
    ctx.line_to(ax + 9.0, ay_tip - 2.0);
    ctx.close_path();
    ctx.fill();
    ctx.fill_text_gsv("+Y", ax - 14.0, ay_tip + 20.0, 14.0);

    // --- X-axis indicator ---
    let bx_base = w * 0.15;
    let bx_tip = w * 0.45;
    let by = 80.0;
    ctx.set_stroke_color(Color::rgb(0.9, 0.3, 0.3));
    ctx.set_line_width(2.5);
    ctx.begin_path();
    ctx.move_to(bx_base, by);
    ctx.line_to(bx_tip, by);
    ctx.stroke();
    ctx.set_fill_color(Color::rgb(0.9, 0.3, 0.3));
    ctx.begin_path();
    ctx.move_to(bx_tip + 14.0, by);
    ctx.line_to(bx_tip - 2.0, by - 9.0);
    ctx.line_to(bx_tip - 2.0, by + 9.0);
    ctx.close_path();
    ctx.fill();
    ctx.fill_text_gsv("+X", bx_tip + 18.0, by - 7.0, 14.0);

    // --- Origin dot ---
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 0.3));
    ctx.begin_path();
    ctx.circle(18.0, 18.0, 8.0);
    ctx.fill();
    ctx.set_fill_color(Color::rgb(0.9, 0.9, 0.2));
    ctx.fill_text_gsv("(0,0)", 4.0, 30.0, 11.0);

    // --- CCW rotation proof: translate to center then rotate +90° ---
    ctx.save();
    ctx.translate(cx, cy);
    ctx.rotate(FRAC_PI_2);
    let arrow_len = w.min(h) * 0.18;
    let arrow_half_w = arrow_len * 0.08;
    ctx.set_fill_color(Color::rgb(0.4, 0.6, 1.0));
    ctx.begin_path();
    ctx.move_to(-arrow_len * 0.5, -arrow_half_w);
    ctx.line_to(arrow_len * 0.3, -arrow_half_w);
    ctx.line_to(arrow_len * 0.3, -arrow_half_w * 2.5);
    ctx.line_to(arrow_len * 0.5, 0.0);
    ctx.line_to(arrow_len * 0.3, arrow_half_w * 2.5);
    ctx.line_to(arrow_len * 0.3, arrow_half_w);
    ctx.line_to(-arrow_len * 0.5, arrow_half_w);
    ctx.close_path();
    ctx.fill();
    ctx.restore();

    ctx.set_fill_color(Color::rgba(0.4, 0.6, 1.0, 0.9));
    ctx.fill_text_gsv("rotate(+90deg) -> points UP", cx - 90.0, cy + arrow_len * 0.55 + 18.0, 12.0);

    // --- Reference circle at center ---
    let r = w.min(h) * 0.12;
    ctx.set_stroke_color(Color::rgba(1.0, 1.0, 1.0, 0.25));
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.circle(cx, cy, r);
    ctx.stroke();

    // --- Corner dots ---
    let pad = 30.0;
    let dot_r = 6.0;
    ctx.set_fill_color(Color::rgb(0.9, 0.9, 0.3));
    ctx.begin_path();
    ctx.circle(pad, pad, dot_r);
    ctx.fill();
    ctx.set_fill_color(Color::rgb(0.3, 0.9, 0.9));
    ctx.begin_path();
    ctx.circle(pad, h - pad, dot_r);
    ctx.fill();
    ctx.set_fill_color(Color::rgb(0.9, 0.3, 0.9));
    ctx.begin_path();
    ctx.circle(w - pad, pad, dot_r);
    ctx.fill();
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path();
    ctx.circle(w - pad, h - pad, dot_r);
    ctx.fill();

    // --- Title ---
    ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 0.9));
    ctx.fill_text_gsv("agg-gui  Phase 1", cx - 60.0, h - 36.0, 18.0);
    ctx.set_fill_color(Color::rgba(0.6, 0.6, 0.6, 0.7));
    ctx.fill_text_gsv(
        "Y-up coordinates  |  CCW rotations  |  AGG rasterization",
        cx - 145.0,
        h - 56.0,
        11.0,
    );
}
