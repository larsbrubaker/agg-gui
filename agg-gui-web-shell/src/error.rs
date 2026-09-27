//! [`WebShellError`] — why the web shell could not start — and [`GpuInfo`],
//! the device bundle handed to the app builder and to
//! [`crate::WebShellHost::on_gpu_rebuilt`].
//!
//! Platform-neutral (plain wgpu types), so both compile natively and the
//! builder signature documented on docs.rs is the real one.

use std::sync::Arc;

use crate::config::Backend;

/// Why the shell could not start.
#[derive(Debug)]
#[non_exhaustive]
pub enum WebShellError {
    /// No `window` / `document` — not running on a browser main thread.
    NoWindow,
    /// No `<canvas>` with the configured id (or the element is not a canvas).
    CanvasNotFound(String),
    /// The configured [`Backend`] was not compiled in (WebGPU without this
    /// crate's `webgpu` feature).
    BackendNotCompiled(Backend),
    /// [`Backend::WebGpu`] was required and the browser has no usable WebGPU.
    WebGpuUnavailable,
    /// wgpu could not bind a surface to the canvas.
    CreateSurface(String),
    /// No adapter compatible with the canvas surface.
    RequestAdapter(String),
    /// The adapter refused the device request (limits / features).
    RequestDevice(String),
    /// The surface reported no texture formats or alpha modes.
    UnusableSurface,
    /// The adapter lacks features the app marked required with
    /// [`crate::WebShellConfig::with_required_features`].
    MissingFeatures(wgpu::Features),
    /// The app's builder closure failed. Build one with [`WebShellError::app`].
    App(Box<dyn std::error::Error>),
    /// [`crate::start`] was called a second time on this page. The shell owns
    /// page-global state (one app, one rAF loop); the first instance keeps
    /// running.
    AlreadyStarted,
    /// The GPU device (or WebGL2 context) was lost and could not be rebuilt.
    DeviceLost(String),
}

impl WebShellError {
    /// Wrap an app-side start-up failure so a builder closure can use `?`.
    pub fn app(error: impl Into<Box<dyn std::error::Error>>) -> Self {
        Self::App(error.into())
    }
}

impl std::fmt::Display for WebShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindow => write!(f, "no browser window/document"),
            Self::CanvasNotFound(id) => write!(f, "canvas element #{id} not found"),
            Self::BackendNotCompiled(b) => write!(
                f,
                "{b:?} backend not compiled in (enable agg-gui-web-shell's `webgpu` feature)"
            ),
            Self::WebGpuUnavailable => write!(
                f,
                "WebGPU is not available in this browser. This app needs WebGPU \
                 (Chrome/Edge 113+, Firefox 141+, Safari 26+ — or enable it in \
                 the browser's settings)."
            ),
            Self::CreateSurface(e) => write!(f, "create canvas surface: {e}"),
            Self::RequestAdapter(e) => write!(f, "no suitable GPU adapter: {e}"),
            Self::RequestDevice(e) => write!(f, "GPU device request failed: {e}"),
            Self::UnusableSurface => write!(f, "canvas surface reports no formats/alpha modes"),
            Self::MissingFeatures(missing) => {
                write!(f, "this GPU lacks required features: {missing:?}")
            }
            Self::App(e) => write!(f, "app start-up: {e}"),
            Self::AlreadyStarted => write!(f, "agg-gui-web-shell was already started on this page"),
            Self::DeviceLost(e) => write!(f, "the GPU was lost and could not be recovered: {e}"),
        }
    }
}

impl std::error::Error for WebShellError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::App(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

/// The live GPU: what an app with its own wgpu renderer builds from.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct GpuInfo {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub surface_format: wgpu::TextureFormat,
    /// `BrowserWebGpu` or `Gl` — which API [`Backend::PreferWebGpu`] landed on.
    pub backend: wgpu::Backend,
    pub adapter_info: wgpu::AdapterInfo,
}

