//! The `requestAnimationFrame` loop: per-tick canvas sizing and DPR sync,
//! the paint decision ([`crate::policy`]), one painted frame, and the
//! device-loss rebuild.
//!
//! Owns the GPU-side state ([`Painter`]) in its own thread-local; the app and
//! host live in [`super`]'s cells. Order per tick mirrors the native shell's
//! loop: size → sensors/fullscreen → `on_tick` → decide → (`on_frame` →
//! `paint` → `end_frame` → `after_paint` → present) → `on_idle`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use agg_gui::App;
use agg_gui_wgpu::{SsaaFramebuffer, WgpuGfxCtx};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use super::gpu::{Acquire, WebGpu};
use super::{mark_dirty, INPUT_SINCE_FRAME, POLICY};
use super::{platform, sensors, APP, BUTTONS_HELD, CANVAS, CONFIG, DIRTY, FIRST_PAINT, HOST};
use crate::dom_math::{backing_size, sanitize_dpr};
use crate::host::{CanvasGeometry, Frame, WebShellControl, WebShellHost};
use crate::policy::{layout_key, wants_paint, LayoutKey};

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
}

thread_local! {
    static GPU: RefCell<GpuState> = const { RefCell::new(GpuState::Pending) };
    static GEOMETRY: RefCell<Option<CanvasGeometry>> = const { RefCell::new(None) };
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

/// Match the canvas backing store to `clientSize × DPR`. Returns the size and
/// whether it changed.
pub(super) fn size_backing_store(canvas: &web_sys::HtmlCanvasElement) -> ((u32, u32), bool) {
    let dpr = current_dpr();
    let (w, h) = backing_size(
        canvas.client_width() as f64,
        canvas.client_height() as f64,
        dpr,
    );
    let changed = canvas.width() != w || canvas.height() != h;
    if changed {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    ((w, h), changed)
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
    let dpr = current_dpr();
    if (agg_gui::device_scale() - dpr).abs() > 1e-9 {
        agg_gui::set_device_scale(dpr);
        mark_dirty();
    }
    let ((w, h), resized) = size_backing_store(&canvas);

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

    let pointer_idle = BUTTONS_HELD.with(|b| b.get()) == 0;
    let mut policy = POLICY.with(|p| p.get());
    with_app_host(|app, host| {
        let mut control = WebShellControl::new(&mut policy, painted, pointer_idle);
        host.on_idle(app, &mut control);
    });
    POLICY.with(|p| p.set(policy));
}

fn report_geometry(width: u32, height: u32, scale_factor: f64, fullscreen: bool) {
    let geometry = CanvasGeometry {
        width,
        height,
        fullscreen,
        scale_factor,
    };
    let changed = GEOMETRY.with(|g| {
        let mut g = g.borrow_mut();
        let changed = *g != Some(geometry);
        *g = Some(geometry);
        changed
    });
    if changed {
        mark_dirty();
        HOST.with(|h| {
            if let Ok(mut host) = h.try_borrow_mut() {
                if let Some(host) = host.as_mut() {
                    host.on_geometry_changed(geometry);
                }
            }
        });
    }
}

/// Start a rebuild if the device was lost. Returns `false` while no painting
/// is possible this tick (a rebuild is in flight or was just started).
fn service_device_loss() -> bool {
    let lost = GPU.with(|g| match &*g.borrow() {
        GpuState::Ready(p) => Some(p.gpu.device_lost()),
        GpuState::Rebuilding => None,
        GpuState::Pending => Some(false),
    });
    match lost {
        None => false,
        Some(false) => true,
        Some(true) => {
            let old = GPU.with(|g| std::mem::replace(&mut *g.borrow_mut(), GpuState::Rebuilding));
            let GpuState::Ready(painter) = old else {
                return false;
            };
            wasm_bindgen_futures::spawn_local(async move {
                match painter.gpu.rebuild().await {
                    Ok(gpu) => {
                        let info = gpu.info();
                        install_gpu(gpu);
                        FIRST_PAINT.with(|g| g.reset());
                        with_app_host(|app, host| host.on_gpu_rebuilt(app, &info));
                        mark_dirty();
                    }
                    Err((gpu, err)) => {
                        web_sys::console::error_1(&JsValue::from_str(&format!(
                            "agg-gui-web-shell: GPU rebuild failed: {err}"
                        )));
                        // Keep the dead device installed; the lost flag is
                        // still set, so the next tick retries the rebuild.
                        install_gpu(gpu);
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

        surface_frame.present();
        p.last_duration = started.elapsed();
        true
    })
}
