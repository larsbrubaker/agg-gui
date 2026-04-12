//! Native WGL demo for agg-gui — Phase 6.
//!
//! Renders a TabView demo (Flex, Scroll, Split, Tree) via AGG → Framebuffer
//! → GL texture → full-screen quad.  Winit events are forwarded to [`App`].

use std::num::NonZeroU32;
use std::sync::Arc;

use agg_gui::{
    App, Button, Color, FlexColumn, FlexRow, Font, Framebuffer, GfxCtx,
    Key as AggKey, Modifiers, MouseButton as AggMouseButton, NodeIcon,
    ScrollView, Size, SizedBox, Spacer, Splitter, TabView, TextField, TreeView,
};

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, Version};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{GlSurface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use glow::HasContext;
use raw_window_handle::HasWindowHandle;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::WindowAttributes;

const FONT_BYTES: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

// ---------------------------------------------------------------------------
// GL shaders — full-screen quad using gl_VertexID (no VBO required)
// ---------------------------------------------------------------------------

const VERT_SHADER: &str = r#"#version 330 core
out vec2 v_tex_coord;
void main() {
    float x = float((gl_VertexID & 1) * 2) - 1.0;
    float y = float((gl_VertexID >> 1) * 2) - 1.0;
    gl_Position = vec4(x, y, 0.0, 1.0);
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
// GlPresenter — uploads framebuffer → GL texture → full-screen quad
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
        let program = gl.create_program().expect("create_program");
        let vert = gl.create_shader(glow::VERTEX_SHADER).unwrap();
        gl.shader_source(vert, VERT_SHADER);
        gl.compile_shader(vert);
        assert!(gl.get_shader_compile_status(vert), "vert: {}", gl.get_shader_info_log(vert));
        let frag = gl.create_shader(glow::FRAGMENT_SHADER).unwrap();
        gl.shader_source(frag, FRAG_SHADER);
        gl.compile_shader(frag);
        assert!(gl.get_shader_compile_status(frag), "frag: {}", gl.get_shader_info_log(frag));
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);
        assert!(gl.get_program_link_status(program), "link: {}", gl.get_program_info_log(program));
        gl.delete_shader(vert);
        gl.delete_shader(frag);

        let vao = gl.create_vertex_array().unwrap();

        let texture = gl.create_texture().unwrap();
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);

        Self { gl, program, vao, texture, texture_width: 0, texture_height: 0 }
    }

    unsafe fn update_texture(&mut self, fb: &Framebuffer) {
        let w = fb.width();
        let h = fb.height();
        self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
        if w != self.texture_width || h != self.texture_height {
            self.gl.tex_image_2d(
                glow::TEXTURE_2D, 0, glow::RGBA as i32, w as i32, h as i32,
                0, glow::RGBA, glow::UNSIGNED_BYTE, Some(fb.pixels()),
            );
            self.texture_width = w;
            self.texture_height = h;
        } else {
            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D, 0, 0, 0, w as i32, h as i32,
                glow::RGBA, glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(fb.pixels()),
            );
        }
    }

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
// Widget tree — Phase 5 layout demo (mirrors the WASM Layout tab)
// ---------------------------------------------------------------------------

fn build_layout_ui(font: Arc<Font>) -> App {
    let tab_view = TabView::new(Arc::clone(&font))
        .with_tab_bar_height(36.0)
        .with_font_size(13.0)
        .add_tab("Flex",   Box::new(build_flex_demo(Arc::clone(&font))))
        .add_tab("Scroll", Box::new(build_scroll_demo(Arc::clone(&font))))
        .add_tab("Split",  Box::new(build_split_demo(Arc::clone(&font))))
        .add_tab("Tree",   Box::new(build_tree_demo(Arc::clone(&font))));
    App::new(Box::new(tab_view))
}

