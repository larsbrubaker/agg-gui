//! Canvas GPU bring-up for the web shell: instance (with the WebGPU → WebGL2
//! probe), surface, adapter, device, swap-chain configuration; per-frame
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

use wasm_bindgen::JsValue;

use crate::config::{Backend, WebShellConfig};
use crate::dom_math::clamp_to_max_dim;
use crate::error::{device_limits, pick_surface_format, GpuInfo, WebShellError};

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
    adapter_info: wgpu::AdapterInfo,
    lost: Arc<AtomicBool>,
    optional_features: wgpu::Features,
    limits_override: Option<wgpu::Limits>,
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

        let (device, queue, adapter, lost) = request_device(
            &instance,
            &surface,
            &cfg.device_label,
            cfg.optional_features,
            cfg.limits.as_ref(),
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
            adapter_info: adapter.get_info(),
            lost,
            optional_features: cfg.optional_features,
            limits_override: cfg.limits.clone(),
            label: cfg.device_label.clone(),
        })
    }

    pub(crate) fn info(&self) -> GpuInfo {
        GpuInfo {
            device: Arc::clone(&self.device),
            queue: Arc::clone(&self.queue),
            surface_format: self.config.format,
            backend: self.adapter_info.backend,
            adapter_info: self.adapter_info.clone(),
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
            self.optional_features,
            self.limits_override.as_ref(),
        )
        .await;
        match result {
            Ok((device, queue, adapter, lost)) => {
                self.surface.configure(&device, &self.config);
                self.device = Arc::new(device);
                self.queue = Arc::new(queue);
                self.adapter_info = adapter.get_info();
                self.lost = lost;
                Ok(self)
            }
            Err(e) => Err((self, e)),
        }
    }
}

async fn request_device(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'static>,
    label: &str,
    optional_features: wgpu::Features,
    limits_override: Option<&wgpu::Limits>,
) -> Result<(wgpu::Device, wgpu::Queue, wgpu::Adapter, Arc<AtomicBool>), WebShellError> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| WebShellError::RequestAdapter(format!("{e:?}")))?;
    let backend = adapter.get_info().backend;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some(label),
            // Masked so an absent optional feature degrades instead of
            // failing the request.
            required_features: optional_features & adapter.features(),
            required_limits: device_limits(backend, adapter.limits(), limits_override),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| WebShellError::RequestDevice(format!("{e:?}")))?;

    // Uncaptured errors are the difference between "black canvas, no
    // explanation" and a diagnosable bug on a device we can't attach to.
    device.on_uncaptured_error(Arc::new(|e: wgpu::Error| {
        web_sys::console::error_1(&JsValue::from_str(&format!("wgpu uncaptured error: {e}")));
    }));
    let lost = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&lost);
        device.set_device_lost_callback(move |reason, message| {
            if reason != wgpu::DeviceLostReason::Destroyed {
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "agg-gui-web-shell: GPU device lost ({reason:?}): {message}"
                )));
                flag.store(true, Ordering::Relaxed);
            }
        });
    }
    Ok((device, queue, adapter, lost))
}
