//! wgpu device + surface bundle for one OS window, and the surface-acquire
//! recovery policy that goes with it.
//!
//! Every native shell needs the same thing: an instance, an adapter, a device
//! and queue, a non-sRGB surface format, a clamped surface configuration, and a
//! per-frame `get_current_texture` that recovers from a stale swapchain instead
//! of painting nothing. That was hand-rolled twice (`demo-wgpu`'s
//! `native_shell` and `demo-native`'s `gpu`), with the two copies disagreeing
//! about the `Timeout` case; this module is the single implementation both now
//! use.
//!
//! wasm shells configure their canvas surface through the browser and never
//! block on an adapter request, so this module is native-only.

use std::sync::Arc;

/// How badly the caller needs `COPY_SRC` on the surface texture.
///
/// `COPY_SRC` is what lets [`crate::WgpuGfxCtx::read_screenshot`] blit the
/// rendered surface into a staging buffer. Not every surface supports it, so
/// the caller states whether a screenshot is optional or the point of the run.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CopySrc {
    /// Never request it — the app has no read-back path.
    #[default]
    Never,
    /// Request it when the surface supports it; carry on quietly if not.
    /// The read-back path degrades to returning no pixels.
    IfSupported,
    /// Fail surface creation when the surface cannot provide it — for a
    /// headless capture run, where a missing screenshot is the whole failure.
    Required,
}

/// Everything [`Gpu::new`] needs beyond the window handle.
#[derive(Clone, Copy, Debug)]
pub struct GpuConfig {
    /// `wgpu::DeviceDescriptor` label — shows up in backend validation
    /// messages and GPU captures, so each shell names its own.
    pub label: &'static str,
    /// Surface read-back requirement, see [`CopySrc`].
    pub copy_src: CopySrc,
    /// Present mode for the swap chain. `AutoVsync` for normal windows.
    pub present_mode: wgpu::PresentMode,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            label: "agg-gui-wgpu",
            copy_src: CopySrc::Never,
            present_mode: wgpu::PresentMode::AutoVsync,
        }
    }
}

impl GpuConfig {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            ..Self::default()
        }
    }

    pub fn with_copy_src(mut self, copy_src: CopySrc) -> Self {
        self.copy_src = copy_src;
        self
    }
}

/// Why [`Gpu::new`] could not produce a usable surface.
#[derive(Debug)]
pub enum GpuInitError {
    CreateSurface(wgpu::CreateSurfaceError),
    RequestAdapter,
    RequestDevice,
    /// [`CopySrc::Required`] was asked for and the surface does not offer it.
    CopySrcUnsupported,
}

impl std::fmt::Display for GpuInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateSurface(e) => write!(f, "create wgpu surface: {e}"),
            Self::RequestAdapter => write!(f, "no suitable wgpu adapter"),
            Self::RequestDevice => write!(f, "could not request a wgpu device"),
            Self::CopySrcUnsupported => {
                write!(f, "surface does not support COPY_SRC read-back")
            }
        }
    }
}

impl std::error::Error for GpuInitError {}

/// Clamp a surface configuration size to `[1, max_dim]` on both axes.
///
/// `max_dim` is the device's `max_texture_dimension_2d`. Applied on every
/// `Surface::configure` so a stray oversized request — an over-large window, or
/// a corrupted restored window size that slipped through — degrades to the GPU
/// limit instead of panicking inside wgpu validation.
pub fn clamp_surface_size(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
    let max_dim = max_dim.max(1);
    (w.clamp(1, max_dim), h.clamp(1, max_dim))
}

/// wgpu device + surface bundle for one OS window.
pub struct Gpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: wgpu::SurfaceConfiguration,
}

