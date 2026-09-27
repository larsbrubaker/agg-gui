//! Canvas / GPU configuration for [`crate::start`].
//!
//! Everything the shell needs before the app exists — which canvas, which
//! browser graphics API, how hard to drive the frame loop. The web twin of
//! `agg_gui_shell::ShellConfig`; platform-neutral so it compiles (and is unit
//! tested) on native too. The runtime that consumes it lives in
//! [`crate::web`] and is wasm-only.

/// Which browser graphics API the shell renders through.
///
/// WebGL2 is always compiled in on wasm32 (agg-gui-wgpu enables wgpu's `webgl`
/// backend there). WebGPU needs this crate's `webgpu` cargo feature (on by
/// default); asking for [`Backend::WebGpu`] without it is a start-up error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// Browser WebGPU only. The choice for an app whose own renderer needs
    /// WebGPU-only features (independent blend, storage textures, compute) —
    /// a browser without WebGPU gets an error instead of a broken render.
    WebGpu,
    /// WebGL2 only — the widest browser coverage, and what agg-gui's own
    /// Pages demo uses.
    WebGl2,
    /// WebGPU when the browser actually hands out an adapter, WebGL2
    /// otherwise. The probe runs before the canvas is bound to a context
    /// (a canvas can only ever get one context type), so the fallback is
    /// clean. Degrades to [`Backend::WebGl2`] when the `webgpu` feature is off.
    PreferWebGpu,
}

impl Default for Backend {
    /// [`Backend::PreferWebGpu`] with the `webgpu` feature, else
    /// [`Backend::WebGl2`].
    fn default() -> Self {
        if cfg!(feature = "webgpu") {
            Self::PreferWebGpu
        } else {
            Self::WebGl2
        }
    }
}

impl Backend {
    /// The wgpu backend set to create the instance with, or `None` when the
    /// requested backend was not compiled in.
    pub fn wgpu_backends(self) -> Option<wgpu::Backends> {
        let webgpu = cfg!(feature = "webgpu");
        match self {
            Self::WebGpu if webgpu => Some(wgpu::Backends::BROWSER_WEBGPU),
            Self::WebGpu => None,
            Self::WebGl2 => Some(wgpu::Backends::GL),
            Self::PreferWebGpu if webgpu => {
                Some(wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL)
            }
            Self::PreferWebGpu => Some(wgpu::Backends::GL),
        }
    }
}

/// How hard the shell drives the frame loop when the app is not asking for
/// frames. Same meaning as `agg_gui_shell::RedrawPolicy`.
///
/// `requestAnimationFrame` ticks at vsync either way; the policy decides
/// whether a tick *paints*. A reactive idle page costs one cheap predicate per
/// vsync and no GPU work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RedrawPolicy {
    /// Paint only when something asks for it: input, an `agg_gui::animation`
    /// request, a widget invalidation, or a due scheduled deadline.
    #[default]
    Reactive,
    /// Paint every tick — what a frame-time graph or a game wants.
    Continuous,
}

impl From<agg_gui::RunMode> for RedrawPolicy {
    fn from(mode: agg_gui::RunMode) -> Self {
        match mode {
            agg_gui::RunMode::Continuous => Self::Continuous,
            agg_gui::RunMode::Reactive => Self::Reactive,
        }
    }
}

