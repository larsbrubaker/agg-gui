//! Native demo for agg-gui — winit + wgpu.
//!
//! # Platform-split policy (kept identical across `demo-native`, `demo-wasm`)
//!
//! This crate is a **platform shell only** — it wires up the OS window
//! (winit + wgpu surface), the event loop, and disk I/O for state
//! persistence.  It contains **no demo content**: every widget tree, layout,
//! and GPU renderer the user sees is shared via `demo-wgpu` (the wgpu
//! rendering library) and `demo-ui` (the widget tree + layout).
//!
//! - **Widget / layout code** → `demo-ui`
//! - **GPU renderers (WGSL shaders, geometry, draw calls)** → `demo-wgpu`
//!   (e.g. `WgpuCubeWidget`, the 3-D Animation widget)
//! - **Platform shell (OS window, event loop, persistence backend)** → here
//!   and `demo-wasm`
//!
//! # Scope
//!
//! Currently covers: window creation, wgpu device/surface init, per-frame
//! flush via `WgpuGfxCtx::end_frame`, resize, mouse/keyboard/wheel input,
//! and disk-backed state persistence (window size + open-windows + per-tab
//! open-positions diffed via `AutoSave`).  Future: fullscreen toggle,
//! screenshot capture, MSAA selection, touch events, hi-DPI scale tracking.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{winit_adapter, App, DrawCtx, Modifiers, Size};
use demo_wgpu::{begin_frame, render_app_frame, WgpuCubeWidget, WgpuGfxCtx};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Event, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::{Icon, Window, WindowAttributes};

const STATE_FILE_NAME: &str = ".agg-gui-demo-state";

const APP_ICON_SIZE: u32 = 256;
const APP_ICON_RGBA: &[u8] = include_bytes!("../assets/app-icon-256.rgba");

fn app_window_icon() -> Option<Icon> {
    Icon::from_rgba(APP_ICON_RGBA.to_vec(), APP_ICON_SIZE, APP_ICON_SIZE)
        .map_err(|err| eprintln!("failed to load app icon: {err}"))
        .ok()
}

fn state_file_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(STATE_FILE_NAME)))
        .unwrap_or_else(|| std::path::PathBuf::from(STATE_FILE_NAME))
}

fn load_saved_state() -> Option<demo_ui::SavedState> {
    let path = state_file_path();
    let s = std::fs::read_to_string(&path).ok()?;
    demo_ui::SavedState::deserialize(&s)
}

/// Build the serialized form of the current state.  Substitutes the
/// last-known windowed size when the window is currently maximized so we
/// don't persist the maximized-rect dimensions (those would be wrong to
/// restore on the next launch).
fn serialize_state(accessor: &demo_ui::StateAccessor, last_windowed: (u32, u32)) -> String {
    let mut state = accessor.current_state();
    if state.window_maximized || state.window_fullscreen {
        state.window_w = Some(last_windowed.0);
        state.window_h = Some(last_windowed.1);
    }
    state.serialize()
}

