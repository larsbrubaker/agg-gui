//! Window / surface configuration for [`crate::run`].
//!
//! Everything the shell needs to open the OS window and configure the swap
//! chain before the app exists. Split out of `run.rs` so the event loop file
//! stays about the loop; the persistence trait it references lives in
//! [`crate::bounds`].

use std::path::PathBuf;

use agg_gui_wgpu::CopySrc;

use crate::bounds::WindowBoundsStore;

/// A size in either DPI-independent (logical) or device (physical) pixels.
///
/// Restored window sizes are saved in **physical** pixels and must be restored
/// in the same unit: handing a saved physical size back as a logical one
/// re-multiplies it by the monitor scale on every launch, which is the
/// DPI-ratchet bug that eventually grows the surface past the GPU's maximum
/// texture dimension. See [`crate::bounds::sanitize_restored_window_size`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowSize {
    /// DPI-independent pixels — what a first-launch default should use.
    Logical(f64, f64),
    /// Device pixels — what a restored size uses.
    Physical(u32, u32),
}

impl WindowSize {
    pub(crate) fn to_winit(self) -> winit::dpi::Size {
        match self {
            Self::Logical(w, h) => winit::dpi::LogicalSize::new(w, h).into(),
            Self::Physical(w, h) => winit::dpi::PhysicalSize::new(w, h).into(),
        }
    }
}

/// How hard the shell drives the event loop when the app is not asking for
/// frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RedrawPolicy {
    /// Paint only when something asks for it: an input event, an
    /// `agg_gui::animation` request, or a due scheduled deadline. The event
    /// loop parks in `ControlFlow::Wait`/`WaitUntil` otherwise, so an idle app
    /// costs no CPU.
    #[default]
    Reactive,
    /// Paint every iteration regardless. What a frame-time graph or a
    /// game-style app wants; costs a core.
    Continuous,
}

/// An RGBA8 window icon, as the OS taskbar / title bar wants it.
///
/// Built through [`ShellConfig::with_icon_rgba`]; `#[non_exhaustive]` so it can
/// grow (a separate small icon, a mask) without a breaking release.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct WindowIcon {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Deterministic frame capture: after `settle_frames` painted frames, read the
/// frame back, write it to `path` as a PNG, and leave the event loop.
///
/// Built through [`ShellConfig::with_screenshot`]; `#[non_exhaustive]` so it
/// can grow (a capture region, a scale) without a breaking release.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ScreenshotConfig {
    pub path: PathBuf,
    /// Frames to paint before capturing, clamped to at least 1. ~6 gives
    /// fonts, layout and start-up animations time to settle.
    pub settle_frames: u32,
}

/// Everything [`crate::run`] needs before the app exists.
///
/// Build one with [`ShellConfig::new`] and the `with_*` setters rather than a
/// struct literal — the struct is `#[non_exhaustive]` so new knobs can be
/// added without a breaking release.
///
/// ```no_run
/// use agg_gui_shell::{RedrawPolicy, ShellConfig};
/// let cfg = ShellConfig::new("My App")
///     .with_logical_size(1280.0, 720.0)
///     .with_min_logical_size(800.0, 600.0)
///     .with_redraw_policy(RedrawPolicy::Reactive);
/// ```
#[non_exhaustive]
pub struct ShellConfig {
    /// OS window title. An owned `String` because a title is routinely built
    /// at runtime (document name, version, profile).
    pub title: String,
    /// Initial inner size, used when no [`WindowBoundsStore`] returns a saved
    /// size. Defaults to 1280x720 logical.
    pub size: WindowSize,
    /// Minimum inner size enforced by the window system. `None` =
    /// unconstrained.
    pub min_size: Option<WindowSize>,
    /// Taskbar / title-bar icon.
    pub icon: Option<WindowIcon>,
    /// Start maximized. A [`WindowBoundsStore`] that returns a maximized
    /// state overrides this.
    pub maximized: bool,
    /// Start in borderless fullscreen. The app can toggle later through
    /// `agg_gui::fullscreen::request_toggle`.
    pub fullscreen: bool,
    /// Reactive (default) or continuous redraw. The host can change this at
    /// runtime through [`crate::ShellControl::set_redraw_policy`].
    pub redraw_policy: RedrawPolicy,
    /// Surface read-back requirement, passed through to `agg_gui_wgpu::Gpu`.
    /// Forced to [`CopySrc::Required`] when a screenshot is configured.
    pub copy_src: CopySrc,
    /// Swap-chain present mode, passed through to `agg_gui_wgpu::Gpu` (which
    /// falls back to `Fifo` if the surface does not support it).
    pub present_mode: wgpu::PresentMode,
    /// `wgpu::DeviceDescriptor` label — shows up in backend validation
    /// messages and GPU captures.
    pub device_label: &'static str,
    /// Device features requested when the adapter offers them, passed through
    /// to `agg_gui_wgpu::GpuConfig` (which masks them against
    /// `adapter.features()` so an adapter lacking one still yields a device).
    pub optional_features: wgpu::Features,
    /// Deterministic capture, see [`ScreenshotConfig`].
    pub screenshot: Option<ScreenshotConfig>,
    /// Seed `agg_gui`'s tooltip timings from the OS (Windows:
    /// `SPI_GETMOUSEHOVERTIME`). On by default; other platforms keep the
    /// library defaults regardless.
    pub os_tooltip_timings: bool,
    /// Where the window size / maximized state is restored from and saved to.
    /// `None` = no persistence.
    pub bounds_store: Option<Box<dyn WindowBoundsStore>>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            title: String::new(),
            size: WindowSize::Logical(1280.0, 720.0),
            min_size: None,
            icon: None,
            maximized: false,
            fullscreen: false,
            redraw_policy: RedrawPolicy::Reactive,
            copy_src: CopySrc::Never,
            present_mode: wgpu::PresentMode::AutoVsync,
            device_label: "agg-gui-shell",
            optional_features: wgpu::Features::empty(),
            screenshot: None,
            os_tooltip_timings: true,
            bounds_store: None,
        }
    }
}

