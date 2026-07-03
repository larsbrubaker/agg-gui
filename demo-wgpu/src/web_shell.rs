//! Turn-key browser (canvas + wgpu/WebGL2) shell for an agg-gui [`App`].
//!
//! The web equivalent of [`crate::native_shell`]. A wasm shim crate calls
//! [`start`] once from its `#[wasm_bindgen(start)]` and the shell owns
//! everything platform-generic:
//!
//! - client-platform / `(pointer: coarse)` detection → agg-gui platform,
//!   input profile, on-screen-keyboard enablement, and UX scale,
//! - device-pixel-ratio tracking (boot + window resizes),
//! - canvas backing-store sizing (`clientWidth × DPR`) every frame,
//! - wgpu surface/device init on the canvas and per-frame present,
//! - DOM pointer / wheel / context-menu listeners on the canvas,
//! - physical keyboard + clipboard via
//!   [`agg_gui::web_adapter::install_keyboard_listeners`],
//! - the `requestAnimationFrame` loop with layout caching and idle skip.
//!
//! The shim keeps only what is genuinely app-specific: its platform-trait
//! impl, extra `#[wasm_bindgen]` exports for app state (geolocation,
//! sensors, …), and a per-frame hook:
//!
//! ```ignore
//! #[wasm_bindgen(start)]
//! pub fn start() {
//!     demo_wgpu::web_shell::start(
//!         "my-canvas",
//!         || build_my_app(font, MyWasmPlatform::new()).0,
//!         || {},  // per-frame hook
//!     );
//! }
//! ```
//!
//! App exports mutate their own shared state cells and call
//! [`mark_dirty`]; exports that need the [`App`] itself use [`with_app`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, MouseButton, Size};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::{begin_frame, WgpuGfxCtx};

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static ON_FRAME: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
    static CANVAS_ID: RefCell<String> = const { RefCell::new(String::new()) };
    static WGPU_INIT: RefCell<Option<WgpuInit>> = const { RefCell::new(None) };
    static WGPU_CTX: RefCell<Option<WgpuGfxCtx>> = const { RefCell::new(None) };
    static NEEDS_DRAW: Cell<bool> = const { Cell::new(true) };
    static LAYOUT_KEY: Cell<Option<(u32, u32, u64, u64)>> = const { Cell::new(None) };
}

struct WgpuInit {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: wgpu::SurfaceConfiguration,
}

/// Request a repaint on the next animation frame. App-specific exports
/// call this after mutating state the widget tree reads.
pub fn mark_dirty() {
    NEEDS_DRAW.with(|c| c.set(true));
}

/// Run `f` with the shell-owned [`App`], if it is built and not already
/// borrowed (a poisoned borrow after a paint panic is skipped rather than
/// cascading).
pub fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|cell| {
        if let Ok(mut borrow) = cell.try_borrow_mut() {
            if let Some(app) = borrow.as_mut() {
                f(app);
            }
        }
    });
}

fn canvas() -> Option<web_sys::HtmlCanvasElement> {
    let id = CANVAS_ID.with(|c| c.borrow().clone());
    web_sys::window()?
        .document()?
        .get_element_by_id(&id)?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()
}

fn device_pixel_ratio() -> f64 {
    web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .filter(|d| *d > 0.0)
        .unwrap_or(1.0)
}

/// Detect the client platform once at boot: OS name for shortcut labels,
/// `(pointer: coarse)` for the touch input profile / on-screen keyboard /
/// UX scale. Mirrors what the reference JS harness computed and passed
/// through a `set_client_platform` export.
///
/// Debug override: `?agg_input=mobile` (or `=desktop`) in the page URL
/// forces the detection result, so a developer on a desktop browser can
/// exercise the mobile layout (left rails, on-screen keyboard, UX zoom)
/// without device emulation.
fn apply_client_platform() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let name = window.navigator().user_agent().unwrap_or_default();
    let mut pointer_coarse = window
        .match_media("(pointer: coarse)")
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false);
    if let Ok(search) = window.location().search() {
        if search.contains("agg_input=mobile") {
            pointer_coarse = true;
        } else if search.contains("agg_input=desktop") {
            pointer_coarse = false;
        }
    }
    agg_gui::set_platform(agg_gui::platform_from_name(&name));
    let profile = agg_gui::input_profile::input_profile_from_hint(&name, pointer_coarse);
    agg_gui::input_profile::set_input_profile(profile);
    agg_gui::widgets::on_screen_keyboard::set_enabled(profile.is_mobile_touch());
    // The UX zoom is applied only here — at the platform-shell boundary
    // where we genuinely know the device class — so programmatic profile
    // flips inside an app never silently resize the whole UI.
    agg_gui::ux_scale::set_ux_scale(profile.recommended_ux_scale());
}