fn save_state_to_disk(text: &str) {
    let path = state_file_path();
    let _ = std::fs::write(&path, text);
}

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
            // `RENDER_ATTACHMENT` for the deferred 2-D + bar-grid passes;
            // `COPY_SRC` so `WgpuGfxCtx::read_screenshot` can blit the
            // post-render surface contents to a staging buffer for the
            // capture-pixels path.  The Take-Screenshot button needs this.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
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

    // Pull saved window size out of the state file BEFORE building the window
    // so we can apply it as initial attributes; full UI state is also handed
    // to `build_demo_ui` below to restore open windows / panels / positions.
    let initial_state = load_saved_state();
    let (start_w, start_h) = match initial_state.as_ref() {
        Some(s) => (s.window_w.unwrap_or(1280), s.window_h.unwrap_or(720)),
        None => (1280, 720),
    };
    let start_maximized = initial_state
        .as_ref()
        .map(|s| s.window_maximized)
        .unwrap_or(false);

    // Create the window HIDDEN.  We finish wgpu setup, build the demo UI,
    // and paint the first real frame BEFORE showing it — otherwise the user
    // briefly sees an unstyled OS-default white background plus a black
    // border around the not-yet-resized surface.
    let window_attributes = WindowAttributes::default()
        .with_title("agg-gui — Demo (wgpu)")
        .with_window_icon(app_window_icon())
        .with_inner_size(LogicalSize::new(start_w, start_h))
        .with_maximized(start_maximized)
        .with_visible(false);

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

    // Relaunch flag — set by the Render tab's Relaunch button via the closure
    // we hand to `PlatformHooks::native`.  Polled in `AboutToWait` so the
    // actual spawn+exit happens outside the event-dispatch frame, after state
    // flush.  Keeping the flag local to `main.rs` means demo-ui never imports
    // `std::process`.
    let relaunch_requested = Rc::new(std::cell::Cell::new(false));
    let running_msaa: u8 = initial_state.as_ref().map(|s| s.msaa_samples).unwrap_or(0);
    let platform = {
        let flag = Rc::clone(&relaunch_requested);
        demo_ui::PlatformHooks::native(running_msaa, move || flag.set(true))
            .with_font_requester(install_demo_font_asset)
    };

    // The cube widget takes a shared `Rc<Cell<u8>>` for the MSAA sample
    // count.  `build_demo_ui` builds that cell from the saved state and
    // hands it to our factory closure here, then re-uses the same cell
    // for the in-window MSAA toolbar — toggling there flips the cell,
    // the widget reads it on the next paint, and the bar-grid renderer
    // is rebuilt with the new sample count (no relaunch).
    let (mut app, handles) = demo_ui::build_demo_ui(
        Arc::clone(&font),
        Box::new(|msaa_cell| Box::new(WgpuCubeWidget::new(msaa_cell))),
        "wgpu",
        "native wgpu (winit)",
        initial_state,
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
    let window_maximized = Rc::clone(&handles.window_maximized);
    let screenshot_request = Rc::clone(&handles.screenshot_request);
    let screenshot_available = Rc::clone(&handles.screenshot_available);
    let screenshot_save_pending = Rc::clone(&handles.screenshot_save_pending);
    let screenshot_capture_seq = Rc::clone(&handles.screenshot_capture_seq);
    let screenshot_copy_pending = Rc::clone(&handles.screenshot_copy_pending);
    // `handles.screenshot_continuous` is consumed by `ImageView::paint` via
    // its own clone — the harness no longer needs to read it directly.
    let _screenshot_continuous = Rc::clone(&handles.screenshot_continuous);
    let state_accessor = handles.state;

    let mut win_w = gpu.config.width;
    let mut win_h = gpu.config.height;
    screen_size.set((win_w, win_h));

    // Last size seen while the window was NOT maximized — what we persist
    // across restarts.  Seeded with the saved windowed size (or default).
    let mut last_windowed_w: u32 = start_w;
    let mut last_windowed_h: u32 = start_h;

    // Auto-save tick: only writes when serialized state has actually changed
    // AND no mouse button is held (so we don't thrash disk mid-drag/resize).
    let mut auto_save = agg_gui::persistence::AutoSave::new();
    let mut mouse_buttons_down: u32 = 0;

    let mut cursor_x = 0.0f64;
    let mut cursor_y = 0.0f64;
    let mut current_mods = Modifiers::default();
    let mut last_frame_ms = 0.0f64;

    // Initial layout + first paint into the hidden window.  After this the
    // surface texture has the fully-styled first frame ready, so when we set
    // visible=true the user never sees an OS-default canvas flash.
    app.layout(Size::new(win_w as f64, win_h as f64));
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
        &screenshot_request,
        &screenshot_available,
        &screenshot_save_pending,
        &screenshot_copy_pending,
        &screenshot_capture_seq,
    );
    window.set_visible(true);

    event_loop
        .run(move |event, elwt| match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                let s = serialize_state(&state_accessor, (last_windowed_w, last_windowed_h));
                save_state_to_disk(&s);
                elwt.exit();
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(new_size),
                ..
            } => {
                if new_size.width > 0 && new_size.height > 0 {
                    gpu.resize(new_size.width, new_size.height);
                    win_w = new_size.width;
                    win_h = new_size.height;
                    screen_size.set((win_w, win_h));
                    let is_max = window.is_maximized();
                    window_maximized.set(is_max);
                    if !is_max {
                        last_windowed_w = win_w;
                        last_windowed_h = win_h;
                    }
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
                        mouse_buttons_down = mouse_buttons_down.saturating_add(1);
                        app.on_mouse_down(cursor_x, cursor_y, btn, current_mods);
                    }
                    ElementState::Released => {
                        mouse_buttons_down = mouse_buttons_down.saturating_sub(1);
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
                // DO NOT negate these values. winit's MouseScrollDelta is
                // already in the OS's scroll-direction convention — on
                // Windows the FlipFlopWheel registry setting (and any
                // per-driver "natural scroll" toggle) flips the sign of
                // WM_MOUSEWHEEL before winit sees it; on macOS NSEvent's
                // scrollingDeltaY honours System Settings → Trackpad →
                // Natural Scrolling. Passing the value straight through is
                // what makes the app respect the OS preference for both
                // old-school and natural-scroll users. This block has been
                // regressed multiple times by contributors "fixing" how
                // scrolling feels on their machine; if it feels backwards,
                // the OS preference is the source of truth — don't add a
                // sign flip here.
                let (mut dx, mut dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x as f64, y as f64),
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
                    &screenshot_request,
                    &screenshot_available,
                    &screenshot_save_pending,
                    &screenshot_copy_pending,
                    &screenshot_capture_seq,
                );
            }
            Event::AboutToWait => {
                // Continuous run mode keeps the app repainting unconditionally
                // (used by perf graphs etc.).  Continuous SCREENSHOT capture
                // is driven from inside `ImageView::paint` in the screenshot
                // demo — re-arming it from the harness here would un-scope
                // it from "window is open", so closing the screenshot
                // window with the checkbox still on would leave the host
                // loop spinning forever.
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
                        &screenshot_request,
                        &screenshot_available,
                        &screenshot_save_pending,
                        &screenshot_copy_pending,
                        &screenshot_capture_seq,
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

                // Diff serialized state against last-saved blob and write
                // only on change, gated on idle so a drag/resize doesn't
                // hammer the disk.
                auto_save.tick(
                    mouse_buttons_down == 0,
                    || serialize_state(&state_accessor, (last_windowed_w, last_windowed_h)),
                    |s| save_state_to_disk(s),
                );

                // Render-tab Relaunch button — flush state, spawn a fresh
                // copy of this executable, and exit the current one.  The
                // child reads the just-saved state (including the new MSAA
                // sample count) and applies it on its next surface
                // configuration.  Clear the flag BEFORE spawning so a
                // post-exit `AboutToWait` tick doesn't double-spawn.
                if relaunch_requested.get() {
                    relaunch_requested.set(false);
                    let s = serialize_state(&state_accessor, (last_windowed_w, last_windowed_h));
                    save_state_to_disk(&s);
                    if let Ok(exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    elwt.exit();
                }
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
    screenshot_request: &Rc<std::cell::Cell<bool>>,
    screenshot_available: &Rc<std::cell::Cell<bool>>,
    screenshot_save_pending: &Rc<std::cell::Cell<bool>>,
    screenshot_copy_pending: &Rc<std::cell::Cell<bool>>,
    screenshot_capture_seq: &Rc<std::cell::Cell<u64>>,
) {
    // GPU-direct screenshot flow: a single render per frame.  When a capture
    // is requested we issue ONE extra `copy_texture_to_texture` after
    // `end_frame()` and before `present()` — pure GPU work, no readback.
    // The screenshot widget's preview pane samples the capture texture
    // directly each frame via `DrawCtx::draw_captured_screenshot`.
    //
    // wgpu's swap chain takes ownership of the surface texture at
    // `frame.present()`, so the GPU copy MUST happen before present.
    let want_capture = screenshot_request.get();

    let Some(frame) = acquire_frame(gpu) else {
        return;
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    ctx.set_surface_texture(frame.texture.clone());
    begin_frame(ctx, view);
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
    ctx.end_frame();

    if want_capture {
        // Cheap GPU op: copy_texture_to_texture from surface into our
        // long-lived capture texture.  The screenshot widget will sample
        // the texture next frame and on every frame thereafter.
        if ctx.capture_screenshot() {
            screenshot_request.set(false);
            screenshot_available.set(true);
            // Bump the capture seq + wake the loop so `ImageView`'s
            // `needs_draw` flips true exactly once and the new
            // screenshot displays on the very next frame, instead of
            // waiting for an unrelated event (mouse hover, etc.) to
            // dirty the screenshot Window's backbuffer cache.
            screenshot_capture_seq.set(screenshot_capture_seq.get().wrapping_add(1));
            agg_gui::animation::request_draw();
        }
    }

    // Drain deferred Save / Copy actions — the click handlers can't read
    // pixels themselves (no `DrawCtx` access in event dispatch), so they
    // flip a pending flag and we run the GPU readback here, after
    // capture_screenshot has populated the capture texture.
    if screenshot_save_pending.replace(false) {
        let (rgba, sw, sh) = ctx.read_captured_screenshot();
        if !rgba.is_empty() {
            if let Err(err) =
                agg_gui::screenshot::download_rgba_as_png(&rgba, sw, sh, "agg-gui-screenshot.png")
            {
                eprintln!("screenshot save failed: {err}");
            }
        }
    }
    if screenshot_copy_pending.replace(false) {
        let (rgba, sw, sh) = ctx.read_captured_screenshot();
        if !rgba.is_empty() {
            if let Err(err) = agg_gui::screenshot::copy_rgba_to_clipboard(&rgba, sw, sh) {
                eprintln!("screenshot copy failed: {err}");
            }
        }
    }

    frame.present();
}

/// How to handle the result of `Surface::get_current_texture`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SurfaceAcquire {
    /// Texture is usable — render into it.
    Present,
    /// Surface configuration is stale (`Outdated`/`Lost`): reconfigure with
    /// the current config and try once more THIS frame.
    Reconfigure,
    /// Transient (`Timeout`/`Occluded`/`Validation`): skip the frame.
    Skip,
}

/// Decide how to handle a surface-acquire status.  Split out as a pure
/// function so the resize-recovery policy is unit-testable without a live GPU
/// surface (the no-payload variants are constructible in tests).
///
/// The key case is `Outdated`/`Lost`.  These fire right after a window resize
/// reconfigures the swapchain — the previously acquired textures no longer
/// match.  The old code lumped them into the catch-all `None` skip, so the
/// frame was dropped and the freshly-resized surface stayed BLACK until some
/// unrelated event (mouse move, hover) happened to request another redraw.
/// Reconfiguring and retrying in the same frame paints the new swapchain
/// immediately, so the resize lands a visible frame on the first try.
fn surface_acquire_action(status: &wgpu::CurrentSurfaceTexture) -> SurfaceAcquire {
    use wgpu::CurrentSurfaceTexture as T;
    match status {
        T::Success(_) | T::Suboptimal(_) => SurfaceAcquire::Present,
        T::Outdated | T::Lost => SurfaceAcquire::Reconfigure,
        T::Timeout | T::Occluded | T::Validation => SurfaceAcquire::Skip,
    }
}

/// Acquire the next surface texture, recovering from a stale swapchain by
/// reconfiguring and retrying once.  Returns `None` (skip frame) only for
/// genuinely transient failures so the caller never paints into a stale view.
fn acquire_frame(gpu: &Gpu) -> Option<wgpu::SurfaceTexture> {
    use wgpu::CurrentSurfaceTexture as T;
    let first = gpu.surface.get_current_texture();
    match surface_acquire_action(&first) {
        SurfaceAcquire::Present => match first {
            T::Success(f) | T::Suboptimal(f) => Some(f),
            _ => None,
        },
        SurfaceAcquire::Skip => None,
        SurfaceAcquire::Reconfigure => {
            // `configure` takes `&self`; the config is unchanged (the resize
            // handler already updated width/height) — we just re-bind it.
            gpu.surface.configure(&gpu.device, &gpu.config);
            match gpu.surface.get_current_texture() {
                T::Success(f) | T::Suboptimal(f) => Some(f),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{surface_acquire_action, SurfaceAcquire};
    use wgpu::CurrentSurfaceTexture as T;

    #[test]
    fn stale_swapchain_reconfigures_instead_of_skipping() {
        // This is the resize-black-screen regression: Outdated/Lost must drive
        // a reconfigure-and-retry, NOT a silent skip.
        assert_eq!(
            surface_acquire_action(&T::Outdated),
            SurfaceAcquire::Reconfigure
        );
        assert_eq!(surface_acquire_action(&T::Lost), SurfaceAcquire::Reconfigure);
    }

    #[test]
    fn transient_failures_skip_the_frame() {
        assert_eq!(surface_acquire_action(&T::Timeout), SurfaceAcquire::Skip);
        assert_eq!(surface_acquire_action(&T::Occluded), SurfaceAcquire::Skip);
        assert_eq!(surface_acquire_action(&T::Validation), SurfaceAcquire::Skip);
    }
}
