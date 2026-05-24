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
mod input;
mod platform;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{App, InspectorNode, InspectorOverlay};
use demo_wgpu::{
    begin_frame, render_app_frame, MsaaFramebuffer, WgpuCubeWidget, WgpuGfxCtx, CUBE_SCREEN_RECT,
};
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

    // ── Screenshot handles ──────────────────────────────────────────────
    /// Set by the screenshot-demo "Take Screenshot" button.  `render` picks
    /// it up after the scene-buffer paint finishes and triggers a GPU-only
    /// `copy_texture_to_texture(scene → capture)` via `capture_screenshot`.
    static SCREENSHOT_REQUEST:        RefCell<Option<Rc<Cell<bool>>>> = RefCell::new(None);
    /// Mirrored from the "Capture continuously" checkbox.  When set,
    /// `render` re-arms `SCREENSHOT_REQUEST` every frame.
    static SCREENSHOT_CONTINUOUS:     RefCell<Option<Rc<Cell<bool>>>> = RefCell::new(None);
    /// Lights up the Save / Copy buttons after the first successful capture.
    static SCREENSHOT_AVAILABLE:      RefCell<Option<Rc<Cell<bool>>>> = RefCell::new(None);
    /// Click-deferred Save: button toggles this on; `render` drains it after
    /// paint, runs `read_captured_screenshot`, encodes PNG, triggers download.
    static SCREENSHOT_SAVE_PENDING:   RefCell<Option<Rc<Cell<bool>>>> = RefCell::new(None);
    /// Click-deferred Copy: same pattern, pipes bytes to the system clipboard.
    static SCREENSHOT_COPY_PENDING:   RefCell<Option<Rc<Cell<bool>>>> = RefCell::new(None);
    /// Monotonic counter the harness increments after a successful capture.
    /// `ImageView`'s `needs_draw` checks for a mismatch against its own
    /// `last_seen_seq` so the screenshot Window's backbuffer invalidates
    /// exactly once per capture and the new screenshot displays
    /// immediately, instead of waiting for an unrelated event.
    static SCREENSHOT_CAPTURE_SEQ:    RefCell<Option<Rc<Cell<u64>>>> = RefCell::new(None);
    /// Intermediate "scene" framebuffer that rendering targets every frame
    /// before being blit-displayed onto the real swap-chain surface.
    /// Required because WebGL2 surfaces only advertise `COLOR_TARGET` —
    /// without an intermediate, the GPU-direct screenshot path
    /// (`copy_texture_to_texture(surface → capture)`) fails validation.
    /// The scene texture is `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC`,
    /// so screenshots copy from it cleanly.
    static SCENE_FB:          RefCell<Option<MsaaFramebuffer>> = RefCell::new(None);
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
    // Chain a tess2 input-dumping hook on top of console_error_panic_hook
    // so that when the unresolved tess2-rust mesh-op panics fire on this
    // wasm build, the offending contour set is logged to console.error
    // *before* the page aborts.  Native catch_unwind already covers the
    // non-wasm side; this is the wasm-only path because
    // wasm32-unknown-unknown has no unwinder runtime and panics always
    // abort, making catch_unwind a no-op.
    agg_gui::gl_renderer::install_tess_panic_logger();
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

/// Zero-sized `HasDisplayHandle` impl that returns the `Web` display handle
/// variant.  Plumbed into `InstanceDescriptor::new_with_display_handle` to
/// work around wgpu 29's `MissingDisplayHandle` rejection of canvas
/// surfaces (they have no real display, but wgpu-core requires one of the
/// two display sources to be `Some`).  Trivially `Send + Sync` since it
/// holds no state.
#[derive(Debug)]
struct WebDisplay;