/// Boot the shell: detect the platform, build the app, install input
/// listeners, kick off async wgpu init on `#canvas_id`, and start the
/// `requestAnimationFrame` loop. Call once from `#[wasm_bindgen(start)]`.
///
/// `on_frame` runs on every animation-frame tick, before the shell
/// decides whether to repaint — the hook for per-frame app state such as
/// advancing a wall-clock cell. An app that must repaint continuously
/// (e.g. its rendering depends on wall time) calls [`mark_dirty`] from
/// its hook; apps that don't get idle frames skipped for free.
pub fn start(
    canvas_id: &str,
    build_app: impl FnOnce() -> App + 'static,
    on_frame: impl FnMut() + 'static,
) {
    console_error_panic_hook::set_once();
    CANVAS_ID.with(|c| *c.borrow_mut() = canvas_id.to_string());

    // Platform facts must be in place before the widget tree is built —
    // apps size touch targets / pick icon-vs-label layouts off the input
    // profile at build time.
    apply_client_platform();
    agg_gui::set_device_scale(device_pixel_ratio().max(0.5));
    APP.with(|cell| *cell.borrow_mut() = Some(build_app()));
    ON_FRAME.with(|cell| *cell.borrow_mut() = Some(Box::new(on_frame)));

    agg_gui::web_adapter::install_keyboard_listeners(|key, mods, pressed| {
        with_app(|app| {
            if pressed {
                app.on_key_down(key, mods);
            } else {
                app.on_key_up(key, mods);
            }
        });
        mark_dirty();
    });
    install_pointer_listeners();
    install_resize_listener();

    wasm_bindgen_futures::spawn_local(async {
        match init_wgpu_async().await {
            Ok(init) => WGPU_INIT.with(|c| *c.borrow_mut() = Some(init)),
            Err(err) => web_sys::console::error_1(&JsValue::from_str(&format!(
                "agg-gui web_shell: wgpu init failed: {err}"
            ))),
        }
        mark_dirty();
    });

    start_raf_loop();
}

/// Canvas-local pointer position in physical pixels, Y-down — the input
/// space [`App`] expects.
fn pointer_pos(canvas: &web_sys::HtmlCanvasElement, e: &web_sys::PointerEvent) -> (f64, f64) {
    let rect = canvas.get_bounding_client_rect();
    let dpr = device_pixel_ratio();
    (
        (e.client_x() as f64 - rect.left()) * dpr,
        (e.client_y() as f64 - rect.top()) * dpr,
    )
}

fn pointer_modifiers(e: &web_sys::PointerEvent) -> agg_gui::Modifiers {
    agg_gui::Modifiers {
        shift: e.shift_key(),
        ctrl: e.ctrl_key(),
        alt: e.alt_key(),
        meta: e.meta_key(),
    }
}

fn mouse_button(e: &web_sys::PointerEvent) -> MouseButton {
    match e.button() {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n.clamp(0, 255) as u8),
    }
}

fn add_canvas_listener<E>(
    canvas: &web_sys::HtmlCanvasElement,
    event: &str,
    handler: impl FnMut(E) + 'static,
) where
    E: wasm_bindgen::convert::FromWasmAbi + 'static,
{
    let cb = Closure::<dyn FnMut(E)>::new(handler);
    let _ = canvas.add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
    cb.forget();
}

