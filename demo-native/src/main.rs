//! Native demo for agg-gui — a thin shim over `agg-gui-shell`.
//!
//! # Platform-split policy (kept identical across `demo-native`, `demo-wasm`)
//!
//! This crate is a **platform shim only**. It contains **no demo content**:
//! every widget tree, layout, and GPU renderer the user sees is shared via
//! `demo-wgpu` (the wgpu rendering library) and `demo-ui` (the widget tree +
//! layout), and the window / event loop / present are `agg-gui-shell`'s.
//!
//! - **Widget / layout code** → `demo-ui`
//! - **GPU renderers (WGSL shaders, geometry, draw calls)** → `demo-wgpu`
//!   (e.g. `WgpuCubeWidget`, the 3-D Animation widget)
//! - **Window, event loop, input, present, window-bounds persistence** →
//!   `agg-gui-shell`
//! - **What is left here**: font assets from disk, the Screen Share demo's
//!   tokio/WebRTC wiring, the saved-state file, and the per-frame plumbing in
//!   [`host::DemoHost`] that the shell's hooks thread back in.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::App;
use agg_gui_shell::{CopySrc, SavedBounds, ShellConfig, ShellError};
use demo_wgpu::WgpuCubeWidget;

mod host;
mod screen_share;
mod state_file;

use host::{DemoHost, StateFileBounds};
use state_file::{load_saved_state, state_file_path};

const DEBUG_LOG_FILE_NAME: &str = ".agg-gui-draw-debug.log";

const APP_ICON_SIZE: u32 = 256;
const APP_ICON_RGBA: &[u8] = include_bytes!("../assets/app-icon-256.rgba");

/// Path of the draw-diagnostic log, kept next to the state file so a captured
/// report survives the console scrolling away (or the app closing).
fn debug_log_path() -> std::path::PathBuf {
    state_file_path().with_file_name(DEBUG_LOG_FILE_NAME)
}

/// Build the draw report for the live tree, print it to stderr, and append it
/// (with a wall-clock timestamp) to [`debug_log_path`]. `label` distinguishes a
/// manual Ctrl+Shift+D capture from an auto-detected runaway. This is the
/// one-keypress evidence path for the intermittent "reactive host never
/// quiesces" bug.
fn emit_draw_report(app: &App, label: &str) {
    let report = agg_gui::debug_draw_report(app.root());
    eprintln!("\n[agg-gui draw report — {label}]\n{report}");
    let path = debug_log_path();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    use std::io::Write as _;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "==== {label} @ unix {stamp}s ====\n{report}");
            eprintln!("[agg-gui] draw report appended to {}", path.display());
        }
        Err(err) => {
            eprintln!(
                "[agg-gui] failed to write draw report to {}: {err}",
                path.display()
            );
        }
    }
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

