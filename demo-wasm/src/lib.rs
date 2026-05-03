#![cfg(target_arch = "wasm32")]
//! WASM demo crate for agg-gui — winit-free browser shell + wgpu rendering.
//!
//! # Platform-split policy (kept identical across `demo-native`, `demo-wasm`)
//!
//! This crate is a **platform shell only** — canvas, browser events,
//! `localStorage` state.  It contains **no demo content**: every widget tree,
//! layout, and GPU renderer the user sees is shared via `demo-wgpu` (the wgpu
//! rendering library) and `demo-ui` (widget tree + layout).
//!
//! - **Widget / layout code** → `demo-ui`
//! - **GPU renderers (WGSL shaders, geometry, draw calls)** → `demo-wgpu`
//! - **Platform shell (canvas + event forwarding + persistence backend)** →
//!   here and `demo-native`
//!
//! `demo-wgpu` targets WebGL2 via wgpu on `wasm32-unknown-unknown` (no WebGPU
//! dependency), so the demo runs on every modern browser with WebGL2 support
//! (Chrome, Firefox, Safari).
//!
//! WASM exports:
//! - `render(width, height, frame_ms)` — full-frame render
//! - `on_mouse_move/down/up/wheel/leave` — mouse events
//! - `on_touch_start/move/end/cancel` — multi-touch events
//! - `on_key_down` — keyboard events
//! - `set_device_pixel_ratio` — DPR sync from the browser
//! - `needs_draw` — JS animation loop polls this to skip idle frames

mod clipboard_exports;
mod fonts;
mod platform;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, InspectorNode, InspectorOverlay, Key, Modifiers, MouseButton};
use demo_wgpu::{begin_frame, render_app_frame, WgpuCubeWidget, WgpuGfxCtx, CUBE_SCREEN_RECT};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

// ---------------------------------------------------------------------------
// Thread-local state
// ---------------------------------------------------------------------------

thread_local! {
    pub(crate) static DEMO_APP:  RefCell<Option<App>>          = RefCell::new(None);
    static WGPU_INIT: RefCell<Option<WgpuInit>>                = RefCell::new(None);
    static WGPU_CTX:  RefCell<Option<WgpuGfxCtx>>              = RefCell::new(None);

    static SHOW_INSPECTOR:    RefCell<Option<Rc<Cell<bool>>>>                                  = RefCell::new(None);
    static INSPECTOR_NODES:   RefCell<Option<Rc<RefCell<Vec<InspectorNode>>>>>                 = RefCell::new(None);
    static HOVERED_BOUNDS:    RefCell<Option<Rc<RefCell<Option<InspectorOverlay>>>>>           = RefCell::new(None);
    static BASE_EDITS:        RefCell<Option<Rc<RefCell<Vec<agg_gui::WidgetBaseEdit>>>>>       = RefCell::new(None);
    #[cfg(feature = "reflect")]
    static INSPECTOR_EDITS:   RefCell<Option<Rc<RefCell<Vec<agg_gui::InspectorEdit>>>>>        = RefCell::new(None);
    static SCREEN_SIZE:       RefCell<Option<Rc<Cell<(u32, u32)>>>>                            = RefCell::new(None);
    static STATE_ACCESSOR:    RefCell<Option<demo_ui::StateAccessor>>                          = RefCell::new(None);
    static FRAME_HISTORY:     RefCell<Option<Rc<RefCell<demo_ui::FrameHistory>>>>              = RefCell::new(None);
    static RUN_MODE:          RefCell<Option<Rc<Cell<demo_ui::RunMode>>>>                      = RefCell::new(None);
    static FRAME_COUNT:       Cell<u32> = Cell::new(0);
    static MOUSE_BUTTONS_DOWN: Cell<u32> = Cell::new(0);
    static AUTO_SAVE:         RefCell<agg_gui::persistence::AutoSave> = RefCell::new(agg_gui::persistence::AutoSave::new());
    static NEEDS_DRAW:        Cell<bool> = Cell::new(true);
    static CUBE_VISIBLE:      RefCell<Option<Rc<Cell<bool>>>> = RefCell::new(None);
}

