//! Canvas GPU bring-up for the web shell: instance (with the WebGPU → WebGL2
//! probe), surface, adapter (and the app's adapter check, before any device
//! exists), device, swap-chain configuration; per-frame
//! surface acquire with recovery; and the device-loss rebuild.
//!
//! The union of three copies it replaces — `demo-wgpu`'s `web_shell`
//! (WebGL2, `using_resolution` limits), `demo-wasm`'s `init_wgpu_async`, and
//! AtomArtist's `init_wgpu` (WebGPU, uncaptured-error logging, optional
//! features). The pure choices (format, limits) live in [`crate::error`] so
//! they are unit tested natively.

// wgpu's web types are `!Send`, yet `WgpuGfxCtx` (shared with native) takes
// `Arc`s. Single-threaded on the browser main thread, so the `Arc` is only a
// shared handle here — same as every previous web shell.
#![allow(clippy::arc_with_non_send_sync)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agg_gui_wgpu::{next_device_generation, UncapturedErrors};
use wasm_bindgen::JsValue;

use crate::adapter_check::{run_adapter_check, AdapterCheck};
use crate::config::{Backend, WebShellConfig};
use crate::dom_math::clamp_to_max_dim;
use crate::error::{device_features, device_limits, pick_surface_format, GpuInfo, WebShellError};

/// wgpu 29's `create_surface` rejects a canvas target unless the instance
/// has *some* display handle (canvases have none). A zero-sized `Web` display
/// handle satisfies the check.
#[derive(Debug)]
struct WebDisplay;

impl wgpu::rwh::HasDisplayHandle for WebDisplay {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        Ok(wgpu::rwh::DisplayHandle::web())
    }
}

/// Everything GPU the shell owns for its canvas.
pub(crate) struct WebGpu {
    instance: wgpu::Instance,
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) config: wgpu::SurfaceConfiguration,
    /// The adapter the device came from; the render context and the app's
    /// [`GpuInfo`] read what a device can't report from it.
    pub(crate) adapter: wgpu::Adapter,
    lost: Arc<AtomicBool>,
    /// The device's uncaptured errors, logged to the console and counted;
    /// the render context hands the count to custom render passes.
    pub(crate) errors: UncapturedErrors,
    /// The device's generation (`agg_gui_wgpu::next_device_generation`), new
    /// for each rebuild; the render context hands it to custom render passes.
    pub(crate) device_generation: u64,
    required_features: wgpu::Features,
    optional_features: wgpu::Features,
    limits_override: Option<wgpu::Limits>,
    adapter_check: Option<AdapterCheck>,
    label: String,
}

/// Outcome of one surface acquire.
pub(crate) enum Acquire {
    Frame(wgpu::SurfaceTexture),
    /// Transient — skip this tick but paint again next tick.
    Retry,
    /// Nothing to paint into (occluded / validation) — wait for a real event.
    Skip,
}

