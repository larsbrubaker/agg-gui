//! The `requestAnimationFrame` loop: per-tick canvas sizing and DPR sync,
//! the paint decision ([`crate::policy`]), one painted frame, and the
//! device-loss rebuild.
//!
//! Owns the GPU-side state ([`Painter`]) in its own thread-local; the app and
//! host live in [`super`]'s cells. Order per tick mirrors the native shell's
//! loop: size → sensors/fullscreen → `on_tick` → decide → (`on_frame` →
//! `paint` → `end_frame` → `after_paint` → present → `after_present`) →
//! `on_idle`. Every decision is delegated to a platform-neutral module
//! (`dom_math::fit_backing`, `policy`, `recovery`) that is unit tested.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use agg_gui::App;
use agg_gui_wgpu::{SsaaFramebuffer, WgpuGfxCtx};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use super::gpu::{Acquire, WebGpu};
use super::{lifecycle, mark_dirty, report_fatal, INPUT_SINCE_FRAME, POINTERS, POLICY, PRESENTED};
use super::{platform, sensors, APP, CANVAS, CONFIG, DIRTY, FIRST_PAINT, HOST};
use crate::dom_math::{fit_backing, sanitize_dpr};
use crate::error::WebShellError;
use crate::host::{CanvasGeometry, Frame, WebShellControl, WebShellHost};
use crate::policy::{layout_key, policy_after_idle, wants_paint, GeometryTracker, LayoutKey};
use crate::recovery::{RebuildBackoff, RebuildVerdict};

/// GPU-side per-canvas state.
struct Painter {
    gpu: WebGpu,
    ctx: WgpuGfxCtx,
    /// Offscreen scene target when `WebShellConfig::offscreen_scene` is on.
    scene: Option<SsaaFramebuffer>,
    layout_key: Option<LayoutKey>,
    frames: u64,
    last_duration: Duration,
}

enum GpuState {
    /// Async init has not resolved yet.
    Pending,
    Ready(Box<Painter>),
    /// A device-loss rebuild is in flight; the painter is inside the future.
    Rebuilding,
    /// The GPU is gone for good (rebuild gave up, WebGL2 context lost); the
    /// fatal path has run and nothing paints again.
    Failed,
}

thread_local! {
    static GPU: RefCell<GpuState> = const { RefCell::new(GpuState::Pending) };
    static GEOMETRY: RefCell<GeometryTracker> = const { RefCell::new(GeometryTracker::new()) };
    static BACKOFF: RefCell<RebuildBackoff> = const { RefCell::new(RebuildBackoff::new()) };
    /// The device's `max_texture_dimension_2d`; unbounded until the GPU is up.
    static MAX_DIM: std::cell::Cell<u32> = const { std::cell::Cell::new(u32::MAX) };
    /// Physical px per CSS px of the current backing store — the DPR, or less
    /// when the texture limit forced a smaller surface. Input maps with it.
    static SCALE: std::cell::Cell<f64> = const { std::cell::Cell::new(1.0) };
}

fn new_ctx(gpu: &WebGpu) -> WgpuGfxCtx {
    WgpuGfxCtx::new(
        Arc::clone(&gpu.device),
        Arc::clone(&gpu.queue),
        gpu.config.format,
        gpu.config.width as f32,
        gpu.config.height as f32,
    )
}

pub(super) fn install_gpu(gpu: WebGpu) {
    MAX_DIM.with(|m| m.set(gpu.device.limits().max_texture_dimension_2d));
    let ctx = new_ctx(&gpu);
    GPU.with(|g| {
        *g.borrow_mut() = GpuState::Ready(Box::new(Painter {
            gpu,
            ctx,
            scene: None,
            layout_key: None,
            frames: 0,
            last_duration: Duration::ZERO,
        }))
    });
}

pub(super) fn current_dpr() -> f64 {
    sanitize_dpr(
        web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(1.0),
    )
}

/// Physical px per CSS px of the canvas backing store, for pointer mapping.
pub(super) fn input_scale() -> f64 {
    SCALE.with(|s| s.get())
}