/// All wgpu state that survives the async init and lives for the lifetime of
/// the page.  Held in [`WGPU_INIT`] after `wasm_start` resolves.
struct WgpuInit {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: wgpu::SurfaceConfiguration,
}

fn default_font() -> Arc<agg_gui::Font> {
    demo_ui::load_font_by_name(demo_ui::DEFAULT_FONT_NAME)
        .expect("default font must be installed before first render")
}

/// Set up the panic hook and kick off async wgpu init.
///
/// `wasm-bindgen(start)` runs this on module load.  Async init resolves via
/// `wasm_bindgen_futures::spawn_local`; until it completes, `render()` is a
/// no-op (the JS animation loop polls `needs_draw()` and harmlessly skips).
#[wasm_bindgen(start)]
pub fn wasm_start() {
    console_error_panic_hook::set_once();
    wasm_bindgen_futures::spawn_local(async {
        match init_wgpu_async().await {
            Ok(init) => WGPU_INIT.with(|c| *c.borrow_mut() = Some(init)),
            Err(err) => {
                web_sys::console::error_1(&JsValue::from_str(&format!("wgpu init failed: {err}")));
            }
        }
        // Force a redraw so the first frame paints once init resolves.
        mark_dirty();
    });
}

async fn init_wgpu_async() -> Result<WgpuInit, String> {
    let document = web_sys::window()
        .ok_or("no global window")?
        .document()
        .ok_or("no document")?;
    let canvas = document
        .get_element_by_id("canvas")
        .ok_or("canvas element not found")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "element is not a canvas")?;

    // wgpu's `webgl` feature targets `Backends::GL` on wasm32.  WebGPU is
    // intentionally NOT requested here so behaviour matches `demo-wasm` and
    // works on every browser with WebGL2 (vs. WebGPU, still uneven in 2026).
    let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_desc.backends = wgpu::Backends::GL;
    let instance = wgpu::Instance::new(instance_desc);

    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|e| format!("create_surface: {e:?}"))?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| format!("request_adapter: {e:?}"))?;

    // Critical for WebGL2: pin limits so the device only requests what WebGL2
    // can deliver — otherwise device creation rejects on backends that report
    // higher native limits via the underlying GPU.
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("demo-wasm-wgpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| format!("request_device: {e:?}"))?;

    let caps = surface.get_capabilities(&adapter);
    let surface_format = caps
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .unwrap_or(caps.formats[0]);

    let size = (
        canvas.width().max(1),
        canvas.height().max(1),
    );
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.0,
        height: size.1,
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

// ---------------------------------------------------------------------------
// State persistence helpers (localStorage)
// ---------------------------------------------------------------------------

fn load_state_wasm() -> Option<demo_ui::SavedState> {
    let storage = web_sys::window()?.local_storage().ok()??;
    let s = storage.get_item("agg-gui-demo-state").ok()??;
    demo_ui::SavedState::deserialize(&s)
}

fn ensure_demo_app() {
    DEMO_APP.with(|cell| {
        if cell.borrow().is_none() {
            let font = default_font();
            let initial_state = load_state_wasm();
            let running_msaa: u8 = 0; // wgpu+webgl: MSAA off in this initial port.
            let platform = demo_ui::PlatformHooks::web(running_msaa, || {
                if let Some(win) = web_sys::window() {
                    let _ = win.location().reload();
                }
            });
            let (app, handles) = demo_ui::build_demo_ui(
                Arc::clone(&font),
                Box::new(WgpuCubeWidget::new()),
                "wgpu / WebGL2",
                "Browser wgpu (WebGL2 backend)",
                initial_state,
                platform,
            );
            SHOW_INSPECTOR.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.show_inspector)));
            INSPECTOR_NODES.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.inspector_nodes)));
            HOVERED_BOUNDS.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.hovered_bounds)));
            BASE_EDITS.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.base_edits)));
            #[cfg(feature = "reflect")]
            INSPECTOR_EDITS
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.inspector_edits)));
            SCREEN_SIZE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screen_size)));
            FRAME_HISTORY.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.frame_history)));
            RUN_MODE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.run_mode)));
            CUBE_VISIBLE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.cube_visible)));
            STATE_ACCESSOR.with(|c| *c.borrow_mut() = Some(handles.state));
            *cell.borrow_mut() = Some(app);
        }
    });
}