impl WebGpu {
    pub(crate) async fn init(
        canvas: &web_sys::HtmlCanvasElement,
        cfg: &WebShellConfig,
    ) -> Result<Self, WebShellError> {
        let backends = cfg
            .backend
            .wgpu_backends()
            .ok_or(WebShellError::BackendNotCompiled(cfg.backend))?;
        let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(WebDisplay));
        desc.backends = backends;
        let instance = if backends.contains(wgpu::Backends::BROWSER_WEBGPU) {
            // The probe requests a throwaway adapter from `navigator.gpu`
            // *without* touching the canvas — a canvas that has handed out a
            // "webgpu" context can never give a "webgl2" one, so deciding
            // after `create_surface` would make the fallback impossible.
            if cfg.backend == Backend::WebGpu && !wgpu::util::is_browser_webgpu_supported().await {
                return Err(WebShellError::WebGpuUnavailable);
            }
            wgpu::util::new_instance_with_webgpu_detection(desc).await
        } else {
            wgpu::Instance::new(desc)
        };

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| WebShellError::CreateSurface(format!("{e:?}")))?;

        let (device, queue, adapter, lost, errors) = request_device(
            &instance,
            &surface,
            &cfg.device_label,
            cfg.required_features,
            cfg.optional_features,
            cfg.limits.as_ref(),
            cfg.adapter_check.as_ref(),
        )
        .await?;

        let caps = surface.get_capabilities(&adapter);
        let format = pick_surface_format(&caps.formats).ok_or(WebShellError::UnusableSurface)?;
        let alpha_mode = *caps
            .alpha_modes
            .first()
            .ok_or(WebShellError::UnusableSurface)?;
        let (w, h) = clamp_to_max_dim(
            (canvas.width(), canvas.height()),
            device.limits().max_texture_dimension_2d,
        );
        let config = wgpu::SurfaceConfiguration {
            // Browser surfaces advertise only RENDER_ATTACHMENT; asking for
            // COPY_SRC fails validation. Read-back goes through
            // `WebShellConfig::offscreen_scene` instead.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: w,
            height: h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        Ok(Self {
            instance,
            surface,
            device: Arc::new(device),
            queue: Arc::new(queue),
            config,
            adapter,
            lost,
            errors,
            device_generation: next_device_generation(),
            required_features: cfg.required_features,
            optional_features: cfg.optional_features,
            limits_override: cfg.limits.clone(),
            adapter_check: cfg.adapter_check.clone(),
            label: cfg.device_label.clone(),
        })
    }

    pub(crate) fn info(&self) -> GpuInfo {
        let adapter_info = self.adapter.get_info();
        GpuInfo {
            device: Arc::clone(&self.device),
            queue: Arc::clone(&self.queue),
            surface_format: self.config.format,
            backend: adapter_info.backend,
            adapter_info,
            adapter: self.adapter.clone(),
        }
    }

    pub(crate) fn device_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    /// Reconfigure for a new backing size (clamped to the device limit).
    /// Returns the size actually configured.
    pub(crate) fn resize(&mut self, w: u32, h: u32) -> (u32, u32) {
        let (w, h) = clamp_to_max_dim((w, h), self.device.limits().max_texture_dimension_2d);
        if self.config.width != w || self.config.height != h {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&self.device, &self.config);
        }
        (w, h)
    }

    /// Acquire the next surface texture; a stale swap chain
    /// (`Outdated`/`Lost`) is reconfigured and retried once this tick.
    pub(crate) fn acquire(&self) -> Acquire {
        use wgpu::CurrentSurfaceTexture as T;
        match self.surface.get_current_texture() {
            T::Success(f) | T::Suboptimal(f) => Acquire::Frame(f),
            T::Outdated | T::Lost => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    T::Success(f) | T::Suboptimal(f) => Acquire::Frame(f),
                    _ => Acquire::Retry,
                }
            }
            T::Timeout => Acquire::Retry,
            T::Occluded | T::Validation => Acquire::Skip,
        }
    }

    /// Replace a lost device: a fresh adapter + device on the same instance
    /// and surface, swap chain reconfigured at the current size.
    pub(crate) async fn rebuild(mut self) -> Result<Self, (Self, WebShellError)> {
        let result = request_device(
            &self.instance,
            &self.surface,
            &self.label,
            self.required_features,
            self.optional_features,
            self.limits_override.as_ref(),
            self.adapter_check.as_ref(),
        )
        .await;
        match result {
            Ok((device, queue, adapter, lost, errors)) => {
                self.surface.configure(&device, &self.config);
                self.device = Arc::new(device);
                self.queue = Arc::new(queue);
                self.adapter = adapter;
                self.lost = lost;
                self.errors = errors;
                self.device_generation = next_device_generation();
                Ok(self)
            }
            Err(e) => Err((self, e)),
        }
    }
}

/// A new device with its queue, the adapter it came from, its device-lost
/// flag and its uncaptured-error record.
type RequestedDevice = (
    wgpu::Device,
    wgpu::Queue,
    wgpu::Adapter,
    Arc<AtomicBool>,
    UncapturedErrors,
);

async fn request_device(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'static>,
    label: &str,
    required_features: wgpu::Features,
    optional_features: wgpu::Features,
    limits_override: Option<&wgpu::Limits>,
    adapter_check: Option<&AdapterCheck>,
) -> Result<RequestedDevice, WebShellError> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| WebShellError::RequestAdapter(format!("{e:?}")))?;
    // The app's own refusal, before its required features: its message says
    // what this browser's graphics lack in the app's words.
    run_adapter_check(adapter_check, &adapter)?;
    let backend = adapter.get_info().backend;
    let features = device_features(adapter.features(), required_features, optional_features)?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some(label),
            // Required features were checked above; optional ones are masked
            // so an absent one degrades instead of failing the request.
            required_features: features,
            required_limits: device_limits(backend, adapter.limits(), limits_override),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| WebShellError::RequestDevice(format!("{e:?}")))?;

    // Uncaptured errors are the difference between "black canvas, no
    // explanation" and a diagnosable bug on a device we can't attach to.
    // Counted too: each one cost a frame, which a custom render pass that
    // keeps its picture between frames needs to know.
    let errors = UncapturedErrors::install_reporting(&device, |message| {
        web_sys::console::error_1(&JsValue::from_str(&format!(
            "wgpu uncaptured error: {message}"
        )));
    });
    let lost = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&lost);
        device.set_device_lost_callback(move |reason, message| {
            if reason != wgpu::DeviceLostReason::Destroyed {
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "agg-gui-web-shell: GPU device lost ({reason:?}): {message}"
                )));
                flag.store(true, Ordering::Relaxed);
                // The tick it wakes starts the rebuild.
                super::schedule::wake();
            }
        });
    }
    Ok((device, queue, adapter, lost, errors))
}
