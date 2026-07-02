//! Turn-key native (winit + wgpu) shell for an agg-gui [`App`].
//!
//! Every native platform crate used to hand-roll the same ~250 lines:
//! wgpu instance/surface/device setup, the winit event loop with input
//! forwarding, DPI tracking, redraw scheduling, and the per-frame
//! layout/paint. That machinery is platform glue, not app code — it now
//! lives here so a native shim reduces to its genuinely app-specific
//! parts (platform-trait impl, window title, per-frame state tick):
//!
//! ```ignore
//! fn main() {
//!     let (app, handles) = build_my_app(font, MyNativePlatform::new());
//!     demo_wgpu::native_shell::run(
//!         demo_wgpu::NativeShellConfig { title: "My App", logical_size: (1024.0, 768.0) },
//!         app,
//!         move || handles.timestamp_ms.set(now_ms()), // per-frame hook, or `|| {}`
//!     );
//! }
//! ```
//!
//! The web equivalent is [`crate::web_shell`].

#![allow(deprecated)] // winit 0.30 EventLoop::run idiom

use std::sync::Arc;

use agg_gui::{winit_adapter, App, Modifiers, Size};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes};

use crate::{begin_frame, WgpuGfxCtx};

/// Window parameters for [`run`].
pub struct NativeShellConfig {
    /// OS window title.
    pub title: &'static str,
    /// Initial inner size in logical (DPI-independent) pixels.
    pub logical_size: (f64, f64),
}

/// wgpu device + surface bundle for one OS window.
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
            .expect("create wgpu surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("request wgpu adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("agg-gui-native-shell"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("request wgpu device");

        let caps = surface.get_capabilities(&adapter);
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

/// Open an OS window, wire all input into `app`, and run the event loop
/// until the window closes.
///
/// `on_frame` runs once per painted frame, before layout — the hook for
/// per-frame app state such as advancing a wall-clock cell. Pass `|| {}`
/// when the app has no per-frame state.
pub fn run(config: NativeShellConfig, mut app: App, mut on_frame: impl FnMut() + 'static) {
    let event_loop = EventLoop::new().expect("create event loop");

    let window_attributes = WindowAttributes::default()
        .with_title(config.title)
        .with_inner_size(LogicalSize::new(config.logical_size.0, config.logical_size.1))
        // Shown after the first surface configure to avoid a white flash.
        .with_visible(false);
    let window = Arc::new(
        event_loop
            .create_window(window_attributes)
            .expect("create window"),
    );
    agg_gui::set_device_scale(window.scale_factor());

    let mut gpu = Gpu::new(window.clone());
    let mut wgpu_ctx = WgpuGfxCtx::new(
        Arc::clone(&gpu.device),
        Arc::clone(&gpu.queue),
        gpu.surface_format,
        gpu.config.width as f32,
        gpu.config.height as f32,
    );

    let mut win_w = window.inner_size().width.max(1);
    let mut win_h = window.inner_size().height.max(1);
    let mut cursor_x = 0.0_f64;
    let mut cursor_y = 0.0_f64;
    let mut current_mods = Modifiers::default();
    let mut layout_key: Option<(u32, u32, u64, u64)> = None;

    window.set_visible(true);

    event_loop
        .run(move |event, elwt| match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                elwt.exit();
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } if size.width > 0 && size.height > 0 => {
                win_w = size.width;
                win_h = size.height;
                gpu.resize(win_w, win_h);
                window.request_redraw();
            }

            Event::WindowEvent {
                event: WindowEvent::ScaleFactorChanged { scale_factor, .. },
                ..
            } => {
                agg_gui::set_device_scale(scale_factor);
                window.request_redraw();
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
                event: WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => (dx as f64, dy as f64),
                    // Trackpads report pixel deltas; ~40 px per wheel line
                    // matches the browser-shell conversion in `web_shell`.
                    MouseScrollDelta::PixelDelta(p) => (p.x / 40.0, p.y / 40.0),
                };
                app.on_mouse_wheel_xy_mods(cursor_x, cursor_y, dx, dy, current_mods);
            }

            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        event: key_event, ..
                    },
                ..
            } => {
                let Some(key) = winit_adapter::key_event(&key_event, current_mods) else {
                    return;
                };
                match key_event.state {
                    ElementState::Pressed => {
                        app.on_key_down(key, current_mods);
                    }
                    ElementState::Released => {
                        app.on_key_up(key, current_mods);
                    }
                }
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
                    &mut on_frame,
                    &mut layout_key,
                );
            }

            Event::AboutToWait => {
                if app.wants_draw() {
                    window.request_redraw();
                    elwt.set_control_flow(ControlFlow::Poll);
                } else if let Some(t) = app.next_draw_deadline() {
                    elwt.set_control_flow(ControlFlow::WaitUntil(t));
                } else {
                    elwt.set_control_flow(ControlFlow::Wait);
                }
            }

            _ => {}
        })
        .expect("event loop");
}

fn paint_frame(
    gpu: &Gpu,
    ctx: &mut WgpuGfxCtx,
    app: &mut App,
    win_w: u32,
    win_h: u32,
    on_frame: &mut impl FnMut(),
    layout_key: &mut Option<(u32, u32, u64, u64)>,
) {
    if win_w == 0 || win_h == 0 {
        return;
    }
    let frame = match gpu.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
        _ => return,
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    on_frame();

    ctx.set_surface_texture(frame.texture.clone());
    ctx.reset(win_w as f32, win_h as f32);
    begin_frame(ctx, view);

    // Skip layout when nothing that feeds it changed: same surface size,
    // same DPI, same invalidation epoch.
    let next_layout_key = (
        win_w,
        win_h,
        agg_gui::device_scale().to_bits(),
        agg_gui::animation::invalidation_epoch(),
    );
    if *layout_key != Some(next_layout_key) {
        app.layout(Size::new(win_w as f64, win_h as f64));
        *layout_key = Some(next_layout_key);
    }

    app.paint(ctx);
    ctx.end_frame();
    frame.present();
}