/// Match the canvas backing store to `clientSize × DPR`, fitted to the
/// device's texture limit ([`fit_backing`] — the single source of truth, so
/// the surface `configure` never disagrees with it and resizes every tick).
/// Returns the size, the scale it was built at, and whether it changed.
pub(super) fn size_backing_store(canvas: &web_sys::HtmlCanvasElement) -> ((u32, u32), f64, bool) {
    let ((w, h), scale) = fit_backing(
        canvas.client_width() as f64,
        canvas.client_height() as f64,
        current_dpr(),
        MAX_DIM.with(|m| m.get()),
    );
    SCALE.with(|s| s.set(scale));
    let changed = canvas.width() != w || canvas.height() != h;
    if changed {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    ((w, h), scale, changed)
}

/// Self-rescheduling `requestAnimationFrame` loop.
pub(super) fn start_raf_loop() {
    type RafSlot = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;
    fn schedule(cb: &RafSlot) {
        let borrow = cb.borrow();
        if let (Some(window), Some(closure)) = (web_sys::window(), borrow.as_ref()) {
            let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
        }
    }
    let cb: RafSlot = Rc::new(RefCell::new(None));
    let cb_clone = Rc::clone(&cb);
    *cb.borrow_mut() = Some(Closure::new(move || {
        tick();
        schedule(&cb_clone);
    }));
    schedule(&cb);
}

/// Run `f` with the app and host, skipping when either is missing or borrowed.
fn with_app_host(f: impl FnOnce(&mut App, &mut dyn WebShellHost)) {
    APP.with(|a| {
        HOST.with(|h| {
            let (Ok(mut app), Ok(mut host)) = (a.try_borrow_mut(), h.try_borrow_mut()) else {
                return;
            };
            if let (Some(app), Some(host)) = (app.as_mut(), host.as_mut()) {
                f(app, host.as_mut());
            }
        })
    });
}

fn tick() {
    let Some(canvas) = CANVAS.with(|c| c.borrow().clone()) else {
        return;
    };
    let ((w, h), dpr, resized) = size_backing_store(&canvas);
    if (agg_gui::device_scale() - dpr).abs() > 1e-9 {
        agg_gui::set_device_scale(dpr);
        mark_dirty();
    }

    sensors::service_tilt_requests();
    sensors::poll_gamepads();
    let fullscreen = platform::service_fullscreen(&canvas);
    report_geometry(w, h, dpr, fullscreen);

    with_app_host(|app, host| host.on_tick(app));

    if !service_device_loss() {
        return;
    }

    let ready = GPU.with(|g| matches!(*g.borrow(), GpuState::Ready(_)));
    let mut painted = false;
    if ready {
        let should = FIRST_PAINT.with(|gate| {
            gate.should_paint_tick(resized, || {
                let dirty = DIRTY.with(|d| d.get());
                let policy = POLICY.with(|p| p.get());
                super::with_app(|app| {
                    wants_paint(
                        policy,
                        dirty,
                        app.wants_draw(),
                        app.next_draw_deadline(),
                        web_time::Instant::now(),
                    )
                })
                .unwrap_or(dirty)
            })
        });
        if should {
            painted = paint(w, h);
            if painted {
                FIRST_PAINT.with(|g| g.mark_painted());
            }
        }
    }

    let pointer_idle = POINTERS.with(|p| p.borrow().is_idle());
    let before = POLICY.with(|p| p.get());
    let mut policy = before;
    with_app_host(|app, host| {
        let mut control = WebShellControl::new(&mut policy, painted, pointer_idle);
        host.on_idle(app, &mut control);
    });
    // A global `set_redraw_policy` made inside `on_idle` must survive.
    POLICY.with(|p| p.set(policy_after_idle(before, policy, p.get())));
}

fn report_geometry(width: u32, height: u32, scale_factor: f64, fullscreen: bool) {
    let geometry = CanvasGeometry {
        width,
        height,
        fullscreen,
        scale_factor,
    };
    HOST.with(|h| {
        let Ok(mut host) = h.try_borrow_mut() else {
            return;
        };
        let deliver = GEOMETRY.with(|g| g.borrow_mut().should_deliver(geometry, host.is_some()));
        if let (true, Some(host)) = (deliver, host.as_mut()) {
            mark_dirty();
            host.on_geometry_changed(geometry);
        }
    });
}

/// Stop painting for good and run the fatal path.
fn fail(err: WebShellError) {
    GPU.with(|g| *g.borrow_mut() = GpuState::Failed);
    report_fatal(&err);
}

/// Start a rebuild if the device was lost. Returns `false` while no painting
/// is possible this tick (a rebuild is in flight or was just started).
fn service_device_loss() -> bool {
    if lifecycle::take_webgl_context_lost() {
        // wgpu's GL backend cannot re-create its resources on a restored
        // context, so a lost WebGL2 context is reported, not rebuilt.
        fail(WebShellError::DeviceLost(
            "the WebGL2 context was lost; reload the page".to_string(),
        ));
        return false;
    }
    let lost = GPU.with(|g| match &*g.borrow() {
        GpuState::Ready(p) => Some(p.gpu.device_lost()),
        GpuState::Rebuilding | GpuState::Failed => None,
        GpuState::Pending => Some(false),
    });
    match lost {
        None => false,
        Some(false) => true,
        Some(true) => {
            if !BACKOFF.with(|b| b.borrow().may_attempt(web_time::Instant::now())) {
                return false;
            }
            let old = GPU.with(|g| std::mem::replace(&mut *g.borrow_mut(), GpuState::Rebuilding));
            let GpuState::Ready(painter) = old else {
                return false;
            };
            wasm_bindgen_futures::spawn_local(async move {
                match painter.gpu.rebuild().await {
                    Ok(gpu) => {
                        BACKOFF.with(|b| b.borrow_mut().record_success());
                        let info = gpu.info();
                        install_gpu(gpu);
                        FIRST_PAINT.with(|g| g.reset());
                        with_app_host(|app, host| host.on_gpu_rebuilt(app, &info));
                        mark_dirty();
                    }
                    Err((gpu, err)) => {
                        let verdict = BACKOFF
                            .with(|b| b.borrow_mut().record_failure(web_time::Instant::now()));
                        match verdict {
                            RebuildVerdict::RetryAfter(delay) => {
                                web_sys::console::error_1(&JsValue::from_str(&format!(
                                    "agg-gui-web-shell: GPU rebuild failed ({err}); \
                                     retrying in {delay:?}"
                                )));
                                // Keep the dead device installed; the lost
                                // flag is still set, so a tick after the
                                // backoff retries the rebuild.
                                install_gpu(gpu);
                            }
                            RebuildVerdict::GiveUp => {
                                fail(WebShellError::DeviceLost(err.to_string()))
                            }
                        }
                    }
                }
            });
            false
        }
    }
}

/// Paint one frame. Returns whether a frame was presented.
fn paint(w: u32, h: u32) -> bool {
    let offscreen = CONFIG.with(|c| c.borrow().as_ref().is_some_and(|c| c.offscreen_scene));
    GPU.with(|g| {
        let mut state = g.borrow_mut();
        let GpuState::Ready(p) = &mut *state else {
            return false;
        };
        let started = web_time::Instant::now();
        let (w, h) = p.gpu.resize(w, h);
        let surface_frame = match p.gpu.acquire() {
            Acquire::Frame(f) => f,
            Acquire::Retry => {
                mark_dirty();
                return false;
            }
            Acquire::Skip => return false,
        };
        let surface_view = surface_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let next_key = layout_key(w, h);
        p.frames += 1;
        let frame = Frame {
            width: w,
            height: h,
            device_scale: agg_gui::device_scale(),
            duration: p.last_duration,
            index: p.frames,
            needs_layout: p.layout_key != Some(next_key),
            input_since_last_frame: INPUT_SINCE_FRAME.with(|c| c.replace(false)),
        };
        DIRTY.with(|d| d.set(false));

        if offscreen {
            let device = Arc::clone(&p.gpu.device);
            match p.scene.as_mut() {
                Some(fb) => fb.ensure_size(&device, w, h),
                None => {
                    p.scene = Some(SsaaFramebuffer::new(
                        &device,
                        w,
                        h,
                        p.gpu.config.format,
                        false,
                    ))
                }
            }
        }
        let target_view = match &p.scene {
            Some(fb) if offscreen => {
                p.ctx.set_surface_texture(fb.resolve_texture().clone());
                fb.render_view().clone()
            }
            _ => {
                p.ctx.set_surface_texture(surface_frame.texture.clone());
                surface_view.clone()
            }
        };

        with_app_host(|app, host| {
            host.on_frame(app, &frame);
            p.ctx.begin_frame(target_view);
            host.paint(app, &mut p.ctx, &frame);
            p.ctx.end_frame();
            host.after_paint(&mut p.ctx, &frame);
        });
        p.layout_key = Some(next_key);

        if let (true, Some(fb)) = (offscreen, p.scene.as_ref()) {
            let mut encoder =
                p.gpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("agg-gui-web-shell scene blit"),
                    });
            fb.blit_to(
                &p.gpu.device,
                &mut encoder,
                &surface_view,
                (w, h),
                agg_gui::Rect::new(0.0, 0.0, w as f64, h as f64),
                None,
                p.ctx.pipelines(),
            );
            p.gpu.queue.submit(std::iter::once(encoder.finish()));
        }

        // Releases the stashed texture (surface or SSAA resolve) before
        // presenting — see `WgpuGfxCtx::present`.
        p.ctx.present(surface_frame);
        p.last_duration = started.elapsed();
        PRESENTED.with(|c| c.set(true));
        with_app_host(|app, host| host.after_present(app, &frame));
        true
    })
}