/// Pick the swap-chain format: non-sRGB preferred (the renderer writes
/// linear-space values and must not be gamma-corrected twice), else the
/// surface's first preference.
pub(crate) fn pick_surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| formats.first().copied())
}

/// The device limits to request for `backend`, honouring an explicit override.
pub(crate) fn device_limits(
    backend: wgpu::Backend,
    adapter_limits: wgpu::Limits,
    override_limits: Option<&wgpu::Limits>,
) -> wgpu::Limits {
    if let Some(l) = override_limits {
        return l.clone();
    }
    let base = if backend == wgpu::Backend::Gl {
        wgpu::Limits::downlevel_webgl2_defaults()
    } else {
        wgpu::Limits::default()
    };
    base.using_resolution(adapter_limits)
}

/// The device features to request: every `required` feature (an error naming
/// the missing ones when the adapter lacks any) plus the `optional` ones the
/// adapter offers.
pub(crate) fn device_features(
    adapter: wgpu::Features,
    required: wgpu::Features,
    optional: wgpu::Features,
) -> Result<wgpu::Features, WebShellError> {
    let missing = required.difference(adapter);
    if !missing.is_empty() {
        return Err(WebShellError::MissingFeatures(missing));
    }
    Ok(required | (optional & adapter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_non_srgb_format() {
        use wgpu::TextureFormat as F;
        assert_eq!(
            pick_surface_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm]),
            Some(F::Bgra8Unorm)
        );
        assert_eq!(
            pick_surface_format(&[F::Rgba8UnormSrgb]),
            Some(F::Rgba8UnormSrgb)
        );
        assert_eq!(pick_surface_format(&[]), None);
    }

    #[test]
    fn gl_limits_are_webgl2_with_adapter_resolution() {
        let mut adapter = wgpu::Limits::downlevel_webgl2_defaults();
        adapter.max_texture_dimension_2d = 8192;
        let got = device_limits(wgpu::Backend::Gl, adapter, None);
        assert_eq!(got.max_texture_dimension_2d, 8192);
        assert_eq!(
            got.max_storage_buffers_per_shader_stage,
            wgpu::Limits::downlevel_webgl2_defaults().max_storage_buffers_per_shader_stage
        );
    }

    #[test]
    fn override_wins() {
        let want = wgpu::Limits {
            max_bind_groups: 3,
            ..wgpu::Limits::default()
        };
        let got = device_limits(
            wgpu::Backend::BrowserWebGpu,
            wgpu::Limits::default(),
            Some(&want),
        );
        assert_eq!(got.max_bind_groups, 3);
    }

    #[test]
    fn required_features_must_be_offered() {
        use wgpu::Features as F;
        let adapter = F::DEPTH_CLIP_CONTROL;
        assert_eq!(
            device_features(adapter, F::DEPTH_CLIP_CONTROL, F::empty()).unwrap(),
            F::DEPTH_CLIP_CONTROL
        );
        match device_features(adapter, F::TIMESTAMP_QUERY, F::empty()) {
            Err(WebShellError::MissingFeatures(m)) => assert_eq!(m, F::TIMESTAMP_QUERY),
            other => panic!("expected MissingFeatures, got {other:?}"),
        }
    }

    #[test]
    fn optional_features_are_masked() {
        use wgpu::Features as F;
        let got = device_features(
            F::DEPTH_CLIP_CONTROL,
            F::empty(),
            F::DEPTH_CLIP_CONTROL | F::TIMESTAMP_QUERY,
        )
        .unwrap();
        assert_eq!(got, F::DEPTH_CLIP_CONTROL);
    }

    #[test]
    fn new_variants_display() {
        assert!(WebShellError::AlreadyStarted
            .to_string()
            .contains("already started"));
        assert!(WebShellError::DeviceLost("x".into())
            .to_string()
            .contains("lost"));
    }
}