impl wgpu::rwh::HasDisplayHandle for WebDisplay {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        Ok(wgpu::rwh::DisplayHandle::web())
    }
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
    //
    // wgpu 29 has a quirk in `wgpu_core::Instance::create_surface`: if BOTH
    // the instance's display handle AND the surface target's display handle
    // are `None`, it returns `MissingDisplayHandle` — even though
    // `SurfaceTarget::Canvas` legitimately has no display handle (canvases
    // bind to a window-like handle, not a display).  Workaround: hand the
    // instance a no-op `Web` display handle so the `(None, None)` branch
    // doesn't fire.  Cost is one zero-sized boxed shim.
    let mut instance_desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(WebDisplay));
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
    //
    // Resolution caveat: the raw `downlevel_webgl2_defaults` pins
    // `max_texture_dimension_2d = 2048`, which an Android phone's canvas
    // routinely overshoots (a 411 × 731 CSS-px viewport at DPR 3 is
    // 1233 × 2193 device px — already past 2048 on the long axis).  When the
    // surface texture exceeds the device limit, `surface.configure` and the
    // scene-framebuffer texture creation both fail validation and the canvas
    // stays black.  `using_resolution(adapter.limits())` keeps every other
    // conservative WebGL2 default but raises the texture-dimension caps to
    // whatever the actual adapter advertises (typically 4096–16384 on
    // mobile WebGL2).
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("demo-wasm-wgpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
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

    let size = (canvas.width().max(1), canvas.height().max(1));
    let config = wgpu::SurfaceConfiguration {
        // WebGL2 surfaces only advertise `COLOR_TARGET` (== RENDER_ATTACHMENT);
        // requesting `COPY_SRC` panics at `Surface::configure` validation.
        // The native shell adds `COPY_SRC` so screenshots can copy the
        // surface texture, but on WASM that path isn't available — the
        // browser-side capture story (via canvas.toBlob / readPixels in
        // GL) goes through a different mechanism, so we simply omit it.
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
            let running_msaa: u8 = initial_state.as_ref().map(|s| s.msaa_samples).unwrap_or(0);
            let platform = demo_ui::PlatformHooks::web(running_msaa, || {
                if let Some(win) = web_sys::window() {
                    let _ = win.location().reload();
                }
            });
            // The cube widget takes a shared `Rc<Cell<u8>>` for the MSAA
            // sample count, built by `build_demo_ui` from the saved state
            // and reused by the in-window MSAA toolbar — same live-toggle
            // path as the native shell.
            let (app, handles) = demo_ui::build_demo_ui(
                Arc::clone(&font),
                Box::new(|msaa_cell| Box::new(WgpuCubeWidget::new(msaa_cell))),
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
            INSPECTOR_EDITS.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.inspector_edits)));
            SCREEN_SIZE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screen_size)));
            FRAME_HISTORY.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.frame_history)));
            RUN_MODE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.run_mode)));
            CUBE_VISIBLE.with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.cube_visible)));
            SCREENSHOT_REQUEST
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_request)));
            SCREENSHOT_CONTINUOUS
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_continuous)));
            SCREENSHOT_AVAILABLE
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_available)));
            SCREENSHOT_SAVE_PENDING
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_save_pending)));
            SCREENSHOT_COPY_PENDING
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_copy_pending)));
            SCREENSHOT_CAPTURE_SEQ
                .with(|c| *c.borrow_mut() = Some(Rc::clone(&handles.screenshot_capture_seq)));
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

