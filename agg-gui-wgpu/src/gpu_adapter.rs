//! Adapter selection for every agg-gui-wgpu device: the window's
//! ([`Gpu::new`](super::Gpu::new)) and the offscreen one
//! ([`crate::headless::HeadlessGpu`]), and the report of which adapter a
//! device ended up on.
//!
//! Pulled in via `#[path]` as the `adapter` child module of `gpu.rs`, like
//! `gpu_acquire.rs`, so the two device paths share one adapter request and
//! `gpu.rs` stays well under the line limit.
//!
//! The one choice a caller makes here is whether to demand wgpu's software
//! (fallback) adapter — WARP on Windows (DX12), lavapipe/llvmpipe on Linux
//! (Vulkan); Metal offers none. That is agg-sharp's `UseSoftwareAdapter`
//! (`WebGpuControl`, `MacWebGpuLayer`, `X11WebGpuLayer`), which MatterCAD's
//! `FORCE_SOFTWARE_RENDERING` switch sets for users whose GPU driver is
//! broken. It is opt-in only: a software rasterizer costs roughly 100x the
//! frame time, so it must never be picked by accident.

/// The adapter request every agg-gui-wgpu device is made with:
/// high-performance hardware first, the software (fallback) adapter only when
/// `force_fallback_adapter` demands it (agg-sharp
/// `WebGpuRenderDevice.BuildAdapterOptions`). `compatible_surface` is the
/// window the adapter must be able to present to, `None` offscreen.
///
/// wgpu honours `force_fallback_adapter` by keeping only adapters whose
/// device type is [`wgpu::DeviceType::Cpu`]; when none is left the request
/// fails, and [`adapter_request_error`] names that failure.
pub(crate) fn adapter_options<'a, 'b>(
    force_fallback_adapter: bool,
    compatible_surface: Option<&'a wgpu::Surface<'b>>,
) -> wgpu::RequestAdapterOptions<'a, 'b> {
    wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface,
        force_fallback_adapter,
    }
}

/// The [`GpuInitError`](super::GpuInitError) for a failed adapter request:
/// [`NoFallbackAdapter`](super::GpuInitError::NoFallbackAdapter) when the
/// software adapter was demanded (the host shows its text), otherwise
/// [`RequestAdapter`](super::GpuInitError::RequestAdapter).
pub(crate) fn adapter_request_error(force_fallback_adapter: bool) -> super::GpuInitError {
    if force_fallback_adapter {
        super::GpuInitError::NoFallbackAdapter
    } else {
        super::GpuInitError::RequestAdapter
    }
}

/// Is `info` a software rasterizer? wgpu's own test for a fallback adapter
/// (and agg-sharp `WebGpuRenderDevice.IsFallbackAdapter`, `adapterType ==
/// CPU`): WARP reports `DXGI_ADAPTER_FLAG_SOFTWARE`, lavapipe
/// `VK_PHYSICAL_DEVICE_TYPE_CPU`.
pub fn is_fallback_adapter(info: &wgpu::AdapterInfo) -> bool {
    info.device_type == wgpu::DeviceType::Cpu
}

/// Which adapter a device runs on, in the terms a host reports it: agg-sharp
/// `WebGpuRenderDevice`'s `AdapterName`, `AdapterBackend` and
/// `IsFallbackAdapter`. [`Gpu::adapter`](super::Gpu::adapter) and
/// [`HeadlessGpu::adapter`](crate::headless::HeadlessGpu::adapter) build it;
/// the full `wgpu::AdapterInfo` stays available beside it.
///
/// `Display` is agg-sharp's render status line (`"{backend} {name}"`, e.g.
/// `Metal Apple M2`), with `(software fallback)` appended on a fallback
/// adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdapterSummary {
    /// The adapter's device name, e.g. `Microsoft Basic Render Driver` (WARP)
    /// or `llvmpipe (LLVM 15.0.7, 256 bits)` (lavapipe).
    pub name: String,
    /// The wgpu backend the adapter was found on: Vulkan, Metal or DX12.
    pub backend: wgpu::Backend,
    /// Whether it is a software rasterizer — see [`is_fallback_adapter`].
    pub is_fallback: bool,
}

