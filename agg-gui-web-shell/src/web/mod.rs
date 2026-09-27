//! The browser runtime (wasm32 only): shell state, [`start`], and the small
//! public API app exports call ([`mark_dirty`], [`with_app`], …).
//!
//! Submodules:
//! - [`gpu`] — async WebGPU/WebGL2 init on the canvas, surface acquire,
//!   device-loss rebuild;
//! - [`frame`] — the `requestAnimationFrame` loop: sizing, the paint
//!   decision, and one painted frame;
//! - [`input`] — canvas pointer / wheel / context-menu listeners;
//! - [`lifecycle`] — window-level pointer-release resync, page-hide flush;
//! - [`platform`] — client-platform detection, fullscreen, fatal panel;
//! - [`sensors`] — tilt + gamepad (moved from `demo-wgpu`).
//!
//! State lives in separate thread-locals (the browser main thread owns the
//! whole shell) so a host callback that receives `&mut App` can still call
//! [`mark_dirty`] / [`set_redraw_policy`] without a re-entrant borrow. Host
//! callbacks must not call [`with_app`] — the app is already borrowed and the
//! call is skipped.

use std::cell::{Cell, RefCell};

use agg_gui::App;
use wasm_bindgen::JsValue;

use crate::config::{RedrawPolicy, WebShellConfig};
use crate::error::{GpuInfo, WebShellError};
use crate::host::WebShellHost;
use crate::policy::FirstPaintGate;

mod frame;
mod gpu;
mod input;
mod lifecycle;
mod platform;
mod sensors;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static HOST: RefCell<Option<Box<dyn WebShellHost>>> = const { RefCell::new(None) };
    static CANVAS: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    static CONFIG: RefCell<Option<WebShellConfig>> = const { RefCell::new(None) };
    static POLICY: Cell<RedrawPolicy> = const { Cell::new(RedrawPolicy::Reactive) };
    /// Shell-level repaint request: input arrived or an app export changed
    /// state. Cleared when a frame is painted.
    static DIRTY: Cell<bool> = const { Cell::new(true) };
    /// Any input since the last painted frame (for [`crate::Frame`]).
    static INPUT_SINCE_FRAME: Cell<bool> = const { Cell::new(false) };
    /// Mouse buttons held, per the DOM `buttons` bitmask — see
    /// [`lifecycle`] for the three paths that keep it honest.
    static BUTTONS_HELD: Cell<u32> = const { Cell::new(0) };
    static FIRST_PAINT: FirstPaintGate = const { FirstPaintGate::new() };
}

/// Request a repaint on the next animation frame. App exports call this after
/// mutating state the widget tree reads.
pub fn mark_dirty() {
    DIRTY.with(|c| c.set(true));
}

pub(crate) fn note_input() {
    INPUT_SINCE_FRAME.with(|c| c.set(true));
    mark_dirty();
}

/// Run `f` with the shell-owned [`App`], if it is built and not already
/// borrowed. Returns `None` when skipped (app not built yet — GPU init is
/// async — or called re-entrantly from a host callback / after a panic that
/// left the borrow poisoned).
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut borrow = cell.try_borrow_mut().ok()?;
        borrow.as_mut().map(f)
    })
}

/// Run `f` with the shell's canvas element.
pub fn with_canvas<R>(f: impl FnOnce(&web_sys::HtmlCanvasElement) -> R) -> Option<R> {
    CANVAS.with(|c| c.borrow().as_ref().map(f))
}

/// Current redraw policy.
pub fn redraw_policy() -> RedrawPolicy {
    POLICY.with(|p| p.get())
}

/// Switch between reactive and continuous redraw — e.g. from an app's
/// Performance-window run-mode selector.
pub fn set_redraw_policy(policy: RedrawPolicy) {
    POLICY.with(|p| p.set(policy));
    mark_dirty();
}

/// What the app builder closure gets: the canvas and the live GPU.
pub struct WebShellInit<'a> {
    canvas: &'a web_sys::HtmlCanvasElement,
    gpu: &'a GpuInfo,
    size: (u32, u32),
}

