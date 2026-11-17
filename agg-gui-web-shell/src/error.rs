//! [`WebShellError`] — why the web shell could not start — and [`GpuInfo`],
//! the device bundle (with the adapter it came from) handed to the app
//! builder and to [`crate::WebShellHost::on_gpu_rebuilt`].
//!
//! Platform-neutral (plain wgpu types), so both compile natively and the
//! builder signature documented on docs.rs is the real one.
//!
//! It also holds the pure choices the canvas GPU bring-up (the crate's
//! wasm-only `web::gpu`) makes: the swap-chain format (with Firefox's
//! `Rgba8Unorm` preference and the user-agent check behind it), the device
//! limits and the device features. That code only runs in a browser, so its
//! decisions live here, where `cargo test` reaches them natively.

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
    /// crate's `webgpu` feature, WebGL2 without its `webgl` feature).
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
    /// The app's adapter check ([`crate::WebShellConfig::with_adapter_check`])
    /// refused the adapter; the message is the app's own, shown as it is.
    AdapterRejected(String),
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
                "{b:?} backend not compiled in (enable agg-gui-web-shell's `webgpu` or `webgl` feature)"
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
            Self::AdapterRejected(why) => f.write_str(why),
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
    /// The adapter the device came from, for what a device cannot report:
    /// `get_downlevel_capabilities()` and `get_texture_format_features(f)`.
    pub adapter: wgpu::Adapter,
}

/// Pick the swap-chain format: agg-gui-wgpu's shared rule
/// ([`agg_gui_wgpu::choose_surface_format`]: Bgra8Unorm, else non-sRGB, else
/// the surface's first preference), so a browser canvas lands on the same
/// format as a native window whichever 8-bit order it lists first.
///
/// With `prefer_rgba8` (see [`prefers_rgba8_canvas`]) `Rgba8Unorm` wins
/// whenever the surface offers it; without it on offer the shared rule
/// still decides.
pub(crate) fn pick_surface_format(
    formats: &[wgpu::TextureFormat],
    prefer_rgba8: bool,
) -> Option<wgpu::TextureFormat> {
    if prefer_rgba8 && formats.contains(&wgpu::TextureFormat::Rgba8Unorm) {
        return Some(wgpu::TextureFormat::Rgba8Unorm);
    }
    agg_gui_wgpu::choose_surface_format(formats)
}

/// Whether a canvas on `backend` in the browser named by `user_agent` should
/// be configured `Rgba8Unorm` rather than the shared rule's `Bgra8Unorm`.
///
/// Firefox's WebGPU canvas presentation leaks one canvas-sized texture of GPU
/// memory for every frame presented on a `bgra8unorm` canvas, freed only
/// when its GPU process exits (Firefox 160, D3D12: 300 frames of a 1280x593
/// canvas kept about 1 GB; the same frames on `rgba8unorm` kept noise-level
/// 46 MB), so a continuously drawing app runs the GPU out of memory within
/// minutes. Channel order is invisible to the app: pipelines are built for
/// the configured format and the readback paths swap only for BGRA. WebGL2
/// canvases and every other browser keep the shared rule, and with it the
/// same pixels as native windows and golden images.
pub(crate) fn prefers_rgba8_canvas(backend: wgpu::Backend, user_agent: &str) -> bool {
    backend == wgpu::Backend::BrowserWebGpu && is_firefox(user_agent)
}

/// Whether `user_agent` is Firefox's Gecko engine (desktop or Android).
/// Chromium and Safari say `like Gecko` but never `Firefox/`; Firefox on iOS
/// is WebKit and says `FxiOS/` instead, so it is (rightly) not Firefox here.
pub(crate) fn is_firefox(user_agent: &str) -> bool {
    user_agent.contains("Firefox/")
}

/// The device limits to request for `backend`, honouring an explicit override.
///
/// On WebGPU the storage-binding and buffer-size limits are the adapter's own,
/// as agg-gui-wgpu's native devices ask for (its `gpu::device_descriptor`): a
/// compute pass over a large canvas needs buffers past wgpu's 128 MiB / 256 MiB
/// defaults, and asking for exactly what the adapter reports cannot fail the
/// request. WebGL2 has no storage buffers, so its downlevel limits stay.
pub(crate) fn device_limits(
    backend: wgpu::Backend,
    adapter_limits: wgpu::Limits,
    override_limits: Option<&wgpu::Limits>,
) -> wgpu::Limits {
    if let Some(l) = override_limits {
        return l.clone();
    }
    if backend == wgpu::Backend::Gl {
        return wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter_limits);
    }
    wgpu::Limits {
        max_storage_buffer_binding_size: adapter_limits.max_storage_buffer_binding_size,
        max_buffer_size: adapter_limits.max_buffer_size,
        ..wgpu::Limits::default().using_resolution(adapter_limits)
    }
}

