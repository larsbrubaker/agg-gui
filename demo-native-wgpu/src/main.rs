//! Native wgpu demo for agg-gui.
//!
//! Sibling to `demo-native` (which uses `demo-gl` + glutin/glow): this binary
//! is the smallest scaffold that runs the shared demo UI through `WgpuGfxCtx`.
//! Use it to verify the wgpu rendering path before retiring the GL stack in
//! Phase 12 of the migration.
//!
//! # Scope
//!
//! Currently covers: window creation, wgpu device/surface init, the per-frame
//! flush via `WgpuGfxCtx::end_frame`, resize, and mouse/keyboard/wheel input.
//! NOT yet ported from `demo-native`: state persistence, fullscreen toggle,
//! screenshot, MSAA selection, relaunch button, touch events, hi-DPI scale
//! tracking.  Those will fold back into a single platform shell once the wgpu
//! path is verified end to end.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{winit_adapter, App, Modifiers, Size};
use demo_wgpu::{begin_frame, render_app_frame, WgpuCubeWidget, WgpuGfxCtx};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes};

fn demo_asset_path(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("demo")
        .join(relative)
}

fn install_demo_font_asset(name: &str, path: &str) {
    let primary = match std::fs::read(demo_asset_path(path)) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("failed to read font asset {path}: {err}");
            return;
        }
    };
    let icons = std::fs::read(demo_asset_path(demo_ui::FONT_AWESOME_PATH)).ok();
    let emoji = std::fs::read(demo_asset_path(demo_ui::EMOJI_FONT_PATH)).ok();
    if let Err(err) = demo_ui::install_font_bytes(name, primary, icons, emoji) {
        eprintln!("failed to parse font asset {path}: {err}");
    }
}

struct Gpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_desc);
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("request adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("demo-native-wgpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        // Prefer a non-sRGB format so the existing colour math (which assumes
        // linear-space writes) doesn't get gamma-corrected by the surface.
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface,
            surface_format,
            config,
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }
}