impl WebShellInit<'_> {
    pub fn canvas(&self) -> &web_sys::HtmlCanvasElement {
        self.canvas
    }

    /// The device + queue an app with its own wgpu renderer builds from.
    pub fn gpu(&self) -> &GpuInfo {
        self.gpu
    }

    /// Canvas backing-store size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// `agg_gui::device_scale()` at build time.
    pub fn device_scale(&self) -> f64 {
        agg_gui::device_scale()
    }
}

type Builder =
    Box<dyn FnOnce(&WebShellInit<'_>) -> Result<(App, Box<dyn WebShellHost>), WebShellError>>;

/// Boot the shell. Call once from `#[wasm_bindgen(start)]`.
///
/// Synchronously: installs the panic hook, detects the client platform (so the
/// widget tree is built against the right input profile / UX scale), sizes the
/// canvas, installs input + lifecycle listeners, and starts the
/// `requestAnimationFrame` loop. Asynchronously: brings up the GPU, then runs
/// `build` — like the native shell, the app is built *after* the GPU exists so
/// custom renderers can use the device. Input before that is dropped.
///
/// Returns an error only for failures detectable synchronously (no canvas).
/// An async failure (no WebGPU, device refused, builder error) is logged to the
/// console and, with [`WebShellConfig::fatal_panel`], shown in place of the
/// canvas.
pub fn start<H, F>(config: WebShellConfig, build: F) -> Result<(), WebShellError>
where
    H: WebShellHost + 'static,
    F: FnOnce(&WebShellInit<'_>) -> Result<(App, H), WebShellError> + 'static,
{
    if config.panic_hook {
        console_error_panic_hook::set_once();
    }
    let canvas = platform::find_canvas(&config.canvas_id)?;
    if config.detect_platform {
        platform::apply_client_platform();
    }
    agg_gui::set_device_scale(frame::current_dpr());
    frame::size_backing_store(&canvas);
    POLICY.with(|p| p.set(config.redraw_policy));
    CANVAS.with(|c| *c.borrow_mut() = Some(canvas.clone()));
    CONFIG.with(|c| *c.borrow_mut() = Some(config.clone()));

    input::install_keyboard();
    input::install_pointer_listeners(&canvas);
    lifecycle::install_window_pointer_release();
    lifecycle::install_page_hide();
    lifecycle::install_resize();

    let builder: Builder = Box::new(move |init| {
        build(init).map(|(app, host)| (app, Box::new(host) as Box<dyn WebShellHost>))
    });
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(err) = boot(canvas, config, builder).await {
            report_fatal(&err);
        }
    });

    frame::start_raf_loop();
    Ok(())
}

async fn boot(
    canvas: web_sys::HtmlCanvasElement,
    config: WebShellConfig,
    build: Builder,
) -> Result<(), WebShellError> {
    let gpu = gpu::WebGpu::init(&canvas, &config).await?;
    let info = gpu.info();
    let size = (gpu.config.width, gpu.config.height);
    let init = WebShellInit {
        canvas: &canvas,
        gpu: &info,
        size,
    };
    let (app, host) = build(&init)?;
    frame::install_gpu(gpu);
    APP.with(|c| *c.borrow_mut() = Some(app));
    HOST.with(|c| *c.borrow_mut() = Some(host));
    // The web equivalent of winit's initial `RedrawRequested`; the
    // FirstPaintGate is the belt to this brace.
    agg_gui::animation::request_draw();
    mark_dirty();
    Ok(())
}

fn report_fatal(err: &WebShellError) {
    let msg = err.to_string();
    web_sys::console::error_1(&JsValue::from_str(&format!("agg-gui-web-shell: {msg}")));
    let panel = CONFIG.with(|c| c.borrow().as_ref().map(|c| c.fatal_panel).unwrap_or(true));
    if panel {
        platform::show_fatal_panel(&msg);
    }
}
