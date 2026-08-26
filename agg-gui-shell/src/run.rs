//! Startup: restore the window bounds, open the window, bring up wgpu, build
//! the app, paint the first frame into the still-hidden window, and hand
//! control to [`crate::shell_loop::ShellLoop`].
//!
//! The ordering here is deliberate and load-bearing; see [`run`].

#![allow(deprecated)] // winit 0.30 `EventLoop::run` idiom

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use agg_gui::{App, Modifiers};
use agg_gui_wgpu::{CopySrc, Gpu, GpuConfig};
use winit::event_loop::EventLoop;
use winit::window::{Icon, Window, WindowAttributes};

use crate::bounds::{
    sanitize_restored_window_size, BoundsAutoSave, SavedBounds, WindowedSizeTracker,
};
use crate::config::{ShellConfig, WindowIcon, WindowSize};
use crate::host::{ExitAction, ShellHost};
use crate::paint::{PaintRequest, Painter};
use crate::shell_loop::{ShellInit, ShellLoop};
use crate::waker::HostWakerGuard;
use crate::ShellError;

/// Open a window, bring up wgpu, build the app, and run the event loop until
/// the window closes.
///
/// `build` is called once the window and the GPU exist — the device scale,
/// input profile and surface size are already in place, which is what a widget
/// tree wants to see while it is being built. It returns the [`App`] and the
/// [`ShellHost`] that carries the app's own per-frame plumbing; a host with
/// nothing to add is [`crate::NoHost`].
///
/// ```no_run
/// # fn demo(app: agg_gui::App) -> Result<(), agg_gui_shell::ShellError> {
/// use agg_gui_shell::{run, NoHost, ShellConfig};
///
/// run(ShellConfig::new("My App"), move |_init| Ok((app, NoHost)))
/// # }
/// ```
///
/// # Ordering
///
/// 1. Restore and sanitise the saved window size, then refine it against the
///    real monitor once the window exists (winit 0.30 only exposes monitors
///    through a live `Window`).
/// 2. Create the window **hidden**, finish wgpu setup, build the app, and
///    paint the first real frame before showing it — otherwise the user sees
///    an unstyled OS-default background and a black border around the
///    not-yet-configured surface.
/// 3. Install the agg-gui host waker only after the window and GPU came up,
///    and clear it on every exit path (`HostWakerGuard`).
pub fn run<H, B>(config: ShellConfig, build: B) -> Result<(), ShellError>
where
    H: ShellHost + 'static,
    B: FnOnce(&ShellInit<'_>) -> Result<(App, H), ShellError>,
{
    let event_loop = EventLoop::new().map_err(ShellError::EventLoop)?;

    let restored_bounds = config.bounds_store.as_ref().and_then(|s| s.load());
    let (window, restored_size) = create_window(&event_loop, &config, restored_bounds)?;
    agg_gui::set_device_scale(window.scale_factor());
    if config.os_tooltip_timings {
        if let Some(timings) = crate::tooltip::os_tooltip_timings() {
            agg_gui::set_tooltip_timings(timings);
        }
    }

    // Configure at the window's REAL inner size rather than the size that was
    // requested: `request_inner_size` may be applied asynchronously, and the
    // `Resized` event that follows drives the surface reconfigure anyway.
    let init_size = window.inner_size();
    let gpu_config = GpuConfig::new(config.device_label)
        .with_present_mode(config.present_mode)
        .with_optional_features(config.optional_features)
        // A capture run that silently produced nothing would be worse than a
        // hard failure, so a configured screenshot *requires* read-back.
        .with_copy_src(if config.screenshot.is_some() {
            CopySrc::Required
        } else {
            config.copy_src
        });
    let gpu = Gpu::new(
        Arc::clone(&window),
        (init_size.width.max(1), init_size.height.max(1)),
        gpu_config,
    )
    .map_err(ShellError::Gpu)?;

    let (app, host) = build(&ShellInit {
        window: &window,
        gpu: &gpu,
        restored_bounds,
    })?;

    // Only now: window up, GPU up, app built. Cleared on every exit path by
    // the guard, which the loop owns from here on.
    let waker_guard = HostWakerGuard::install(event_loop.create_proxy());

    let painter = Painter::new(&gpu);
    let maximized = window.is_maximized();
    // Seeded from the restored size, NOT from the live surface: a window
    // created maximized reports the work-area rect, which must never become
    // the persisted windowed size. See `WindowedSizeTracker::new`.
    let windowed_size =
        WindowedSizeTracker::new(restored_size, (gpu.config().width, gpu.config().height));
    let mut bounds_auto = BoundsAutoSave::default();
    bounds_auto.seed(restored_bounds);

    let exit = Rc::new(Cell::new(None::<ExitAction>));
    let error = Rc::new(RefCell::new(None::<ShellError>));

    let mut shell = ShellLoop {
        window: Arc::clone(&window),
        gpu: Some(gpu),
        gpu_config,
        painter,
        app,
        host,
        policy: config.redraw_policy,
        screenshot: config.screenshot.clone(),
        screenshot_done: false,
        screenshot_last_paint: Instant::now(),
        cursor: (0.0, 0.0),
        mods: Modifiers::default(),
        mouse_buttons_down: 0,
        input_since_frame: false,
        pending_resize: None,
        bounds_store: config.bounds_store,
        bounds_auto,
        windowed_size,
        maximized,
        exit: Rc::clone(&exit),
        error: Rc::clone(&error),
        finished: false,
        _waker_guard: waker_guard,
    };

    // First frame into the hidden window: after this the surface holds the
    // fully-styled first frame, so `set_visible(true)` never flashes an
    // OS-default background. The screenshot path is deliberately not armed
    // here — a capture counts settle frames from the visible window.
    if let Some(gpu) = shell.gpu.as_mut() {
        shell.painter.paint(
            gpu,
            &window,
            &mut shell.app,
            &mut shell.host,
            PaintRequest {
                pending_resize: &mut None,
                input_since_last_frame: false,
                capture: None,
            },
        )?;
    }
    window.set_visible(true);

    event_loop
        .run(move |event, elwt| shell.handle(event, elwt))
        .map_err(ShellError::EventLoop)?;

    if let Some(e) = error.borrow_mut().take() {
        return Err(e);
    }
    if exit.get() == Some(ExitAction::Relaunch) {
        relaunch()?;
    }
    Ok(())
}

/// Start a fresh copy of this executable. Called after the loop has exited and
/// every store has been flushed, so the child reads the state this run wrote.
fn relaunch() -> Result<(), ShellError> {
    let exe = std::env::current_exe().map_err(ShellError::Relaunch)?;
    std::process::Command::new(exe)
        .spawn()
        .map(|_| ())
        .map_err(ShellError::Relaunch)
}

/// Create the window, returning it alongside the sanitised restored size that
/// was applied (`None` when nothing was restored). The caller needs that size
/// to seed [`WindowedSizeTracker`]: it is the last *windowed* size, which the
/// live surface does not report when the window came up maximized.
fn create_window(
    event_loop: &EventLoop<()>,
    config: &ShellConfig,
    restored: Option<SavedBounds>,
) -> Result<(Arc<Window>, Option<(u32, u32)>), ShellError> {
    // A restored size is physical px and may be corrupt (an old DPI-ratchet
    // bug, a zeroed file), so it is sanitised before winit ever sees it. This
    // first pass has no monitor information: winit 0.30 exposes monitors on
    // the `Window`, not the pre-run `EventLoop`, so the real display size is
    // not available until the window exists. The fallback ceiling already
    // floors tiny values and caps an over-large size below the GPU max; the
    // refinement against the actual monitor happens right after creation.
    let saved = restored.map(|b| (b.width, b.height));
    let mut restored_size = saved.map(|_| sanitize_restored_window_size(saved, None));
    let size = match restored_size {
        Some((w, h)) => WindowSize::Physical(w, h),
        None => config.size,
    };

    let mut attributes = WindowAttributes::default()
        .with_title(config.title.clone())
        .with_inner_size(size.to_winit())
        .with_maximized(restored.map(|b| b.maximized).unwrap_or(config.maximized))
        // Shown after the first frame is painted, to avoid a white flash.
        .with_visible(false);
    if let Some(min) = config.min_size {
        attributes = attributes.with_min_inner_size(min.to_winit());
    }
    if let Some(icon) = config.icon.as_ref() {
        attributes = attributes.with_window_icon(window_icon(icon));
    }
    if config.fullscreen {
        attributes = attributes.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        agg_gui::fullscreen::set_active(true);
    }

    let window = Arc::new(
        event_loop
            .create_window(attributes)
            .map_err(ShellError::CreateWindow)?,
    );

    // Refine a restored size against the window's real monitor now that it
    // exists: this shrinks a size larger than the display back onto it before
    // the window is shown. The window is still hidden, so the resize is
    // invisible.
    if saved.is_some() {
        if let Some(monitor) = window
            .primary_monitor()
            .or_else(|| window.current_monitor())
        {
            let m = monitor.size();
            let refined = sanitize_restored_window_size(saved, Some((m.width, m.height)));
            restored_size = Some(refined);
            if WindowSize::Physical(refined.0, refined.1) != size {
                let _ =
                    window.request_inner_size(winit::dpi::PhysicalSize::new(refined.0, refined.1));
            }
        }
    }
    Ok((window, restored_size))
}

/// Build a winit icon, logging and dropping one the platform rejects — a bad
/// icon must not stop the app from opening.
fn window_icon(icon: &WindowIcon) -> Option<Icon> {
    match Icon::from_rgba(icon.rgba.clone(), icon.width, icon.height) {
        Ok(icon) => Some(icon),
        Err(err) => {
            log::warn!("agg-gui-shell: ignoring invalid window icon: {err}");
            None
        }
    }
}