impl AdapterSummary {
    pub fn from_info(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            backend: info.backend,
            is_fallback: is_fallback_adapter(info),
        }
    }
}

impl From<&wgpu::AdapterInfo> for AdapterSummary {
    fn from(info: &wgpu::AdapterInfo) -> Self {
        Self::from_info(info)
    }
}

impl std::fmt::Display for AdapterSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Debug` gives the capitalised backend name (`Metal`, `Vulkan`,
        // `Dx12`) agg-sharp's `WGPUBackendType` prints; wgpu's `Display` is
        // lower case.
        write!(f, "{:?} {}", self.backend, self.name)?;
        if self.is_fallback {
            write!(f, " (software fallback)")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GpuConfig, GpuInitError};

    fn info(
        name: &str,
        backend: wgpu::Backend,
        device_type: wgpu::DeviceType,
    ) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: name.to_owned(),
            vendor: 0,
            device: 0,
            device_type,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend,
            subgroup_min_size: 4,
            subgroup_max_size: 128,
            transient_saves_memory: false,
        }
    }

    #[test]
    fn hardware_is_the_default_and_the_fallback_is_opt_in() {
        // agg-sharp's `UseSoftwareAdapter` defaults to false: a window renders
        // on the GPU unless the host explicitly asks otherwise.
        let cfg = GpuConfig::new("t");
        assert!(!cfg.force_fallback_adapter);
        assert!(cfg.with_force_fallback_adapter(true).force_fallback_adapter);
        assert!(
            !cfg.with_force_fallback_adapter(true)
                .with_force_fallback_adapter(false)
                .force_fallback_adapter
        );
    }

    #[test]
    fn the_option_reaches_the_adapter_request() {
        let forced = adapter_options(true, None);
        assert!(forced.force_fallback_adapter);
        // Everything else is the hardware request's.
        assert_eq!(
            forced.power_preference,
            wgpu::PowerPreference::HighPerformance
        );
        assert!(forced.compatible_surface.is_none());

        let hardware = adapter_options(false, None);
        assert!(!hardware.force_fallback_adapter);
        assert_eq!(
            hardware.power_preference,
            wgpu::PowerPreference::HighPerformance
        );
    }

    #[test]
    fn a_failed_fallback_request_is_its_own_error_with_a_message_a_host_can_show() {
        assert!(matches!(
            adapter_request_error(true),
            GpuInitError::NoFallbackAdapter
        ));
        assert!(matches!(
            adapter_request_error(false),
            GpuInitError::RequestAdapter
        ));
        assert_eq!(
            GpuInitError::NoFallbackAdapter.to_string(),
            "Software rendering was requested, but this computer has no software graphics \
             adapter. Start without software rendering to use the GPU."
        );
    }

    #[test]
    fn a_cpu_adapter_is_the_fallback_and_every_other_type_is_not() {
        use wgpu::DeviceType as T;
        let warp = info("Microsoft Basic Render Driver", wgpu::Backend::Dx12, T::Cpu);
        assert!(is_fallback_adapter(&warp));
        for device_type in [T::DiscreteGpu, T::IntegratedGpu, T::VirtualGpu, T::Other] {
            assert!(!is_fallback_adapter(&info(
                "gpu",
                wgpu::Backend::Vulkan,
                device_type
            )));
        }
    }

    #[test]
    fn the_summary_reports_name_backend_and_fallback() {
        let lavapipe = info(
            "llvmpipe (LLVM 15.0.7, 256 bits)",
            wgpu::Backend::Vulkan,
            wgpu::DeviceType::Cpu,
        );
        let summary = AdapterSummary::from(&lavapipe);
        assert_eq!(summary.name, "llvmpipe (LLVM 15.0.7, 256 bits)");
        assert_eq!(summary.backend, wgpu::Backend::Vulkan);
        assert!(summary.is_fallback);
        assert_eq!(
            summary.to_string(),
            "Vulkan llvmpipe (LLVM 15.0.7, 256 bits) (software fallback)"
        );

        let metal = info(
            "Apple M2",
            wgpu::Backend::Metal,
            wgpu::DeviceType::IntegratedGpu,
        );
        let summary = AdapterSummary::from_info(&metal);
        assert!(!summary.is_fallback);
        assert_eq!(summary.to_string(), "Metal Apple M2");
    }
}
