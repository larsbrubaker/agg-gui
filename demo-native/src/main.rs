//! Native GL demo for agg-gui.
//!
//! Renders via `GlGfxCtx` (tess2 → GL vertex buffers), matching the WASM path.
//! The UI is shared with the WASM target via `demo-ui`.
//! A rotating 3D cube widget is drawn on top each frame.

mod cube_widget;
use cube_widget::{CubeGlRenderer, GlCubeWidget, CUBE_SCREEN_RECT};

use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, Color, DrawCtx, Font, Key as AggKey, Modifiers,
              MouseButton as AggMouseButton, Rect, Size};

use demo_gl::GlGfxCtx;

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
// main
// ---------------------------------------------------------------------------

fn main() {
    let event_loop = EventLoop::new().expect("EventLoop::new");

    let window_attributes = WindowAttributes::default()
        .with_title("agg-gui — Demo (GL)")
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

    // Wrap in Rc so GlGfxCtx can share the context.
    let gl = Rc::new(unsafe {
        glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
    });

    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("parse CascadiaCode.ttf"));

    let mut cube_renderer = unsafe { CubeGlRenderer::new(&gl) };
    let init_w = size.width.max(1) as f32;
    let init_h = size.height.max(1) as f32;
    let mut gl_ctx = unsafe { GlGfxCtx::new(Rc::clone(&gl), init_w, init_h) };

    let (mut app, show_inspector, inspector_nodes) =
        demo_ui::build_demo_ui(Arc::clone(&font), Box::new(GlCubeWidget::new()));

    let mut cursor_x    = 0.0f64;
    let mut cursor_y    = 0.0f64;
    let mut last_frame_ms = 0.0f64;
    let mut win_w       = size.width.max(1);
    let mut win_h       = size.height.max(1);

    // Initial frame
    render_frame(&mut app, &mut gl_ctx, &mut cube_renderer, &gl,
                 win_w, win_h, last_frame_ms, Arc::clone(&font));
    gl_surface.swap_buffers(&gl_context).expect("swap_buffers");

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
                        unsafe { gl.viewport(0, 0, new_size.width as i32, new_size.height as i32); }
                        win_w = new_size.width;
                        win_h = new_size.height;
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
                    // Winit: LineDelta y > 0 = wheel up = scroll UP = negative delta.
                    let delta_y = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => -(y as f64),
                        winit::event::MouseScrollDelta::PixelDelta(d) => d.y / 40.0,
                    };
                    app.on_mouse_wheel(cursor_x, cursor_y, delta_y);
                }
                Event::AboutToWait => {
                    let t0 = std::time::Instant::now();

                    // Sync inspector node snapshot before painting.
                    if show_inspector.get() {
                        *inspector_nodes.borrow_mut() = app.collect_inspector_nodes();
                    }

                    render_frame(&mut app, &mut gl_ctx, &mut cube_renderer, &gl,
                                 win_w, win_h, last_frame_ms, Arc::clone(&font));
                    gl_surface.swap_buffers(&gl_context).expect("swap_buffers");

                    last_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
                }
                _ => {}
            }
        })
        .expect("event_loop.run");
}

// ---------------------------------------------------------------------------
// render_frame — GL path (no AGG framebuffer)
// ---------------------------------------------------------------------------

fn render_frame(
    app:      &mut App,
    gl_ctx:   &mut GlGfxCtx,
    cube:     &mut CubeGlRenderer,
    gl:       &glow::Context,
    w:        u32,
    h:        u32,
    frame_ms: f64,
    font:     Arc<Font>,
) {
    unsafe {
        gl.viewport(0, 0, w as i32, h as i32);
        gl.clear_color(0.1, 0.1, 0.1, 1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.enable(glow::BLEND);
        gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        gl.disable(glow::DEPTH_TEST);
        gl.disable(glow::SCISSOR_TEST);
    }

    // Reset cube rect so a hidden GlCubeWidget leaves it zeroed — draw_gl
    // skips automatically when width < 1.
    CUBE_SCREEN_RECT.with(|r| r.set(Rect::default()));

    gl_ctx.reset(w as f32, h as f32);
    app.layout(Size::new(w as f64, h as f64));
    app.paint(gl_ctx);

    // Status bar overlay: "WxH  X.Xms"
    let status = format!("{}×{}   {:.1}ms", w, h, frame_ms);
    gl_ctx.set_font(font);
    gl_ctx.set_font_size(11.0);
    gl_ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.30));
    gl_ctx.fill_text(&status, 12.0, 6.0);

    // Draw the rotating cube on top, inside its widget rect.
    let cube_rect = CUBE_SCREEN_RECT.with(|r| r.get());
    unsafe { cube.draw_gl(gl, cube_rect, h as f64, w as i32, h as i32) };
}

// ---------------------------------------------------------------------------
// Input mapping helpers
// ---------------------------------------------------------------------------

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

fn map_mouse_button(b: &winit::event::MouseButton) -> AggMouseButton {
    match b {
        winit::event::MouseButton::Left   => AggMouseButton::Left,
        winit::event::MouseButton::Right  => AggMouseButton::Right,
        winit::event::MouseButton::Middle => AggMouseButton::Middle,
        winit::event::MouseButton::Other(n) => AggMouseButton::Other(*n as u8),
        _ => AggMouseButton::Other(255),
    }
}