fn ensure_wgpu_ctx(width: f32, height: f32) {
    WGPU_CTX.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if borrow.is_none() {
            WGPU_INIT.with(|init_cell| {
                if let Some(init) = init_cell.borrow().as_ref() {
                    let ctx = WgpuGfxCtx::new(
                        Arc::clone(&init.device),
                        Arc::clone(&init.queue),
                        init.surface_format,
                        width,
                        height,
                    );
                    *borrow = Some(ctx);
                }
            });
        }
    });
}

/// Reconfigure the wgpu surface for a new canvas size.  Called from `render`
/// when the JS-side caller's `width`/`height` differ from the configured
/// surface — the canvas typically resizes on browser zoom or window resize.
fn resize_surface_if_needed(width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }
    WGPU_INIT.with(|cell| {
        if let Some(init) = cell.borrow_mut().as_mut() {
            if init.config.width != width || init.config.height != height {
                init.config.width = width;
                init.config.height = height;
                init.surface.configure(&init.device, &init.config);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// WASM render export
// ---------------------------------------------------------------------------

/// Full-frame render.
///
/// Called from a JS animation loop each tick; returns immediately if init
/// hasn't completed (the harness polls `needs_draw()` to schedule retries).
#[wasm_bindgen]
pub fn render(width: u32, height: u32, frame_ms: f64) {
    // Skip until async init has resolved.
    let init_ready = WGPU_INIT.with(|c| c.borrow().is_some());
    if !init_ready {
        return;
    }

    ensure_demo_app();
    ensure_wgpu_ctx(width as f32, height as f32);
    resize_surface_if_needed(width, height);

    SCREEN_SIZE.with(|c| {
        if let Some(ref rc) = *c.borrow() {
            rc.set((width, height));
        }
    });
    CUBE_SCREEN_RECT.with(|r| r.set(agg_gui::Rect::default()));

    let show_inspector =
        SHOW_INSPECTOR.with(|c| c.borrow().as_ref().map(|r| r.get()).unwrap_or(false));
    let inspector_nodes_rc = INSPECTOR_NODES.with(|c| c.borrow().as_ref().map(Rc::clone));
    let hovered_bounds_rc = HOVERED_BOUNDS.with(|c| c.borrow().as_ref().map(Rc::clone));
    let base_edits_rc = BASE_EDITS.with(|c| c.borrow().as_ref().map(Rc::clone));
    #[cfg(feature = "reflect")]
    let inspector_edits_rc = INSPECTOR_EDITS.with(|c| c.borrow().as_ref().map(Rc::clone));

    let frame = WGPU_INIT.with(|c| {
        let init = c.borrow();
        let init = init.as_ref()?;
        match init.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => Some(f),
            _ => None,
        }
    });
    let Some(frame) = frame else { return };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    if let (Some(nodes), Some(hb), Some(base_edits)) =
        (inspector_nodes_rc, hovered_bounds_rc, base_edits_rc)
    {
        WGPU_CTX.with(|ctx_cell| {
            let mut ctx_borrow = ctx_cell.borrow_mut();
            if let Some(wgpu_ctx) = ctx_borrow.as_mut() {
                begin_frame(wgpu_ctx, view);
                DEMO_APP.with(|app_cell| {
                    let mut app_borrow = app_cell.borrow_mut();
                    if let Some(app) = app_borrow.as_mut() {
                        render_app_frame(
                            wgpu_ctx,
                            app,
                            width,
                            height,
                            frame_ms,
                            show_inspector,
                            &nodes,
                            &hb,
                            &base_edits,
                            #[cfg(feature = "reflect")]
                            inspector_edits_rc.as_ref().expect(
                                "INSPECTOR_EDITS must be initialised with reflect on",
                            ),
                        );
                    }
                });
                wgpu_ctx.end_frame();
            }
        });
    }
    frame.present();

    if frame_ms > 0.0 {
        FRAME_HISTORY.with(|c| {
            if let Some(ref rc) = *c.borrow() {
                rc.borrow_mut().push(frame_ms as f32);
            }
        });
    }

    FRAME_COUNT.set(FRAME_COUNT.get() + 1);
    let idle = MOUSE_BUTTONS_DOWN.get() == 0;
    STATE_ACCESSOR.with(|c| {
        if let Some(ref acc) = *c.borrow() {
            AUTO_SAVE.with_borrow_mut(|auto| {
                auto.tick(
                    idle,
                    || acc.current_state().serialize(),
                    |s| {
                        if let Some(storage) =
                            web_sys::window().and_then(|w| w.local_storage().ok().flatten())
                        {
                            let _ = storage.set_item("agg-gui-demo-state", s);
                        }
                    },
                );
            });
        }
    });

    NEEDS_DRAW.with(|c| c.set(false));
}

// ---------------------------------------------------------------------------
// Input exports — byte-for-byte mirrors of demo-wasm
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn set_device_pixel_ratio(dpr: f64) {
    agg_gui::set_device_scale(dpr.max(0.5));
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_mouse_move(x: f64, y: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_move(x, y);
        }
    });
    if let Some(window) = web_sys::window() {
        if let Some(doc) = window.document() {
            if let Some(el) = doc.get_element_by_id("canvas") {
                let style = agg_gui::web_adapter::cursor_style(agg_gui::current_cursor_icon());
                let _ = el.set_attribute("style", &style);
            }
        }
    }
}

#[wasm_bindgen]
pub fn on_mouse_down(x: f64, y: f64, button: u8) {
    MOUSE_BUTTONS_DOWN.set(MOUSE_BUTTONS_DOWN.get().saturating_add(1));
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_down(x, y, btn, Modifiers::default());
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_up(x: f64, y: f64, button: u8) {
    MOUSE_BUTTONS_DOWN.set(MOUSE_BUTTONS_DOWN.get().saturating_sub(1));
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_up(x, y, btn, Modifiers::default());
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_wheel(x: f64, y: f64, delta_y: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_wheel(x, y, delta_y);
        }
    });
}