/// Lazily allocate the scene framebuffer the first time it's requested,
/// resizing on subsequent calls when the surface size changes.
fn ensure_scene_fb(width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }
    SCENE_FB.with(|cell| {
        WGPU_INIT.with(|init_cell| {
            let init_borrow = init_cell.borrow();
            let Some(init) = init_borrow.as_ref() else {
                return;
            };
            let mut fb_borrow = cell.borrow_mut();
            match fb_borrow.as_mut() {
                Some(fb) => fb.ensure_size(&init.device, width, height),
                None => {
                    *fb_borrow = Some(MsaaFramebuffer::new(
                        &init.device,
                        width,
                        height,
                        /* sample_count = */ 1,
                        init.surface_format,
                        /* with_depth = */ false,
                    ));
                }
            }
        });
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
    ensure_scene_fb(width, height);
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

    // Continuous capture re-arming is driven by `ImageView::paint` inside
    // the screenshot demo — keeping it scoped to "screenshot window is
    // open" so closing the window genuinely idles the host loop.  See
    // the comment on `ImageView.continuous` for why the harness must
    // not re-arm here.

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
    let surface_view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    if let (Some(nodes), Some(hb), Some(base_edits)) =
        (inspector_nodes_rc, hovered_bounds_rc, base_edits_rc)
    {
        // Render every frame into the intermediate scene framebuffer
        // instead of straight to the swap-chain surface.  The scene
        // texture is `RENDER_ATTACHMENT | TEXTURE_BINDING | COPY_SRC`,
        // so it can serve as the source of the GPU-direct screenshot
        // copy — the WebGL2 surface texture only advertises
        // `COLOR_TARGET`, which is why a direct
        // `copy_texture_to_texture(surface → capture)` fails on WASM.
        // After the deferred command list flushes, we run a textured-
        // quad blit pass on the scene to display it on the real surface.
        SCENE_FB.with(|fb_cell| {
            let fb_borrow = fb_cell.borrow();
            let Some(scene_fb) = fb_borrow.as_ref() else {
                return;
            };
            let scene_view = scene_fb.render_view().clone();
            let scene_texture = scene_fb.resolve_texture().clone();

            WGPU_CTX.with(|ctx_cell| {
                let mut ctx_borrow = ctx_cell.borrow_mut();
                if let Some(wgpu_ctx) = ctx_borrow.as_mut() {
                    // `set_surface_texture` is what `capture_screenshot`
                    // copies from — point it at the scene texture so
                    // screenshots succeed.
                    wgpu_ctx.set_surface_texture(scene_texture);
                    begin_frame(wgpu_ctx, scene_view);
                    // `try_borrow_mut` so a previously-panicked event
                    // handler that left `DEMO_APP` borrowed silently
                    // skips the render-app step instead of panicking
                    // again.  The end-of-frame blit + screenshot
                    // bookkeeping below still runs so the swapchain
                    // doesn't stall on its current-texture handle.
                    DEMO_APP.with(|app_cell| {
                        if let Ok(mut app_borrow) = app_cell.try_borrow_mut() {
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
                        }
                    });
                    wgpu_ctx.end_frame();

                    // GPU-direct screenshot: AFTER `end_frame` (so the
                    // scene texture has the rendered content), if a
                    // capture was requested do the cheap
                    // `copy_texture_to_texture(scene → capture)`.  Pixels
                    // stay on the GPU; the screenshot demo's preview pane
                    // samples the capture texture directly via
                    // `DrawCtx::draw_captured_screenshot`.
                    let want_capture = SCREENSHOT_REQUEST
                        .with(|c| c.borrow().as_ref().map(|r| r.get()).unwrap_or(false));
                    if want_capture {
                        use agg_gui::DrawCtx;
                        if wgpu_ctx.capture_screenshot() {
                            SCREENSHOT_REQUEST.with(|c| {
                                if let Some(ref rc) = *c.borrow() {
                                    rc.set(false);
                                }
                            });
                            SCREENSHOT_AVAILABLE.with(|c| {
                                if let Some(ref rc) = *c.borrow() {
                                    rc.set(true);
                                }
                            });
                            // Bump capture seq + wake the loop so
                            // `ImageView::needs_draw` flips true exactly
                            // once and the new screenshot displays on the
                            // very next frame.
                            SCREENSHOT_CAPTURE_SEQ.with(|c| {
                                if let Some(ref rc) = *c.borrow() {
                                    rc.set(rc.get().wrapping_add(1));
                                }
                            });
                            mark_dirty();
                        }
                    }

                    // Drain deferred Save / Copy.  Click handlers can't
                    // read pixels themselves (no `DrawCtx` access in event
                    // dispatch); they flip a pending flag and we
                    // round-trip pixels through `read_captured_screenshot`
                    // here, then hand the bytes to the cross-platform
                    // download / clipboard helpers.
                    let save_pending = SCREENSHOT_SAVE_PENDING.with(|c| {
                        c.borrow()
                            .as_ref()
                            .map(|r| r.replace(false))
                            .unwrap_or(false)
                    });
                    let copy_pending = SCREENSHOT_COPY_PENDING.with(|c| {
                        c.borrow()
                            .as_ref()
                            .map(|r| r.replace(false))
                            .unwrap_or(false)
                    });
                    if save_pending || copy_pending {
                        use agg_gui::DrawCtx;
                        let (rgba, sw, sh) = wgpu_ctx.read_captured_screenshot();
                        if !rgba.is_empty() {
                            if save_pending {
                                if let Err(err) = agg_gui::screenshot::download_rgba_as_png(
                                    &rgba,
                                    sw,
                                    sh,
                                    "agg-gui-screenshot.png",
                                ) {
                                    web_sys::console::error_1(&JsValue::from_str(&format!(
                                        "screenshot save failed: {err}"
                                    )));
                                }
                            }
                            if copy_pending {
                                if let Err(err) =
                                    agg_gui::screenshot::copy_rgba_to_clipboard(&rgba, sw, sh)
                                {
                                    web_sys::console::error_1(&JsValue::from_str(&format!(
                                        "screenshot copy failed: {err}"
                                    )));
                                }
                            }
                        }
                    }

                    // Display: blit the scene onto the actual surface
                    // through the shared 2-D textured-quad pipeline.
                    let device = Arc::clone(&wgpu_ctx.device());
                    let queue = Arc::clone(&wgpu_ctx.queue());
                    let mut encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("scene_blit_to_surface"),
                        });
                    let dst_rect = agg_gui::Rect::new(0.0, 0.0, width as f64, height as f64);
                    scene_fb.blit_to(
                        &device,
                        &mut encoder,
                        &surface_view,
                        (width, height),
                        dst_rect,
                        None,
                        wgpu_ctx.pipelines(),
                    );
                    queue.submit(std::iter::once(encoder.finish()));
                }
            });
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

pub(crate) fn mark_dirty() {
    NEEDS_DRAW.with(|c| c.set(true));
}