fn build_tree_demo(font: Arc<Font>) -> TreeView {
    let mut tv = TreeView::new(Arc::clone(&font))
        .with_row_height(26.0)
        .with_font_size(13.0)
        .with_indent_width(18.0);

    let alpha = tv.add_root("Project Alpha", NodeIcon::Package);
    tv.expand(alpha);
    let src = tv.add_child(alpha, "src", NodeIcon::Folder);
    tv.expand(src);
    tv.add_child(src, "main.rs", NodeIcon::File);
    tv.add_child(src, "lib.rs", NodeIcon::File);
    let widgets_dir = tv.add_child(src, "widgets", NodeIcon::Folder);
    tv.expand(widgets_dir);
    tv.add_child(widgets_dir, "button.rs", NodeIcon::File);
    tv.add_child(widgets_dir, "scroll_view.rs", NodeIcon::File);
    tv.add_child(widgets_dir, "tree_view.rs", NodeIcon::File);
    let tests = tv.add_child(alpha, "tests", NodeIcon::Folder);
    tv.expand(tests);
    tv.add_child(tests, "integration.rs", NodeIcon::File);
    tv.add_child(tests, "unit.rs", NodeIcon::File);
    tv.add_child(alpha, "Cargo.toml", NodeIcon::File);

    let beta = tv.add_root("Project Beta", NodeIcon::Package);
    let bsrc = tv.add_child(beta, "src", NodeIcon::Folder);
    tv.add_child(bsrc, "app.rs", NodeIcon::File);
    tv.add_child(bsrc, "config.rs", NodeIcon::File);
    tv.add_child(beta, "Cargo.toml", NodeIcon::File);

    let gamma = tv.add_root("Project Gamma", NodeIcon::Package);
    let gsrc = tv.add_child(gamma, "src", NodeIcon::Folder);
    tv.add_child(gsrc, "main.rs", NodeIcon::File);
    tv.add_child(gsrc, "render.rs", NodeIcon::File);
    tv.add_child(gamma, "Cargo.toml", NodeIcon::File);

    tv
}

fn map_key(key: &WinitKey) -> Option<AggKey> {
    Some(match key {
        WinitKey::Named(NamedKey::ArrowUp)    => AggKey::ArrowUp,
        WinitKey::Named(NamedKey::ArrowDown)  => AggKey::ArrowDown,
        WinitKey::Named(NamedKey::ArrowLeft)  => AggKey::ArrowLeft,
        WinitKey::Named(NamedKey::ArrowRight) => AggKey::ArrowRight,
        WinitKey::Named(NamedKey::Enter)      => AggKey::Enter,
        WinitKey::Named(NamedKey::Space)      => AggKey::Char(' '),
        WinitKey::Named(NamedKey::Tab)        => AggKey::Tab,
        WinitKey::Named(NamedKey::Escape)     => AggKey::Escape,
        WinitKey::Named(NamedKey::Backspace)  => AggKey::Backspace,
        WinitKey::Character(s) => AggKey::Char(s.chars().next()?),
        _ => return None,
    })
}

fn build_flex_demo(font: Arc<Font>) -> FlexColumn {
    let fa = Arc::clone(&font);
    let fb = Arc::clone(&font);
    let fc = Arc::clone(&font);
    let fd = Arc::clone(&font);
    let fe = Arc::clone(&font);

    let row = FlexRow::new()
        .with_gap(8.0)
        .add_flex(Box::new(Button::new("One",   fa).with_font_size(13.0).on_click(|| println!("One"))), 1.0)
        .add_flex(Box::new(Button::new("Two",   fb).with_font_size(13.0).on_click(|| println!("Two"))), 1.0)
        .add_flex(Box::new(Button::new("Three", fc).with_font_size(13.0).on_click(|| println!("Three"))), 1.0);

    FlexColumn::new()
        .with_gap(12.0)
        .with_padding(20.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(row))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(fd).with_font_size(14.0).with_placeholder("Search…"),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0)
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Confirm", fe).with_font_size(14.0).on_click(|| println!("Confirm")),
        ))))
}

fn build_scroll_demo(font: Arc<Font>) -> ScrollView {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96));

    for i in 0..24u32 {
        let label = format!("Item {:02}", i + 1);
        col.push(
            Box::new(SizedBox::new().with_height(40.0).with_child(Box::new(
                Button::new(label, Arc::clone(&font))
                    .with_font_size(13.0)
                    .on_click(|| {}),
            ))),
            0.0,
        );
    }

    ScrollView::new(Box::new(col))
}