#[wasm_bindgen]
pub fn on_mouse_leave() {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_mouse_leave();
        }
    });
}

#[wasm_bindgen]
pub fn on_touch_start(id: u32, x: f64, y: f64, force: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            let f = if force > 0.0 { Some(force as f32) } else { None };
            app.on_touch_start(
                agg_gui::TouchDeviceId(0),
                agg_gui::TouchId(id as u64),
                x,
                y,
                f,
            );
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_move(id: u32, x: f64, y: f64, force: f64) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            let f = if force > 0.0 { Some(force as f32) } else { None };
            app.on_touch_move(
                agg_gui::TouchDeviceId(0),
                agg_gui::TouchId(id as u64),
                x,
                y,
                f,
            );
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_end(id: u32) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_touch_end(agg_gui::TouchDeviceId(0), agg_gui::TouchId(id as u64));
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_cancel(id: u32) {
    DEMO_APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.on_touch_cancel(agg_gui::TouchDeviceId(0), agg_gui::TouchId(id as u64));
        }
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_key_down(key_str: &str, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    if let Some(key) = parse_js_key(key_str) {
        let mods = Modifiers { shift, ctrl, alt, meta };
        DEMO_APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.on_key_down(key, mods);
            }
        });
    }
}

#[wasm_bindgen]
pub fn needs_draw() -> bool {
    let continuous = RUN_MODE.with(|c| {
        c.borrow()
            .as_ref()
            .map(|rc| rc.get() == demo_ui::RunMode::Continuous)
            .unwrap_or(false)
    });
    if continuous {
        return true;
    }
    if NEEDS_DRAW.with(|c| c.get()) {
        return true;
    }
    let want = DEMO_APP.with(|c| c.borrow().as_ref().map(|a| a.wants_draw()).unwrap_or(false));
    want
}

pub(crate) fn mark_dirty() {
    NEEDS_DRAW.with(|c| c.set(true));
}

fn parse_js_key(key: &str) -> Option<Key> {
    agg_gui::web_adapter::key(key)
}