fn main() -> Result<(), ShellError> {
    // Shared multi-thread runtime for the LAN phone server + WebRTC signaling.
    let screen_share_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let default_font_asset = demo_ui::font_asset_by_name(demo_ui::DEFAULT_FONT_NAME)
        .expect("default demo font asset is registered");
    install_demo_font_asset(default_font_asset.name, default_font_asset.path);
    let font = demo_ui::load_font_by_name(demo_ui::DEFAULT_FONT_NAME)
        .expect("default demo font asset should load at startup");

    // The window size lives in the same file as the rest of the session state,
    // so the shell's bounds store only records it; this cell is the handoff to
    // the demo's own save path.
    let bounds = Rc::new(Cell::new(None::<SavedBounds>));
    let config = ShellConfig::new("agg-gui — Demo (wgpu)")
        .with_icon_rgba(APP_ICON_RGBA, APP_ICON_SIZE, APP_ICON_SIZE)
        // `COPY_SRC` is what the Take-Screenshot button's read-back needs;
        // `IfSupported` because a surface that can't offer it should cost us
        // the screenshot feature, not the whole app.
        .with_copy_src(CopySrc::IfSupported)
        .with_device_label("demo-native-wgpu")
        .with_bounds_store(StateFileBounds {
            latest: Rc::clone(&bounds),
        });

    agg_gui_shell::run(config, |init| {
        // Saved state is read here rather than before `run`: the widget tree is
        // built from it, and the shell has already restored the window size
        // from the same file through `StateFileBounds`.
        let initial_state = load_saved_state();

        // Relaunch flag — set by the Render tab's Relaunch button via the
        // closure handed to `PlatformHooks::native`, and turned into a shell
        // relaunch request on the idle tick. Keeping it here means demo-ui
        // never imports `std::process`.
        let relaunch_requested = Rc::new(Cell::new(false));
        let running_msaa: u8 = initial_state.as_ref().map(|s| s.msaa_samples).unwrap_or(0);
        let platform = {
            let flag = Rc::clone(&relaunch_requested);
            demo_ui::PlatformHooks::native(running_msaa, move || flag.set(true))
                .with_font_requester(install_demo_font_asset)
        };

        // The cube widget takes a shared `Rc<Cell<u8>>` for the MSAA sample
        // count. `build_demo_ui` builds that cell from the saved state and
        // hands it to our factory closure here, then re-uses the same cell for
        // the in-window MSAA toolbar — toggling there flips the cell, the
        // widget reads it on the next paint, and the bar-grid renderer is
        // rebuilt with the new sample count (no relaunch).
        let (app, handles) = demo_ui::build_demo_ui(
            Arc::clone(&font),
            Box::new(|msaa_cell| Box::new(WgpuCubeWidget::new(msaa_cell))),
            "wgpu",
            "native wgpu (winit)",
            initial_state,
            platform,
        );

        // Screen Share demo: bring up the LAN phone server + WebRTC bridge and
        // inject the live transport into the demo's screen-share seam. The wake
        // goes through agg-gui rather than a winit proxy of our own — the shell
        // installs the host waker, and `signal_async_state_change` is what a
        // background thread is supposed to call.
        let wake: Arc<dyn Fn() + Send + Sync> =
            Arc::new(|| agg_gui::animation::signal_async_state_change());
        let screen_share = screen_share::start(&screen_share_runtime, &handles.screen_share, wake);

        handles.screen_size.set(init.size());

        let host = DemoHost {
            handles_show_inspector: Rc::clone(&handles.show_inspector),
            inspector_nodes: Rc::clone(&handles.inspector_nodes),
            hovered_bounds: Rc::clone(&handles.hovered_bounds),
            base_edits: Rc::clone(&handles.base_edits),
            #[cfg(feature = "reflect")]
            inspector_edits: Rc::clone(&handles.inspector_edits),
            run_mode: Rc::clone(&handles.run_mode),
            screen_size: Rc::clone(&handles.screen_size),
            frame_history: Rc::clone(&handles.frame_history),
            window_maximized: Rc::clone(&handles.window_maximized),
            window_fullscreen: Rc::clone(&handles.window_fullscreen),
            screenshot_request: Rc::clone(&handles.screenshot_request),
            screenshot_available: Rc::clone(&handles.screenshot_available),
            screenshot_save_pending: Rc::clone(&handles.screenshot_save_pending),
            screenshot_copy_pending: Rc::clone(&handles.screenshot_copy_pending),
            screenshot_capture_seq: Rc::clone(&handles.screenshot_capture_seq),
            debug_report_requested: Rc::clone(&handles.debug_report_requested),
            relaunch_requested,
            state: handles.state,
            bounds: Rc::clone(&bounds),
            auto_save: agg_gui::persistence::AutoSave::new(),
            runaway: demo_ui::RunawayDetector::new(demo_ui::DEFAULT_RUNAWAY_THRESHOLD),
            _screen_share: screen_share,
        };
        Ok((app, host))
    })
}
