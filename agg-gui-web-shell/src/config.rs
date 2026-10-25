//! Canvas / GPU configuration for [`crate::start`].
//!
//! Everything the shell needs before the app exists — which canvas, which
//! browser graphics API, how hard to drive the frame loop. The web twin of
//! `agg_gui_shell::ShellConfig`; platform-neutral so it compiles (and is unit
//! tested) on native too. The runtime that consumes it lives in
//! [`crate::web`] and is wasm-only.

use crate::adapter_check::AdapterCheck;
use crate::fatal::{FatalHook, FatalMessageFn};

/// Which browser graphics API the shell renders through.
///
/// Each API is a cargo feature of this crate, both on by default: `webgpu`
/// (wgpu's browser WebGPU backend) and `webgl` (wgpu's WebGL2 backend, through
/// agg-gui-wgpu). Asking for an API that was not compiled in is a start-up
/// error ([`crate::WebShellError::BackendNotCompiled`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
    /// clean. Degrades to whichever of the two is compiled in when the
    /// `webgpu` or the `webgl` feature is off.
    ///
    /// Limits and features apply to whichever API it lands on: an explicit
    /// [`WebShellConfig::limits`] or [`WebShellConfig::required_features`]
    /// sized for WebGPU will fail the device request on the WebGL2 fallback
    /// (WebGL2 caps are far lower — e.g. no storage buffers). Leave `limits`
    /// at `None` (per-backend defaults), keep WebGPU-only features in
    /// `optional_features`, and branch on [`crate::GpuInfo::backend`]; or use
    /// [`Backend::WebGpu`] when the app cannot run without them.
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
        let webgpu = if cfg!(feature = "webgpu") {
            wgpu::Backends::BROWSER_WEBGPU
        } else {
            wgpu::Backends::empty()
        };
        let webgl = if cfg!(feature = "webgl") {
            wgpu::Backends::GL
        } else {
            wgpu::Backends::empty()
        };
        let wanted = match self {
            Self::WebGpu => webgpu,
            Self::WebGl2 => webgl,
            Self::PreferWebGpu => webgpu | webgl,
        };
        (!wanted.is_empty()).then_some(wanted)
    }
}

