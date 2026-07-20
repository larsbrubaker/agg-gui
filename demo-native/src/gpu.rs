//! wgpu device/surface wiring for the native demo shell.
//!
//! Split out of `main.rs` (800-line limit) so the surface lifecycle — device
//! init, resize, and per-frame acquire/recover — lives in one focused unit.
//! The event loop in `main.rs` owns a single [`Gpu`] and calls [`acquire_frame`]
//! each paint.  Surface sizes are clamped through [`crate::window_size`] so no
//! configure can exceed the GPU's max texture dimension.

use std::sync::Arc;

use winit::window::Window;

use crate::window_size;

pub struct Gpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    pub surface_format: wgpu::TextureFormat,
    pub config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    pub fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_desc);
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("request adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("demo-native-wgpu"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        // Prefer a non-sRGB format so the existing colour math (which assumes
        // linear-space writes) doesn't get gamma-corrected by the surface.
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);

        // Clamp to the GPU's max texture dimension so an over-large window
        // (or a corrupted restored size that slipped through) can't panic
        // `Surface::configure` with an out-of-range extent.
        let (cfg_w, cfg_h) = window_size::clamp_surface_size(
            size.width,
            size.height,
            device.limits().max_texture_dimension_2d,
        );

        let config = wgpu::SurfaceConfiguration {
            // `RENDER_ATTACHMENT` for the deferred 2-D + bar-grid passes;
            // `COPY_SRC` so `WgpuGfxCtx::read_screenshot` can blit the
            // post-render surface contents to a staging buffer for the
            // capture-pixels path.  The Take-Screenshot button needs this.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: surface_format,
            width: cfg_w,
            height: cfg_h,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface,
            surface_format,
            config,
        }
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        // Same defence-in-depth clamp as `new`: never configure past the
        // device's max texture dimension.
        let (w, h) =
            window_size::clamp_surface_size(w, h, self.device.limits().max_texture_dimension_2d);
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }
}

/// How to handle the result of `Surface::get_current_texture`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SurfaceAcquire {
    /// Texture is usable — render into it.
    Present,
    /// Surface configuration is stale (`Outdated`/`Lost`): reconfigure with
    /// the current config and try once more THIS frame.
    Reconfigure,
    /// Transient (`Timeout`/`Occluded`/`Validation`): skip the frame.
    Skip,
}

/// Decide how to handle a surface-acquire status.  Split out as a pure
/// function so the resize-recovery policy is unit-testable without a live GPU
/// surface (the no-payload variants are constructible in tests).
///
/// The key case is `Outdated`/`Lost`.  These fire right after a window resize
/// reconfigures the swapchain — the previously acquired textures no longer
/// match.  The old code lumped them into the catch-all `None` skip, so the
/// frame was dropped and the freshly-resized surface stayed BLACK until some
/// unrelated event (mouse move, hover) happened to request another redraw.
/// Reconfiguring and retrying in the same frame paints the new swapchain
/// immediately, so the resize lands a visible frame on the first try.
fn surface_acquire_action(status: &wgpu::CurrentSurfaceTexture) -> SurfaceAcquire {
    use wgpu::CurrentSurfaceTexture as T;
    match status {
        T::Success(_) | T::Suboptimal(_) => SurfaceAcquire::Present,
        T::Outdated | T::Lost => SurfaceAcquire::Reconfigure,
        T::Timeout | T::Occluded | T::Validation => SurfaceAcquire::Skip,
    }
}

/// Acquire the next surface texture, recovering from a stale swapchain by
/// reconfiguring and retrying once.  Returns `None` (skip frame) only for
/// genuinely transient failures so the caller never paints into a stale view.
pub fn acquire_frame(gpu: &Gpu) -> Option<wgpu::SurfaceTexture> {
    use wgpu::CurrentSurfaceTexture as T;
    let first = gpu.surface.get_current_texture();
    match surface_acquire_action(&first) {
        SurfaceAcquire::Present => match first {
            T::Success(f) | T::Suboptimal(f) => Some(f),
            _ => None,
        },
        SurfaceAcquire::Skip => None,
        SurfaceAcquire::Reconfigure => {
            // `configure` takes `&self`; the config is unchanged (the resize
            // handler already updated width/height) — we just re-bind it.
            gpu.surface.configure(&gpu.device, &gpu.config);
            match gpu.surface.get_current_texture() {
                T::Success(f) | T::Suboptimal(f) => Some(f),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{surface_acquire_action, SurfaceAcquire};
    use wgpu::CurrentSurfaceTexture as T;

    #[test]
    fn stale_swapchain_reconfigures_instead_of_skipping() {
        // This is the resize-black-screen regression: Outdated/Lost must drive
        // a reconfigure-and-retry, NOT a silent skip.
        assert_eq!(
            surface_acquire_action(&T::Outdated),
            SurfaceAcquire::Reconfigure
        );
        assert_eq!(surface_acquire_action(&T::Lost), SurfaceAcquire::Reconfigure);
    }

    #[test]
    fn transient_failures_skip_the_frame() {
        assert_eq!(surface_acquire_action(&T::Timeout), SurfaceAcquire::Skip);
        assert_eq!(surface_acquire_action(&T::Occluded), SurfaceAcquire::Skip);
        assert_eq!(surface_acquire_action(&T::Validation), SurfaceAcquire::Skip);
    }
}