/// The device features to request: every `required` feature (an error naming
/// the missing ones when the adapter lacks any) plus the `optional` ones the
/// adapter offers. `TIMESTAMP_QUERY` is always optional, as on agg-gui-wgpu's
/// native devices, so a custom render pass can time its own work on the GPU's
/// clock wherever the browser allows it. A browser may round the results
/// (Chrome rounds them to 100 microseconds), so a short pass cannot be timed
/// finely there.
pub(crate) fn device_features(
    adapter: wgpu::Features,
    required: wgpu::Features,
    optional: wgpu::Features,
) -> Result<wgpu::Features, WebShellError> {
    let missing = required.difference(adapter);
    if !missing.is_empty() {
        return Err(WebShellError::MissingFeatures(missing));
    }
    Ok(required | ((optional | wgpu::Features::TIMESTAMP_QUERY) & adapter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_non_srgb_format() {
        use wgpu::TextureFormat as F;
        assert_eq!(
            pick_surface_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm], false),
            Some(F::Bgra8Unorm)
        );
        assert_eq!(
            pick_surface_format(&[F::Rgba8UnormSrgb], false),
            Some(F::Rgba8UnormSrgb)
        );
        assert_eq!(pick_surface_format(&[], false), None);
    }

    #[test]
    fn a_firefox_canvas_draws_in_rgba8_whichever_order_it_is_offered() {
        use wgpu::TextureFormat as F;
        assert_eq!(
            pick_surface_format(&[F::Bgra8Unorm, F::Rgba8Unorm, F::Rgba16Float], true),
            Some(F::Rgba8Unorm)
        );
        assert_eq!(
            pick_surface_format(&[F::Rgba8Unorm, F::Bgra8Unorm, F::Rgba16Float], true),
            Some(F::Rgba8Unorm)
        );
    }

    #[test]
    fn other_canvases_keep_the_shared_rule() {
        use wgpu::TextureFormat as F;
        assert_eq!(
            pick_surface_format(&[F::Rgba8Unorm, F::Bgra8Unorm, F::Rgba16Float], false),
            Some(F::Bgra8Unorm)
        );
        // Preferring rgba8 when the surface lacks it falls back to the shared
        // rule rather than inventing a format.
        assert_eq!(
            pick_surface_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm], true),
            Some(F::Bgra8Unorm)
        );
        assert_eq!(pick_surface_format(&[], true), None);
    }

    const FIREFOX_WINDOWS: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:142.0) Gecko/20100101 Firefox/142.0";
    const FIREFOX_MAC: &str =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:142.0) Gecko/20100101 Firefox/142.0";
    const FIREFOX_LINUX: &str =
        "Mozilla/5.0 (X11; Linux x86_64; rv:142.0) Gecko/20100101 Firefox/142.0";
    const FIREFOX_ANDROID: &str =
        "Mozilla/5.0 (Android 14; Mobile; rv:142.0) Gecko/142.0 Firefox/142.0";
    const CHROME_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
        AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
    const EDGE_WINDOWS: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
        AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";
    const SAFARI_MAC: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
        AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
    const CHROME_IOS: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) \
        AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/141.0.0.0 Mobile/15E148 Safari/604.1";
    const FIREFOX_IOS: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) \
        AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/142.0 Mobile/15E148 Safari/605.1.15";

    #[test]
    fn firefox_is_recognised_on_desktop_and_android() {
        for ua in [FIREFOX_WINDOWS, FIREFOX_MAC, FIREFOX_LINUX, FIREFOX_ANDROID] {
            assert!(is_firefox(ua), "{ua}");
        }
    }

    #[test]
    fn chromium_safari_and_ios_browsers_are_not_firefox() {
        // Firefox on iOS is WebKit underneath, so its canvases are Safari's.
        for ua in [
            CHROME_WINDOWS,
            EDGE_WINDOWS,
            SAFARI_MAC,
            CHROME_IOS,
            FIREFOX_IOS,
            "",
        ] {
            assert!(!is_firefox(ua), "{ua}");
        }
    }

    #[test]
    fn only_firefox_webgpu_canvases_prefer_rgba8() {
        assert!(prefers_rgba8_canvas(
            wgpu::Backend::BrowserWebGpu,
            FIREFOX_WINDOWS
        ));
        // Firefox's WebGL2 canvas is another presentation path; it keeps the
        // shared rule.
        assert!(!prefers_rgba8_canvas(wgpu::Backend::Gl, FIREFOX_WINDOWS));
        assert!(!prefers_rgba8_canvas(
            wgpu::Backend::BrowserWebGpu,
            CHROME_WINDOWS
        ));
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
    fn webgpu_limits_take_the_adapters_buffer_sizes() {
        let adapter = wgpu::Limits {
            max_storage_buffer_binding_size: 2_147_483_644,
            max_buffer_size: 4_294_967_296,
            max_texture_dimension_2d: 16384,
            ..wgpu::Limits::default()
        };
        let got = device_limits(wgpu::Backend::BrowserWebGpu, adapter.clone(), None);
        assert_eq!(got.max_storage_buffer_binding_size, 2_147_483_644);
        assert_eq!(got.max_buffer_size, 4_294_967_296);
        assert_eq!(got.max_texture_dimension_2d, 16384);
        // Everything else stays at the WebGPU defaults, which every WebGPU
        // adapter offers, so the request cannot exceed the adapter.
        assert_eq!(got.max_bind_groups, wgpu::Limits::default().max_bind_groups);
        assert!(got.check_limits(&adapter));
    }

    #[test]
    fn timestamp_queries_are_requested_only_when_offered() {
        use wgpu::Features as F;
        let with = device_features(F::TIMESTAMP_QUERY, F::empty(), F::empty()).unwrap();
        assert_eq!(with, F::TIMESTAMP_QUERY);
        let without = device_features(F::DEPTH_CLIP_CONTROL, F::empty(), F::empty()).unwrap();
        assert_eq!(without, F::empty());
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
        assert_eq!(
            WebShellError::AdapterRejected("Needs float targets.".into()).to_string(),
            "Needs float targets."
        );
    }
}