/// How hard the shell drives the frame loop when the app is not asking for
/// frames. Same meaning as `agg_gui_shell::RedrawPolicy`.
///
/// `requestAnimationFrame` ticks at vsync either way; the policy decides
/// whether a tick *paints*. A reactive idle page costs one cheap predicate per
/// vsync and no GPU work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
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
    /// Device features the app cannot run without. An adapter lacking any of
    /// them fails start-up with [`crate::WebShellError::MissingFeatures`]
    /// (and the fatal panel) instead of a device the app can't use.
    pub required_features: wgpu::Features,
    /// Device limits to request. `None` (default) asks for
    /// `Limits::downlevel_webgl2_defaults()` on WebGL2 and `Limits::default()`
    /// on WebGPU, each with the texture-dimension caps raised to what the
    /// adapter offers (`using_resolution`) — a DPR-3 phone canvas overshoots
    /// the conservative 2048 WebGL2 default. Set it when the app's own
    /// renderer needs more (storage-buffer sizes for compute, for example).
    pub limits: Option<wgpu::Limits>,
    /// The app's check of the adapter the browser handed out, run before the
    /// device is requested (and again on a device-loss rebuild). Its `Err`
    /// message stops start-up as [`crate::WebShellError::AdapterRejected`]
    /// through the fatal path, so a browser whose graphics can't draw what the
    /// app's renderer needs is told so plainly. `None` (default) accepts every
    /// adapter. The adapter itself reaches the app builder too, in
    /// [`crate::GpuInfo::adapter`].
    pub adapter_check: Option<AdapterCheck>,
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
    /// Human-readable app name; prefixes the fatal message
    /// (`"{name} could not start: …"`) unless [`Self::fatal_message`] is set.
    pub app_name: Option<String>,
    /// Composes the fatal panel / console text from the error, replacing the
    /// default wording entirely.
    pub fatal_message: Option<FatalMessageFn>,
    /// CSS class for the fatal panel. `None` (default) uses the built-in
    /// class and stylesheet (light, with a `prefers-color-scheme: dark`
    /// variant); `Some` sets only this class and injects no styles, so the
    /// page's own CSS owns the look.
    pub fatal_panel_class: Option<String>,
    /// Called for every fatal error, with or without the panel — the way
    /// asynchronous start-up failures and an unrecoverable device loss reach
    /// the app (they happen after [`crate::start`] has returned).
    pub on_fatal: Option<FatalHook>,
    /// Keys and the clipboard reach the app only while the canvas (or the
    /// shell's IME input) has focus, and a press on the canvas focuses it.
    /// A canvas the page gave no `tabindex` gets `tabindex="0"` (it could
    /// not take focus otherwise); the page may want `outline: none` on it.
    /// A key whose keydown reached the app still has its keyup delivered
    /// after focus moves away.
    /// Off by default: a full-window app owns every key on its page. Turn it
    /// on for a canvas embedded in a larger page, so the page's own keyboard
    /// scrolling, find and copy keep working while it has focus.
    ///
    /// A full-window app's canvas (this off) is also kept inside the visible
    /// viewport: the shell caps its `max-height` at the window's bottom, so a
    /// `100vh` canvas on Android Chrome with the address bar showing is not
    /// laid out under the bottom edge, on-screen keyboard and all. An
    /// embedded canvas (this on) is left at the size the page gives it.
    pub keyboard_needs_focus: bool,
    /// Ctrl/Cmd shortcuts (by key, with or without Shift) the app handles
    /// itself and takes from the browser, beyond the clipboard and undo set
    /// every app gets: `'s'` keeps the browser's "Save Page As" from opening
    /// when the app saves. Applied at start through
    /// [`agg_gui::claim_web_shortcut`], which an app can also call later.
    /// Empty by default.
    pub claimed_shortcuts: Vec<char>,
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
            required_features: wgpu::Features::empty(),
            limits: None,
            adapter_check: None,
            offscreen_scene: false,
            detect_platform: true,
            fatal_panel: true,
            panic_hook: true,
            app_name: None,
            fatal_message: None,
            fatal_panel_class: None,
            on_fatal: None,
            keyboard_needs_focus: false,
            claimed_shortcuts: Vec::new(),
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

    /// Features the device must have — see [`Self::required_features`].
    pub fn with_required_features(mut self, features: wgpu::Features) -> Self {
        self.required_features = features;
        self
    }

    pub fn with_limits(mut self, limits: wgpu::Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Check the adapter before the device is requested (see
    /// [`Self::adapter_check`]): return `Err` with the message the user
    /// should see to refuse it.
    pub fn with_adapter_check(
        mut self,
        check: impl Fn(&wgpu::Adapter) -> Result<(), String> + 'static,
    ) -> Self {
        self.adapter_check = Some(AdapterCheck(std::rc::Rc::new(check)));
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

    /// Name the app in the default fatal message.
    pub fn with_app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = Some(name.into());
        self
    }

    /// Compose the fatal message yourself.
    pub fn with_fatal_message(mut self, compose: FatalMessageFn) -> Self {
        self.fatal_message = Some(compose);
        self
    }

    /// Style the fatal panel with the page's own CSS class.
    pub fn with_fatal_panel_class(mut self, class: impl Into<String>) -> Self {
        self.fatal_panel_class = Some(class.into());
        self
    }

    /// Run `hook` for every fatal error (see [`Self::on_fatal`]).
    pub fn with_on_fatal(mut self, hook: impl Fn(&crate::WebShellError) + 'static) -> Self {
        self.on_fatal = Some(FatalHook(std::rc::Rc::new(hook)));
        self
    }

    /// See [`Self::keyboard_needs_focus`].
    pub fn with_keyboard_needs_focus(mut self, on: bool) -> Self {
        self.keyboard_needs_focus = on;
        self
    }

    /// Claim Ctrl/Cmd+`key` for the app (see [`Self::claimed_shortcuts`]).
    pub fn with_claimed_shortcut(mut self, key: char) -> Self {
        if !self.claimed_shortcuts.contains(&key) {
            self.claimed_shortcuts.push(key);
        }
        self
    }

    /// Hand [`Self::claimed_shortcuts`] to the keyboard bridge's policy.
    /// Start-up calls it before the keyboard listeners go in.
    // Only the wasm runtime calls this outside the tests.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(crate) fn apply_shortcut_claims(&self) {
        for key in &self.claimed_shortcuts {
            agg_gui::claim_web_shortcut(*key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webgl2_availability_follows_the_feature() {
        let got = Backend::WebGl2.wgpu_backends();
        if cfg!(feature = "webgl") {
            assert_eq!(got, Some(wgpu::Backends::GL));
        } else {
            assert_eq!(got, None);
        }
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
    fn prefer_webgpu_includes_every_compiled_in_backend() {
        let got = Backend::PreferWebGpu.wgpu_backends();
        if !cfg!(feature = "webgpu") && !cfg!(feature = "webgl") {
            assert_eq!(got, None);
            return;
        }
        let got = got.expect("buildable with either backend compiled in");
        assert_eq!(got.contains(wgpu::Backends::GL), cfg!(feature = "webgl"));
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
        assert!(cfg.required_features.is_empty());
        assert!(cfg.adapter_check.is_none());
        assert!(cfg.app_name.is_none() && cfg.fatal_message.is_none());
        assert!(cfg.fatal_panel_class.is_none() && cfg.on_fatal.is_none());
        assert!(!cfg.keyboard_needs_focus);
        assert!(cfg.claimed_shortcuts.is_empty());
    }

    #[test]
    fn an_embedded_canvas_can_ask_for_keys_only_with_focus() {
        let cfg = WebShellConfig::new("c").with_keyboard_needs_focus(true);
        assert!(cfg.keyboard_needs_focus);
    }

    #[test]
    fn a_claimed_save_shortcut_reaches_the_key_policy() {
        assert!(!agg_gui::is_web_shortcut_claimed('s'));
        let cfg = WebShellConfig::new("c")
            .with_claimed_shortcut('s')
            .with_claimed_shortcut('s');
        assert_eq!(cfg.claimed_shortcuts, vec!['s']);
        cfg.apply_shortcut_claims();
        assert!(agg_gui::is_web_shortcut_claimed('s'));
        assert!(agg_gui::is_web_shortcut_claimed('S'));
        assert!(!agg_gui::is_web_shortcut_claimed('p'));
        agg_gui::release_web_shortcut('s');
    }
}