impl Gpu {
    /// Create the surface, adapter, device and queue for `target` (typically an
    /// `Arc<winit::window::Window>`) and configure the swap chain at
    /// `size` physical pixels, clamped by [`clamp_surface_size`].
    ///
    /// A non-sRGB surface format is preferred so the renderer's colour maths —
    /// which writes linear-space values — isn't gamma-corrected twice by the
    /// surface.
    pub fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        size: (u32, u32),
        config: GpuConfig,
    ) -> Result<Self, GpuInitError> {
        let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_desc);
        let surface = instance
            .create_surface(target)
            .map_err(GpuInitError::CreateSurface)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .map_err(|_| GpuInitError::RequestAdapter)?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some(config.label),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|_| GpuInitError::RequestDevice)?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let has_copy_src = caps.usages.contains(wgpu::TextureUsages::COPY_SRC);
        match config.copy_src {
            CopySrc::Never => {}
            CopySrc::IfSupported => {
                if has_copy_src {
                    usage |= wgpu::TextureUsages::COPY_SRC;
                }
            }
            CopySrc::Required => {
                if !has_copy_src {
                    return Err(GpuInitError::CopySrcUnsupported);
                }
                usage |= wgpu::TextureUsages::COPY_SRC;
            }
        }

        let (cfg_w, cfg_h) =
            clamp_surface_size(size.0, size.1, device.limits().max_texture_dimension_2d);
        let surface_config = wgpu::SurfaceConfiguration {
            usage,
            format: surface_format,
            width: cfg_w,
            height: cfg_h,
            present_mode: config.present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface,
            surface_format,
            config: surface_config,
        })
    }

    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    pub fn surface(&self) -> &wgpu::Surface<'static> {
        &self.surface
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    /// The live swap-chain configuration — `width` / `height` are the clamped
    /// physical pixel size the shell should hand to layout and `WgpuGfxCtx`.
    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    /// Reconfigure the swap chain for a new physical size. A zero-sized
    /// (minimized) window is ignored — there is no presentable surface then,
    /// and wgpu rejects a zero extent.
    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let (w, h) = clamp_surface_size(w, h, self.device.limits().max_texture_dimension_2d);
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
    }

    /// Acquire the next surface texture, recovering from a stale swapchain by
    /// reconfiguring and retrying once.
    ///
    /// Returns `None` when the frame must be skipped. `request_redraw` is
    /// invoked for the skip cases that can still recover on their own, so a
    /// reactive event loop (`ControlFlow::Wait`) wakes up to try again instead
    /// of idling forever; pass a closure that calls `Window::request_redraw`.
    pub fn acquire_frame(&self, request_redraw: impl Fn()) -> Option<wgpu::SurfaceTexture> {
        use wgpu::CurrentSurfaceTexture as T;
        let first = self.surface.get_current_texture();
        match surface_acquire_action(&first) {
            SurfaceAcquire::Present => match first {
                T::Success(f) | T::Suboptimal(f) => Some(f),
                _ => None,
            },
            SurfaceAcquire::Skip => None,
            SurfaceAcquire::SkipAndRetry => {
                request_redraw();
                None
            }
            SurfaceAcquire::Reconfigure => {
                // `configure` takes `&self`; the config already carries the
                // current (nonzero, `resize`-clamped) size, so we just re-bind.
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    T::Success(f) | T::Suboptimal(f) => Some(f),
                    _ => {
                        // Still nothing after the reconfigure: come back next
                        // frame rather than sitting in `ControlFlow::Wait`.
                        request_redraw();
                        None
                    }
                }
            }
        }
    }
}

/// How to handle the result of `Surface::get_current_texture`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfaceAcquire {
    /// Texture is usable — render into it.
    Present,
    /// The swapchain is stale or gone (`Outdated`/`Lost`): reconfigure the
    /// surface and try once more THIS frame.
    Reconfigure,
    /// Transient (`Timeout`): skip the frame, but ask for another one.
    SkipAndRetry,
    /// Skip the frame with no follow-up (`Occluded`/`Validation`): the window
    /// is not visible / the app must fix the validation error, and a
    /// self-requested redraw would just burn the CPU.
    Skip,
}

/// Decide how to handle a surface-acquire status. A pure function so the
/// recovery policy is unit-testable without a live GPU surface (the
/// no-payload variants are constructible in tests).
///
/// `Outdated`/`Lost` fire right after a window resize reconfigures the
/// swapchain, and after a GPU driver reset (TDR), a display-mode change, or an
/// RDP reconnect. Treating them as a plain skip leaves a reactive shell
/// (`ControlFlow::Wait` whenever `wants_draw()` is false) frozen or black until
/// some unrelated event requests another redraw — the resize-black-screen
/// regression. wgpu documents both as "reconfigure the surface and try again".
pub fn surface_acquire_action(status: &wgpu::CurrentSurfaceTexture) -> SurfaceAcquire {
    use wgpu::CurrentSurfaceTexture as T;
    match status {
        T::Success(_) | T::Suboptimal(_) => SurfaceAcquire::Present,
        T::Outdated | T::Lost => SurfaceAcquire::Reconfigure,
        T::Timeout => SurfaceAcquire::SkipAndRetry,
        T::Occluded | T::Validation => SurfaceAcquire::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_surface_size, surface_acquire_action, SurfaceAcquire};
    use wgpu::CurrentSurfaceTexture as T;

    #[test]
    fn stale_swapchain_reconfigures_instead_of_skipping() {
        // The resize-black-screen regression, and the frozen-window case after
        // a driver reset (TDR) / display-mode change / RDP reconnect: both must
        // drive a reconfigure-and-retry, NOT a silent skip.
        assert_eq!(
            surface_acquire_action(&T::Outdated),
            SurfaceAcquire::Reconfigure
        );
        assert_eq!(
            surface_acquire_action(&T::Lost),
            SurfaceAcquire::Reconfigure
        );
    }

    #[test]
    fn timeout_skips_the_frame_but_asks_for_another() {
        // Timeout is transient: the next acquire usually succeeds, so a
        // reactive loop has to be woken back up or it waits forever.
        assert_eq!(
            surface_acquire_action(&T::Timeout),
            SurfaceAcquire::SkipAndRetry
        );
    }

    #[test]
    fn occluded_and_validation_skip_without_self_requested_redraw() {
        assert_eq!(surface_acquire_action(&T::Occluded), SurfaceAcquire::Skip);
        assert_eq!(surface_acquire_action(&T::Validation), SurfaceAcquire::Skip);
    }

    #[test]
    fn surface_size_clamped_to_max_dim() {
        assert_eq!(clamp_surface_size(10224, 5925, 8192), (8192, 5925));
        assert_eq!(clamp_surface_size(0, 0, 8192), (1, 1));
        assert_eq!(clamp_surface_size(1280, 720, 8192), (1280, 720));
        // Degenerate zero limit still yields a valid 1x1.
        assert_eq!(clamp_surface_size(100, 100, 0), (1, 1));
    }
}