#[allow(deprecated)]
fn main() {
    let event_loop = EventLoop::new().expect("event loop");

    let default_font_asset = demo_ui::font_asset_by_name(demo_ui::DEFAULT_FONT_NAME)
        .expect("default demo font asset is registered");
    install_demo_font_asset(default_font_asset.name, default_font_asset.path);
    let font = demo_ui::load_font_by_name(demo_ui::DEFAULT_FONT_NAME)
        .expect("default demo font asset should load at startup");

    let window_attributes = WindowAttributes::default()
        .with_title("agg-gui — Demo (wgpu)")
        .with_inner_size(LogicalSize::new(1280, 720));

    let window = Arc::new(
        event_loop
            .create_window(window_attributes)
            .expect("create window"),
    );
    agg_gui::set_device_scale(window.scale_factor());

    let mut gpu = Gpu::new(Arc::clone(&window));
    let init_w = gpu.config.width as f32;
    let init_h = gpu.config.height as f32;
    let mut wgpu_ctx = WgpuGfxCtx::new(
        Arc::clone(&gpu.device),
        Arc::clone(&gpu.queue),
        gpu.surface_format,
        init_w,
        init_h,
    );

    // Stub platform hooks — Relaunch/font-requester features are out of scope
    // for this verification binary.
    let platform = demo_ui::PlatformHooks::native(0u8, || {})
        .with_font_requester(install_demo_font_asset);

    let (mut app, handles) = demo_ui::build_demo_ui(
        Arc::clone(&font),
        Box::new(WgpuCubeWidget::new()),
        "wgpu",
        "native wgpu (winit)",
        None,
        platform,
    );
    let show_inspector = Rc::clone(&handles.show_inspector);
    let inspector_nodes = Rc::clone(&handles.inspector_nodes);
    let hovered_bounds = Rc::clone(&handles.hovered_bounds);
    let base_edits = Rc::clone(&handles.base_edits);
    #[cfg(feature = "reflect")]
    let inspector_edits = Rc::clone(&handles.inspector_edits);
    let run_mode = Rc::clone(&handles.run_mode);
    let screen_size = Rc::clone(&handles.screen_size);
    let frame_history = Rc::clone(&handles.frame_history);

    let mut win_w = gpu.config.width;
    let mut win_h = gpu.config.height;
    screen_size.set((win_w, win_h));

    let mut cursor_x = 0.0f64;
    let mut cursor_y = 0.0f64;
    let mut current_mods = Modifiers::default();
    let mut last_frame_ms = 0.0f64;

    // Initial layout pass before we ever paint.
    app.layout(Size::new(win_w as f64, win_h as f64));

    event_loop
        .run(move |event, elwt| match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),
            Event::WindowEvent {
                event: WindowEvent::Resized(new_size),
                ..
            } => {
                if new_size.width > 0 && new_size.height > 0 {
                    gpu.resize(new_size.width, new_size.height);
                    win_w = new_size.width;
                    win_h = new_size.height;
                    screen_size.set((win_w, win_h));
                    window.request_redraw();
                }
            }
            Event::WindowEvent {
                event: WindowEvent::ScaleFactorChanged { scale_factor, .. },
                ..
            } => {
                agg_gui::set_device_scale(scale_factor);
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                cursor_x = position.x;
                cursor_y = position.y;
                app.on_mouse_move(cursor_x, cursor_y);
                winit_adapter::apply_cursor(&window, agg_gui::current_cursor_icon());
            }
            Event::WindowEvent {
                event: WindowEvent::CursorLeft { .. },
                ..
            } => {
                app.on_mouse_leave();
            }
            Event::WindowEvent {
                event: WindowEvent::ModifiersChanged(mods_state),
                ..
            } => {
                current_mods = winit_adapter::modifiers(mods_state.state());
            }
            Event::WindowEvent {
                event: WindowEvent::MouseInput { state, button, .. },
                ..
            } => {
                let btn = winit_adapter::mouse_button(button);
                match state {
                    ElementState::Pressed => {
                        app.on_mouse_down(cursor_x, cursor_y, btn, current_mods);
                    }
                    ElementState::Released => {
                        app.on_mouse_up(cursor_x, cursor_y, btn, current_mods);
                    }
                }
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event: key_event, ..
                    },
                ..
            } => {
                if key_event.state == ElementState::Pressed {
                    if let Some(key) = winit_adapter::key_event(&key_event, current_mods) {
                        app.on_key_down(key, current_mods);
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                let (mut dx, mut dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-(x as f64), -(y as f64)),
                    MouseScrollDelta::PixelDelta(d) => (d.x / 40.0, d.y / 40.0),
                };
                if current_mods.shift && dx == 0.0 {
                    dx = dy;
                    dy = 0.0;
                }
                app.on_mouse_wheel_xy_mods(cursor_x, cursor_y, dx, dy, current_mods);
            }
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                paint_frame(
                    &gpu,
                    &mut wgpu_ctx,
                    &mut app,
                    win_w,
                    win_h,
                    last_frame_ms,
                    show_inspector.get(),
                    &inspector_nodes,
                    &hovered_bounds,
                    &base_edits,
                    #[cfg(feature = "reflect")]
                    &inspector_edits,
                );
            }
            Event::AboutToWait => {
                let continuous = run_mode.get() == demo_ui::RunMode::Continuous;
                let want_render = continuous || app.wants_draw();
                if want_render {
                    let t0 = web_time::Instant::now();
                    paint_frame(
                        &gpu,
                        &mut wgpu_ctx,
                        &mut app,
                        win_w,
                        win_h,
                        last_frame_ms,
                        show_inspector.get(),
                        &inspector_nodes,
                        &hovered_bounds,
                        &base_edits,
                        #[cfg(feature = "reflect")]
                        &inspector_edits,
                    );
                    last_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;
                    frame_history.borrow_mut().push(last_frame_ms as f32);
                }
                let want_next = continuous || app.wants_draw();
                elwt.set_control_flow(if want_next {
                    ControlFlow::Poll
                } else if let Some(t) = app.next_draw_deadline() {
                    ControlFlow::WaitUntil(t)
                } else {
                    ControlFlow::Wait
                });
            }
            _ => {}
        })
        .expect("event loop");
}

#[allow(clippy::too_many_arguments)]
fn paint_frame(
    gpu: &Gpu,
    ctx: &mut WgpuGfxCtx,
    app: &mut App,
    w: u32,
    h: u32,
    frame_ms: f64,
    show_inspector: bool,
    inspector_nodes: &Rc<RefCell<Vec<agg_gui::InspectorNode>>>,
    hovered_bounds: &Rc<RefCell<Option<agg_gui::InspectorOverlay>>>,
    base_edits: &Rc<RefCell<Vec<agg_gui::WidgetBaseEdit>>>,
    #[cfg(feature = "reflect")] inspector_edits: &Rc<RefCell<Vec<agg_gui::InspectorEdit>>>,
) {
    let frame = match gpu.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        // All non-success cases (Lost / Outdated / Timeout / Occluded /
        // Validation) — skip this frame; the surface will be reconfigured or
        // the next tick will retry.
        _ => return,
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    begin_frame(ctx);
    render_app_frame(
        ctx,
        app,
        w,
        h,
        frame_ms,
        show_inspector,
        inspector_nodes,
        hovered_bounds,
        base_edits,
        #[cfg(feature = "reflect")]
        inspector_edits,
    );
    ctx.end_frame(&view);
    frame.present();
}
