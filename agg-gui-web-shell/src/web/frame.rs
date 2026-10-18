//! One tick of the frame loop: canvas sizing and DPR sync, the paint
//! decision ([`crate::policy`], held back while the GPU is busy by
//! [`crate::frames_in_flight`]), one painted frame, the device-loss rebuild,
//! and the end-of-tick decision to run on or sleep ([`decide`], over
//! [`crate::frame_schedule`]); [`super::schedule`] runs the ticks.
//!
//! Owns the GPU-side state ([`Painter`]) in its own thread-local; the app and
//! host live in [`super`]'s cells. Order per tick mirrors the native shell's
//! loop: size → sensors/fullscreen → `on_tick` → drain `agg_gui::ui_thread`
//! → decide → (`on_frame` → `paint` → `end_frame` → `after_paint` → present →
//! `after_present`) → `on_idle` → run on or sleep. Every decision is delegated
//! to a platform-neutral module (`dom_math::fit_backing`, `policy`,
//! `frames_in_flight`, `recovery`, `frame_schedule`) that is unit tested.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use agg_gui::App;
use agg_gui_wgpu::{SsaaFramebuffer, WgpuGfxCtx};
use wasm_bindgen::JsValue;

use super::gpu::{Acquire, WebGpu};
use super::{lifecycle, mark_dirty, report_fatal, INPUT_SINCE_FRAME, POINTERS, POLICY, PRESENTED};
use super::{platform, sensors, APP, CANVAS, CONFIG, CURSOR, DIRTY, FIRST_PAINT, HOST};
use crate::config::RedrawPolicy;
use crate::dom_math::{fit_backing, sanitize_dpr};
use crate::error::WebShellError;
use crate::frame_schedule::{next_tick, LoopDemand, NextTick};
use crate::frames_in_flight::{
    tick_paint, Expired, FramesInFlight, TickPaint, MAX_FRAMES_IN_FLIGHT,
};
use crate::host::{CanvasGeometry, Frame, WebShellControl, WebShellHost};
use crate::policy::{policy_after_idle, wants_paint, GeometryTracker};
use crate::recovery::{RebuildBackoff, RebuildVerdict};
use agg_gui::frame_policy::{LayoutKey, LayoutTracker};

/// GPU-side per-canvas state.
struct Painter {
    gpu: WebGpu,
    ctx: WgpuGfxCtx,
    /// Offscreen scene target when `WebShellConfig::offscreen_scene` is on.
    scene: Option<SsaaFramebuffer>,
    /// Layout-skip memory (`agg_gui::frame_policy`, shared with the native
    /// shell and headless drivers).
    layout: LayoutTracker,
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
    /// Presented frames the GPU has not finished; reset whenever a device is
    /// installed, so a lost device's unfinished frames never block painting.
    static IN_FLIGHT: RefCell<FramesInFlight> =
        const { RefCell::new(FramesInFlight::new(MAX_FRAMES_IN_FLIGHT)) };
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
    .with_adapter(&gpu.adapter)
    // Custom render passes see the device's rejected frames as
    // `WgpuCustomRenderCtx::lost_frames`.
    .with_uncaptured_errors(&gpu.errors)
    // Custom render passes key their GPU state on the device's generation,
    // which a rebuilt device never shares with the lost one.
    .with_device_generation(gpu.device_generation)
}