impl ShellConfig {
    /// Default configuration under a window title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    /// Initial inner size in DPI-independent pixels.
    pub fn with_logical_size(mut self, w: f64, h: f64) -> Self {
        self.size = WindowSize::Logical(w, h);
        self
    }

    /// Initial inner size in device pixels — the unit a restored size uses.
    pub fn with_physical_size(mut self, w: u32, h: u32) -> Self {
        self.size = WindowSize::Physical(w, h);
        self
    }

    /// Refuse to let the user shrink the window below `(w, h)` logical pixels.
    pub fn with_min_logical_size(mut self, w: f64, h: f64) -> Self {
        self.min_size = Some(WindowSize::Logical(w, h));
        self
    }

    /// Refuse to let the user shrink the window below `(w, h)` device pixels.
    pub fn with_min_physical_size(mut self, w: u32, h: u32) -> Self {
        self.min_size = Some(WindowSize::Physical(w, h));
        self
    }

    /// Window icon from raw RGBA8 bytes (`width * height * 4`). An icon that
    /// winit rejects is logged and dropped — an app should still open.
    pub fn with_icon_rgba(mut self, rgba: impl Into<Vec<u8>>, width: u32, height: u32) -> Self {
        self.icon = Some(WindowIcon {
            rgba: rgba.into(),
            width,
            height,
        });
        self
    }

    pub fn with_maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    pub fn with_redraw_policy(mut self, policy: RedrawPolicy) -> Self {
        self.redraw_policy = policy;
        self
    }

    pub fn with_copy_src(mut self, copy_src: CopySrc) -> Self {
        self.copy_src = copy_src;
        self
    }

    pub fn with_present_mode(mut self, present_mode: wgpu::PresentMode) -> Self {
        self.present_mode = present_mode;
        self
    }

    pub fn with_device_label(mut self, label: &'static str) -> Self {
        self.device_label = label;
        self
    }

    /// Request these device features when — and only when — the adapter
    /// offers them. See [`ShellConfig::optional_features`].
    pub fn with_optional_features(mut self, features: wgpu::Features) -> Self {
        self.optional_features = features;
        self
    }

    /// Headless-style capture: after `settle_frames` painted frames, read the
    /// frame back, write it as a PNG to `path`, and leave the event loop.
    /// The shell polls and paints every idle iteration until the capture
    /// fires, so the frame count advances without any user input.
    ///
    /// The image is in PHYSICAL pixels — on a 2x display that is twice the
    /// logical window size. A failed capture ends [`crate::run`] with
    /// [`crate::ShellError::Screenshot`]; the shell never exits the process
    /// itself.
    pub fn with_screenshot(mut self, path: impl Into<PathBuf>, settle_frames: u32) -> Self {
        self.screenshot = Some(ScreenshotConfig {
            path: path.into(),
            settle_frames,
        });
        self
    }

    pub fn with_os_tooltip_timings(mut self, enabled: bool) -> Self {
        self.os_tooltip_timings = enabled;
        self
    }

    /// Restore the window size / maximized state from `store` at startup and
    /// save it back as it changes. See [`WindowBoundsStore`].
    pub fn with_bounds_store(mut self, store: impl WindowBoundsStore + 'static) -> Self {
        self.bounds_store = Some(Box::new(store));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults_and_overrides() {
        let cfg = ShellConfig::new("Title");
        assert_eq!(cfg.title, "Title");
        assert_eq!(cfg.size, WindowSize::Logical(1280.0, 720.0));
        assert_eq!(cfg.redraw_policy, RedrawPolicy::Reactive);
        assert!(cfg.os_tooltip_timings);

        let cfg = cfg
            .with_physical_size(1600, 900)
            .with_min_logical_size(640.0, 480.0)
            .with_redraw_policy(RedrawPolicy::Continuous)
            .with_screenshot("out.png", 6);
        assert_eq!(cfg.size, WindowSize::Physical(1600, 900));
        assert_eq!(cfg.min_size, Some(WindowSize::Logical(640.0, 480.0)));
        assert_eq!(cfg.redraw_policy, RedrawPolicy::Continuous);
        assert_eq!(cfg.screenshot.map(|s| s.settle_frames), Some(6));
    }

    #[test]
    fn min_size_can_be_given_in_either_unit() {
        let cfg = ShellConfig::new("Title").with_min_physical_size(320, 240);
        assert_eq!(cfg.min_size, Some(WindowSize::Physical(320, 240)));
    }
}