fn build_split_demo(font: Arc<Font>) -> Splitter {
    let fa = Arc::clone(&font);
    let fb = Arc::clone(&font);
    let fc = Arc::clone(&font);
    let fd = Arc::clone(&font);

    let left = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.96, 0.96, 0.99))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left A", fa).with_font_size(13.0).on_click(|| println!("Left A")),
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left B", fb).with_font_size(13.0).on_click(|| println!("Left B")),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    let right = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.99, 0.96, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(fc).with_font_size(13.0).with_placeholder("Right field…"),
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Action", fd).with_font_size(13.0).on_click(|| println!("Action")),
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    Splitter::new(Box::new(left), Box::new(right)).with_ratio(0.4)
}

fn map_mouse_button(b: &winit::event::MouseButton) -> AggMouseButton {
    match b {
        winit::event::MouseButton::Left   => AggMouseButton::Left,
        winit::event::MouseButton::Right  => AggMouseButton::Right,
        winit::event::MouseButton::Middle => AggMouseButton::Middle,
        winit::event::MouseButton::Other(n) => AggMouseButton::Other(*n as u8),
        _ => AggMouseButton::Other(255),
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let event_loop = EventLoop::new().expect("EventLoop::new");

    let window_attributes = WindowAttributes::default()
        .with_title("agg-gui — Phase 6 Demo")
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
    let mut app = build_layout_ui(Arc::clone(&font));

    // Last known cursor position (Y-down, physical pixels).
    let mut cursor_x = 0.0f64;
    let mut cursor_y = 0.0f64;

    render_frame(&mut app, &mut fb, &mut presenter);

    #[allow(deprecated)]
    event_loop
        .run(|event, elwt| {
            elwt.set_control_flow(ControlFlow::Poll);
            match event {
                Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                    elwt.exit();
                }
                Event::WindowEvent {
                    event: WindowEvent::Resized(new_size), ..
                } => {
                    if new_size.width > 0 && new_size.height > 0 {
                        gl_surface.resize(
                            &gl_context,
                            NonZeroU32::new(new_size.width).unwrap(),
                            NonZeroU32::new(new_size.height).unwrap(),
                        );
                        unsafe {
                            presenter.gl.viewport(0, 0, new_size.width as i32, new_size.height as i32);
                        }
                        fb.resize(new_size.width, new_size.height);
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorMoved { position, .. }, ..
                } => {
                    cursor_x = position.x;
                    cursor_y = position.y;
                    app.on_mouse_move(cursor_x, cursor_y);
                }
                Event::WindowEvent {
                    event: WindowEvent::CursorLeft { .. }, ..
                } => {
                    app.on_mouse_leave();
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseInput { state, button, .. }, ..
                } => {
                    let btn = map_mouse_button(&button);
                    let mods = Modifiers::default();
                    match state {
                        ElementState::Pressed  => app.on_mouse_down(cursor_x, cursor_y, btn, mods),
                        ElementState::Released => app.on_mouse_up(cursor_x, cursor_y, btn, mods),
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::KeyboardInput { event: key_event, .. }, ..
                } => {
                    if key_event.state == ElementState::Pressed {
                        if let Some(key) = map_key(&key_event.logical_key) {
                            app.on_key_down(key, Modifiers::default());
                        }
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseWheel { delta, .. }, ..
                } => {
                    // Positive delta_y = scroll up = increase scroll_offset in ScrollView.
                    let delta_y = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y as f64,
                        winit::event::MouseScrollDelta::PixelDelta(d) => -d.y / 40.0,
                    };
                    app.on_mouse_wheel(cursor_x, cursor_y, delta_y);
                }
                Event::AboutToWait => {
                    render_frame(&mut app, &mut fb, &mut presenter);
                    unsafe { presenter.present() };
                    gl_surface.swap_buffers(&gl_context).expect("swap_buffers");
                }
                _ => {}
            }
        })
        .expect("event_loop.run");
}

fn render_frame(app: &mut App, fb: &mut Framebuffer, presenter: &mut GlPresenter) {
    let w = fb.width();
    let h = fb.height();
    app.layout(Size::new(w as f64, h as f64));
    {
        let mut ctx = GfxCtx::new(fb);
        app.paint(&mut ctx);

        let lsize = (w as f64 * 0.012).clamp(9.0, 13.0);
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.3));
        ctx.fill_text_gsv("agg-gui  Phase 6 — Layout & Tree", 12.0, 6.0, lsize);
    }
    unsafe { presenter.update_texture(fb) };
}