fn install_pointer_listeners() {
    let Some(canvas) = canvas() else {
        return;
    };

    {
        let canvas_ref = canvas.clone();
        add_canvas_listener(&canvas, "pointermove", move |e: web_sys::PointerEvent| {
            let (x, y) = pointer_pos(&canvas_ref, &e);
            with_app(|app| app.on_mouse_move(x, y));
            // Reflect the hovered widget's preferred cursor on the canvas.
            let icon = agg_gui::current_cursor_icon();
            let _ = canvas_ref
                .style()
                .set_property("cursor", icon.to_css());
        });
    }
    {
        let canvas_ref = canvas.clone();
        add_canvas_listener(&canvas, "pointerdown", move |e: web_sys::PointerEvent| {
            // Capture so drags keep reporting positions outside the canvas.
            let _ = canvas_ref.set_pointer_capture(e.pointer_id());
            let (x, y) = pointer_pos(&canvas_ref, &e);
            with_app(|app| app.on_mouse_down(x, y, mouse_button(&e), pointer_modifiers(&e)));
            mark_dirty();
        });
    }
    for release_event in ["pointerup", "pointercancel"] {
        let canvas_ref = canvas.clone();
        add_canvas_listener(&canvas, release_event, move |e: web_sys::PointerEvent| {
            let (x, y) = pointer_pos(&canvas_ref, &e);
            with_app(|app| app.on_mouse_up(x, y, mouse_button(&e), pointer_modifiers(&e)));
            mark_dirty();
        });
    }
    {
        let canvas_ref = canvas.clone();
        add_canvas_listener(&canvas, "wheel", move |e: web_sys::WheelEvent| {
            e.prevent_default();
            let rect = canvas_ref.get_bounding_client_rect();
            let dpr = device_pixel_ratio();
            let x = (e.client_x() as f64 - rect.left()) * dpr;
            let y = (e.client_y() as f64 - rect.top()) * dpr;
            // Browser deltaY is positive-scroll-DOWN; App expects positive =
            // wheel rotated forward (winit convention). deltaMode 0 = pixels
            // (~40 px per line); other modes are already line-ish.
            let scale = if e.delta_mode() == 0 { 40.0 } else { 1.0 };
            let mods = agg_gui::Modifiers {
                shift: e.shift_key(),
                ctrl: e.ctrl_key(),
                alt: e.alt_key(),
                meta: e.meta_key(),
            };
            with_app(|app| {
                app.on_mouse_wheel_xy_mods(x, y, -e.delta_x() / scale, -e.delta_y() / scale, mods);
            });
            mark_dirty();
        });
    }
    add_canvas_listener(&canvas, "contextmenu", move |e: web_sys::Event| {
        e.prevent_default();
    });
}

/// Keep the device scale fresh across window resizes / monitor moves /
/// browser zoom changes (all of which can change `devicePixelRatio`).
fn install_resize_listener() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let cb = Closure::<dyn FnMut()>::new(|| {
        agg_gui::set_device_scale(device_pixel_ratio().max(0.5));
        mark_dirty();
    });
    let _ = window.add_event_listener_with_callback("resize", cb.as_ref().unchecked_ref());
    cb.forget();
}

#[derive(Debug)]
struct WebDisplay;

impl wgpu::rwh::HasDisplayHandle for WebDisplay {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        Ok(wgpu::rwh::DisplayHandle::web())
    }
}

async fn init_wgpu_async() -> Result<WgpuInit, String> {
    let canvas = canvas().ok_or_else(|| {
        let id = CANVAS_ID.with(|c| c.borrow().clone());
        format!("canvas element #{id} not found")
    })?;

    let mut instance_desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(WebDisplay));
    instance_desc.backends = wgpu::Backends::GL;
    let instance = wgpu::Instance::new(instance_desc);
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|err| format!("create_surface: {err:?}"))?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|err| format!("request_adapter: {err:?}"))?;

    let adapter_limits = adapter.limits();
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("agg-gui-web-shell"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter_limits),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|err| format!("request_device: {err:?}"))?;

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
        width: canvas.width().max(1),
        height: canvas.height().max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        desired_maximum_frame_latency: 2,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
    };
    surface.configure(&device, &config);

    Ok(WgpuInit {
        device: Arc::new(device),
        queue: Arc::new(queue),
        surface,
        surface_format,
        config,
    })
}

/// Self-rescheduling `requestAnimationFrame` loop.
fn start_raf_loop() {
    fn schedule(cb: &Rc<RefCell<Option<Closure<dyn FnMut()>>>>) {
        let borrow = cb.borrow();
        if let (Some(window), Some(closure)) = (web_sys::window(), borrow.as_ref()) {
            let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
        }
    }

    let cb: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let cb_clone = Rc::clone(&cb);
    *cb.borrow_mut() = Some(Closure::new(move || {
        frame();
        schedule(&cb_clone);
    }));
    schedule(&cb);
}

