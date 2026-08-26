//! Deprecated compatibility wrapper over [`agg_gui_shell`].
//!
//! This module used to *be* the native shell. It now forwards to the
//! `agg-gui-shell` crate, which is the published, maintained version of the
//! same thing plus everything `demo-native`'s hand-rolled loop had that this
//! one didn't (touch, cursor-leave, window-bounds persistence, the host waker,
//! device-loss recovery, …).
//!
//! The old entry point is kept only so external path-dependency consumers keep
//! compiling; new code should call [`agg_gui_shell::run`] directly, which gives
//! it a builder closure, a real error type, and the [`agg_gui_shell::ShellHost`]
//! hooks:
//!
//! ```ignore
//! agg_gui_shell::run(
//!     agg_gui_shell::ShellConfig::new("My App").with_logical_size(1024.0, 768.0),
//!     |_init| Ok((build_my_app(), agg_gui_shell::NoHost)),
//! )
//! ```
//!
//! The web equivalent is [`crate::web_shell`].

use std::path::PathBuf;

use agg_gui::App;
use agg_gui_shell::{Frame, ShellConfig, ShellHost};

/// Window parameters for [`run`].
///
/// Deprecated alongside [`run`]; [`agg_gui_shell::ShellConfig`] is the
/// replacement and takes an owned `String` title, physical sizes, an icon,
/// a redraw policy and a bounds store as well.
#[deprecated(
    since = "0.5.0",
    note = "use agg_gui_shell::ShellConfig with agg_gui_shell::run"
)]
pub struct NativeShellConfig {
    /// OS window title.
    pub title: &'static str,
    /// Initial inner size in logical (DPI-independent) pixels.
    pub logical_size: (f64, f64),
    /// Minimum inner size in logical pixels. `None` = unconstrained.
    pub min_size: Option<(f64, f64)>,
    /// Deterministic capture target: `(png path, settle frames)`.
    pub screenshot: Option<(PathBuf, u32)>,
}

#[allow(deprecated)]
impl NativeShellConfig {
    pub fn new(title: &'static str, logical_size: (f64, f64)) -> Self {
        Self {
            title,
            logical_size,
            min_size: None,
            screenshot: None,
        }
    }

    /// Refuse to let the user shrink the window below `(w, h)` logical pixels.
    pub fn with_min_size(mut self, w: f64, h: f64) -> Self {
        self.min_size = Some((w, h));
        self
    }

    /// Headless-style capture: after `settle_frames` painted frames, write the
    /// frame to `path` as a PNG and exit.
    pub fn with_screenshot(mut self, path: impl Into<PathBuf>, settle_frames: u32) -> Self {
        self.screenshot = Some((path.into(), settle_frames));
        self
    }
}

/// A per-frame closure in [`ShellHost`] clothing — all this wrapper's callers
/// ever supplied.
struct OnFrameHost<F: FnMut()>(F);

impl<F: FnMut()> ShellHost for OnFrameHost<F> {
    fn on_frame(&mut self, _app: &mut App, _frame: &Frame) {
        (self.0)();
    }
}

/// Open an OS window, wire all input into `app`, and run the event loop until
/// the window closes.
///
/// Kept for source compatibility only: it swallows the app-visible error type
/// and exits the process on failure, which is exactly what a library should not
/// do. Call [`agg_gui_shell::run`] instead.
#[deprecated(
    since = "0.5.0",
    note = "use agg_gui_shell::run, which returns a ShellError instead of exiting the process"
)]
#[allow(deprecated)]
pub fn run(config: NativeShellConfig, app: App, on_frame: impl FnMut() + 'static) {
    let mut shell_config = ShellConfig::new(config.title)
        .with_logical_size(config.logical_size.0, config.logical_size.1)
        .with_device_label("agg-gui-native-shell");
    if let Some((w, h)) = config.min_size {
        shell_config = shell_config.with_min_logical_size(w, h);
    }
    if let Some((path, settle_frames)) = config.screenshot {
        shell_config = shell_config.with_screenshot(path, settle_frames);
    }

    // The old signature has no way to report a failure, and its callers are
    // `fn main()`s that treated one as fatal — so preserve that behaviour here
    // rather than silently painting nothing.
    if let Err(err) =
        agg_gui_shell::run(shell_config, move |_init| Ok((app, OnFrameHost(on_frame))))
    {
        eprintln!("native shell: {err}");
        std::process::exit(1);
    }
}