pub(super) fn install_gpu(gpu: WebGpu) {
    MAX_DIM.with(|m| m.set(gpu.device.limits().max_texture_dimension_2d));
    // Only the browser's WebGPU reports completions itself, and can report
    // them late (`frames_in_flight`); wgpu's WebGL2 backend runs them from the
    // shell's own polling.
    let browser_webgpu = gpu.adapter.get_info().backend == wgpu::Backend::BrowserWebGpu;
    IN_FLIGHT.with(|f| {
        let mut f = f.borrow_mut();
        f.reset();
        f.enable_report_probes(browser_webgpu);
    });
    let ctx = new_ctx(&gpu);
    GPU.with(|g| {
        *g.borrow_mut() = GpuState::Ready(Box::new(Painter {
            gpu,
            ctx,
            scene: None,
            layout: LayoutTracker::new(),
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

/// Start the frame loop; it sleeps whenever [`decide`] finds nothing to do.
pub(super) fn start_frame_loop() {
    super::schedule::install(|| {
        tick();
        decide()
    });
}

/// Run `f` with the app and host, skipping (`None`) when either is missing
/// or borrowed.
fn with_app_host<R>(f: impl FnOnce(&mut App, &mut dyn WebShellHost) -> R) -> Option<R> {
    APP.with(|a| {
        HOST.with(|h| {
            let (Ok(mut app), Ok(mut host)) = (a.try_borrow_mut(), h.try_borrow_mut()) else {
                return None;
            };
            match (app.as_mut(), host.as_mut()) {
                (Some(app), Some(host)) => Some(f(app, host.as_mut())),
                _ => None,
            }
        })
    })
}

/// After a tick: whether anything needs the next frame, a timer, or nothing.
/// Reads the state itself rather than the tick's results, so it holds after
/// a tick that returned early, and after work `on_idle` did.
fn decide() -> NextTick {
    let now = web_time::Instant::now();
    let (can_paint, rebuild_at) = GPU.with(|g| match &*g.borrow() {
        // A lost device is rebuilt by a tick once the backoff allows.
        GpuState::Ready(p) if p.gpu.device_lost() => (
            false,
            Some(BACKOFF.with(|b| b.borrow().retry_at()).unwrap_or(now)),
        ),
        GpuState::Ready(_) => (true, None),
        GpuState::Pending | GpuState::Rebuilding | GpuState::Failed => (false, None),
    });
    let (app_wants_draw, next_draw_deadline) = super::with_app(|app| {
        // `wants_draw` also promotes a deadline that has come due.
        (app.wants_draw(), app.next_draw_deadline())
    })
    .unwrap_or((false, None));
    let host_wants_tick = with_app_host(|app, host| host.wants_tick(app)).unwrap_or(false);
    let demand = LoopDemand {
        can_paint,
        continuous: POLICY.with(|p| p.get()) == RedrawPolicy::Continuous,
        first_paint_pending: !FIRST_PAINT.with(|g| g.has_painted()),
        dirty: DIRTY.with(|d| d.get()),
        app_wants_draw,
        next_draw_deadline,
        idle_work_at: agg_gui::ui_thread::next_due(),
        pointer_held: !POINTERS.with(|p| p.borrow().is_idle()),
        gamepad_connected: agg_gui::gamepad::state().is_some(),
        input_held: super::input_held(),
        platform_request_pending: agg_gui::tilt::enable_requested()
            || agg_gui::fullscreen::requested(),
        tick_requested: super::schedule::take_tick_request() || host_wants_tick,
        rebuild_at,
        now,
    };
    next_tick(&demand)
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

    // Input held while the app was borrowed reaches it before the tick's
    // own work, so a press and its release stay in order with everything else.
    super::deliver_held_input();
    with_app_host(|app, host| host.on_tick(app));
    // Queued UI work (`agg_gui::ui_thread`) before the paint decision, so
    // what it changes shows this tick; it runs even while the GPU is not
    // ready to paint.
    agg_gui::ui_thread::invoke_pending_actions();
    // A cursor changed by code (`agg_gui::invalidate_cursor`) reaches the
    // canvas while the mouse rests and painting is idle, as agg-gui-shell
    // does on every paint.
    let icon = super::with_app(|app| CURSOR.with(|c| c.borrow_mut().after_tick(app))).flatten();
    super::write_cursor(&canvas, icon);

    if !service_device_loss() {
        return;
    }

    let ready = GPU.with(|g| matches!(*g.borrow(), GpuState::Ready(_)));
    let mut painted = false;
    if ready {
        expire_stalled_frames();
        let decision = IN_FLIGHT.with(|f| {
            tick_paint(&f.borrow(), || {
                FIRST_PAINT.with(|gate| {
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
                })
            })
        });
        if decision == TickPaint::WaitForGpu {
            wait_for_gpu(resized);
        }
        if decision == TickPaint::Paint {
            painted = paint(w, h);
            // The paint re-resolved hover (`App::paint`), which can change it.
            super::sync_cursor(&canvas);
            if painted {
                FIRST_PAINT.with(|g| g.mark_painted());
            }
        }
    }

    // The browser's input method follows focus and the caret.
    super::ime::sync();

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
                                // backoff retries the rebuild. The loop slept
                                // while the rebuild ran: the tick this wakes
                                // arms the backoff timer.
                                install_gpu(gpu);
                                super::schedule::wake();
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

/// A tick with a redraw pending while the GPU still holds the frames allowed
/// in flight: paint nothing. Every pending request stays as it is (the
/// decision did not read it), so the first tick after the GPU finishes
/// paints the latest state.
fn wait_for_gpu(resized: bool) {
    // A resize clears the canvas, and its flag lives only for this tick.
    if resized {
        mark_dirty();
    }
    GPU.with(|g| {
        if let GpuState::Ready(p) = &*g.borrow() {
            // WebGPU resolves completions on its own; wgpu's WebGL2 backend
            // runs them only when the device is polled or a submit happens,
            // and a waiting loop submits nothing.
            let _ = p.gpu.device.poll(wgpu::PollType::Poll);
        }
    });
}

/// Count a presented frame until the GPU has finished every submission so
/// far (the completion attaches to the queue's latest submission, which is
/// this frame's last).
fn track_in_flight(queue: &wgpu::Queue) {
    let ticket = IN_FLIGHT.with(|f| f.borrow_mut().submitted(web_time::Instant::now()));
    queue.on_submitted_work_done(move || {
        let (recovered, ask) = IN_FLIGHT.with(|f| {
            let mut f = f.borrow_mut();
            let recovered = f.finished(ticket, web_time::Instant::now());
            (recovered, f.wants_report_probe())
        });
        if recovered {
            web_sys::console::info_1(&JsValue::from_str(
                "agg-gui-web-shell: the GPU is finishing frames again",
            ));
        }
        if ask {
            send_report_probe();
        }
    });
}

/// Asks the browser how late it reports finished work
/// ([`FramesInFlight::wants_report_probe`]): an empty `onSubmittedWorkDone`,
/// with nothing on the GPU, sent straight from the completion that raised the
/// question (or the answer that needs confirming), so the time it takes is
/// the browser's own. No submit: a page counting submissions sees none.
fn send_report_probe() {
    GPU.with(|g| {
        let GpuState::Ready(p) = &*g.borrow() else {
            return;
        };
        let ticket = IN_FLIGHT.with(|f| f.borrow_mut().report_probe_sent(web_time::Instant::now()));
        p.gpu.queue.on_submitted_work_done(move || {
            let again = IN_FLIGHT.with(|f| {
                let mut f = f.borrow_mut();
                f.report_probe_answered(ticket, web_time::Instant::now());
                f.wants_report_probe()
            });
            if again {
                send_report_probe();
            }
        });
    });
}

/// Stops waiting for frames whose completion is long overdue
/// ([`FramesInFlight::expire`]), so a completion that never arrives cannot
/// stop painting for good. Warns once per stall, however long it lasts.
fn expire_stalled_frames() {
    let expired = IN_FLIGHT.with(|f| f.borrow_mut().expire(web_time::Instant::now()));
    if let Expired::StallStarted(waited) = expired {
        web_sys::console::warn_1(&JsValue::from_str(&format!(
            "agg-gui-web-shell: the GPU did not report finishing a frame within {waited:?}; \
             painting on without waiting for it"
        )));
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

        let next_key = LayoutKey::for_surface(w, h);
        p.frames += 1;
        let frame = Frame {
            width: w,
            height: h,
            device_scale: agg_gui::device_scale(),
            duration: p.last_duration,
            index: p.frames,
            needs_layout: p.layout.needs_layout(next_key),
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
        p.layout.record(next_key);

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
        track_in_flight(&p.gpu.queue);
        p.last_duration = started.elapsed();
        PRESENTED.with(|c| c.set(true));
        with_app_host(|app, host| host.after_present(app, &frame));
        true
    })
}