/// One animation-frame tick: size the canvas backing store, then paint if
/// anything wants a frame.
fn frame() {
    let Some(canvas) = canvas() else {
        return;
    };

    // Match the backing store to the CSS size × DPR so rendering is
    // pixel-perfect at any zoom / DPI / orientation. The device scale is
    // re-synced every tick, not just at boot / window-resize: browser
    // zoom and monitor moves can change `devicePixelRatio` without a
    // resize event, and a stale scale renders the whole UI at the wrong
    // density.
    let dpr = device_pixel_ratio();
    if (agg_gui::device_scale() - dpr).abs() > 1e-9 {
        agg_gui::set_device_scale(dpr.max(0.5));
        mark_dirty();
    }
    let w = ((canvas.client_width() as f64 * dpr) as u32).max(1);
    let h = ((canvas.client_height() as f64 * dpr) as u32).max(1);
    if canvas.width() != w || canvas.height() != h {
        canvas.set_width(w);
        canvas.set_height(h);
        mark_dirty();
    }

    // The app hook runs every tick — not just on painted frames — so a
    // wall-clock-driven app can advance its state and `mark_dirty()` to
    // keep the loop hot.
    ON_FRAME.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });

    let wants = NEEDS_DRAW.with(|c| c.get())
        || APP.with(|c| {
            c.try_borrow()
                .ok()
                .and_then(|b| b.as_ref().map(|a| a.wants_draw()))
                .unwrap_or(false)
        });
    if !wants {
        return;
    }

    paint(w, h);
}

fn paint(w: u32, h: u32) {
    // Reconfigure the surface if the canvas size changed; create the draw
    // ctx on first use once async wgpu init has landed.
    let ready = WGPU_INIT.with(|init_cell| {
        let mut init = init_cell.borrow_mut();
        let Some(init) = init.as_mut() else {
            return false;
        };
        if init.config.width != w || init.config.height != h {
            init.config.width = w;
            init.config.height = h;
            init.surface.configure(&init.device, &init.config);
            WGPU_CTX.with(|ctx_cell| {
                if let Some(ctx) = ctx_cell.borrow_mut().as_mut() {
                    ctx.reset(w as f32, h as f32);
                }
            });
        }
        WGPU_CTX.with(|ctx_cell| {
            if ctx_cell.borrow().is_none() {
                *ctx_cell.borrow_mut() = Some(WgpuGfxCtx::new(
                    Arc::clone(&init.device),
                    Arc::clone(&init.queue),
                    init.surface_format,
                    w as f32,
                    h as f32,
                ));
            }
        });
        true
    });
    if !ready {
        return;
    }

    let frame = WGPU_INIT.with(|init_cell| {
        let init = init_cell.borrow();
        let init = init.as_ref()?;
        match init.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(f),
            _ => None,
        }
    });
    let Some(frame) = frame else {
        return;
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    NEEDS_DRAW.with(|c| c.set(false));

    APP.with(|app_cell| {
        let Ok(mut borrow) = app_cell.try_borrow_mut() else {
            return;
        };
        let Some(app) = borrow.as_mut() else {
            return;
        };
        // Skip layout when nothing that feeds it changed: same surface
        // size, same DPI, same invalidation epoch.
        let layout_key = (
            w,
            h,
            agg_gui::device_scale().to_bits(),
            agg_gui::animation::invalidation_epoch(),
        );
        if LAYOUT_KEY.with(|last| last.get()) != Some(layout_key) {
            app.layout(Size::new(w as f64, h as f64));
            LAYOUT_KEY.with(|last| last.set(Some(layout_key)));
        }

        WGPU_CTX.with(|ctx_cell| {
            if let Some(ctx) = ctx_cell.borrow_mut().as_mut() {
                ctx.set_surface_texture(frame.texture.clone());
                ctx.reset(w as f32, h as f32);
                begin_frame(ctx, view);
                app.paint(ctx);
                ctx.end_frame();
            }
        });
    });

    frame.present();
}