/// Everything [`crate::start`] needs before the app exists.
///
/// Build with [`WebShellConfig::new`] and the `with_*` setters; the struct is
/// `#[non_exhaustive]` so knobs can be added without a breaking release.
///
/// ```
/// use agg_gui_web_shell::{Backend, RedrawPolicy, WebShellConfig};
/// let cfg = WebShellConfig::new("canvas")
///     .with_backend(Backend::WebGpu)
///     .with_redraw_policy(RedrawPolicy::Reactive);
/// assert_eq!(cfg.canvas_id, "canvas");
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct WebShellConfig {
    /// `id` of the `<canvas>` element to render into.
    pub canvas_id: String,
    /// Browser graphics API, see [`Backend`].
    pub backend: Backend,
    /// Reactive (default) or continuous. Changeable at runtime through
    /// [`crate::WebShellControl::set_redraw_policy`].
    pub redraw_policy: RedrawPolicy,
    /// `wgpu::DeviceDescriptor` label.
    pub device_label: String,
    /// Device features requested **when the adapter offers them** (masked
    /// against `adapter.features()`), like
    /// `agg_gui_wgpu::GpuConfig::with_optional_features`.
    pub optional_features: wgpu::Features,
    /// Device limits to request. `None` (default) asks for
    /// `Limits::downlevel_webgl2_defaults()` on WebGL2 and `Limits::default()`
    /// on WebGPU, each with the texture-dimension caps raised to what the
    /// adapter offers (`using_resolution`) — a DPR-3 phone canvas overshoots
    /// the conservative 2048 WebGL2 default. Set it when the app's own
    /// renderer needs more (storage-buffer sizes for compute, for example).
    pub limits: Option<wgpu::Limits>,
    /// Render every frame into an offscreen scene texture and blit it to the
    /// canvas, so `WgpuGfxCtx::capture_screenshot` / `read_screenshot` work.
    /// The web counterpart of `CopySrc::Required`: browser surfaces only
    /// advertise `RENDER_ATTACHMENT`, so the surface itself can't be copied.
    pub offscreen_scene: bool,
    /// Detect the client platform (`navigator.userAgent`, `(pointer: coarse)`,
    /// `?agg_input=mobile|desktop`) at boot and apply agg-gui's platform,
    /// input profile, on-screen keyboard and UX scale. On by default; turn off
    /// if the app sets these itself.
    pub detect_platform: bool,
    /// Replace the canvas with a readable message when GPU init fails, so a
    /// user without WebGPU sees *why* the page is blank. On by default.
    pub fatal_panel: bool,
    /// Install `console_error_panic_hook`. On by default.
    pub panic_hook: bool,
}

impl WebShellConfig {
    /// Defaults for `<canvas id="{canvas_id}">`.
    pub fn new(canvas_id: impl Into<String>) -> Self {
        Self {
            canvas_id: canvas_id.into(),
            backend: Backend::default(),
            redraw_policy: RedrawPolicy::Reactive,
            device_label: "agg-gui-web-shell".to_string(),
            optional_features: wgpu::Features::empty(),
            limits: None,
            offscreen_scene: false,
            detect_platform: true,
            fatal_panel: true,
            panic_hook: true,
        }
    }

    pub fn with_backend(mut self, backend: Backend) -> Self {
        self.backend = backend;
        self
    }

    pub fn with_redraw_policy(mut self, policy: RedrawPolicy) -> Self {
        self.redraw_policy = policy;
        self
    }

    pub fn with_device_label(mut self, label: impl Into<String>) -> Self {
        self.device_label = label.into();
        self
    }

    pub fn with_optional_features(mut self, features: wgpu::Features) -> Self {
        self.optional_features = features;
        self
    }

    pub fn with_limits(mut self, limits: wgpu::Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn with_offscreen_scene(mut self, on: bool) -> Self {
        self.offscreen_scene = on;
        self
    }

    pub fn with_platform_detection(mut self, on: bool) -> Self {
        self.detect_platform = on;
        self
    }

    pub fn with_fatal_panel(mut self, on: bool) -> Self {
        self.fatal_panel = on;
        self
    }

    pub fn with_panic_hook(mut self, on: bool) -> Self {
        self.panic_hook = on;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webgl2_is_always_available() {
        assert_eq!(Backend::WebGl2.wgpu_backends(), Some(wgpu::Backends::GL));
    }

    #[test]
    fn webgpu_availability_follows_the_feature() {
        let got = Backend::WebGpu.wgpu_backends();
        if cfg!(feature = "webgpu") {
            assert_eq!(got, Some(wgpu::Backends::BROWSER_WEBGPU));
        } else {
            assert_eq!(got, None);
        }
    }

    #[test]
    fn prefer_webgpu_always_includes_the_gl_fallback() {
        let got = Backend::PreferWebGpu
            .wgpu_backends()
            .expect("always buildable");
        assert!(got.contains(wgpu::Backends::GL));
        assert_eq!(
            got.contains(wgpu::Backends::BROWSER_WEBGPU),
            cfg!(feature = "webgpu")
        );
    }

    #[test]
    fn run_mode_maps_to_redraw_policy() {
        assert_eq!(
            RedrawPolicy::from(agg_gui::RunMode::Continuous),
            RedrawPolicy::Continuous
        );
        assert_eq!(
            RedrawPolicy::from(agg_gui::RunMode::Reactive),
            RedrawPolicy::Reactive
        );
    }

    #[test]
    fn defaults_are_reactive_with_detection_and_panel() {
        let cfg = WebShellConfig::new("c");
        assert_eq!(cfg.redraw_policy, RedrawPolicy::Reactive);
        assert!(cfg.detect_platform && cfg.fatal_panel && cfg.panic_hook);
        assert!(!cfg.offscreen_scene);
        assert_eq!(cfg.backend, Backend::default());
    }
}
