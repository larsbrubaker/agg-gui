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
#[non_exhaustive]
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
///
/// Build one with [`GpuConfig::new`] (or [`Default`]) and the `with_*`
/// setters rather than a struct literal — the struct is `#[non_exhaustive]`
/// so new knobs can be added without a breaking release:
///
/// ```no_run
/// use agg_gui_wgpu::{CopySrc, GpuConfig};
/// let cfg = GpuConfig::new("my-app").with_copy_src(CopySrc::IfSupported);
/// ```
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct GpuConfig {
    /// `wgpu::DeviceDescriptor` label — shows up in backend validation
    /// messages and GPU captures, so each shell names its own.
    pub label: &'static str,
    /// Surface read-back requirement, see [`CopySrc`].
    pub copy_src: CopySrc,
    /// Present mode for the swap chain. `AutoVsync` for normal windows.
    pub present_mode: wgpu::PresentMode,
    /// Device features requested **when the adapter offers them** — the set is
    /// masked against `adapter.features()` before `request_device`, so an
    /// adapter that lacks one still yields a device (the app degrades instead
    /// of failing to start). For an app renderer that can use, e.g.,
    /// `FLOAT32_BLENDABLE` when present and fall back when not.
    pub optional_features: wgpu::Features,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            label: "agg-gui-wgpu",
            copy_src: CopySrc::Never,
            present_mode: wgpu::PresentMode::AutoVsync,
            optional_features: wgpu::Features::empty(),
        }
    }
}

impl GpuConfig {
    /// Default configuration under a shell-specific device label.
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

    /// Override the swap-chain present mode (default `AutoVsync`).
    pub fn with_present_mode(mut self, present_mode: wgpu::PresentMode) -> Self {
        self.present_mode = present_mode;
        self
    }

    /// Request these device features when — and only when — the adapter
    /// offers them. See [`GpuConfig::optional_features`].
    pub fn with_optional_features(mut self, features: wgpu::Features) -> Self {
        self.optional_features = features;
        self
    }
}

/// Why [`Gpu::new`] could not produce a usable surface.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuInitError {
    CreateSurface(wgpu::CreateSurfaceError),
    RequestAdapter,
    RequestDevice,
    /// [`CopySrc::Required`] was asked for and the surface does not offer it.
    CopySrcUnsupported,
    /// The surface reported no supported texture formats — a torn-down or
    /// otherwise unusable surface.
    NoSurfaceFormats,
    /// The surface reported no supported composite alpha modes.
    NoAlphaModes,
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
            Self::NoSurfaceFormats => write!(f, "surface reports no supported texture formats"),
            Self::NoAlphaModes => write!(f, "surface reports no supported alpha modes"),
        }
    }
}

impl std::error::Error for GpuInitError {}

/// Pick the surface format to configure the swap chain with.
///
/// A non-sRGB format is preferred so the renderer's linear-space colour maths
/// isn't gamma-corrected a second time by the surface; when the surface only
/// offers sRGB formats we take its own first preference. Pure so the choice is
/// testable without a live surface.
fn pick_surface_format(
    formats: &[wgpu::TextureFormat],
) -> Result<wgpu::TextureFormat, GpuInitError> {
    formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .or_else(|| formats.first().copied())
        .ok_or(GpuInitError::NoSurfaceFormats)
}

/// Pick the composite alpha mode — the surface's first preference.
fn pick_alpha_mode(
    modes: &[wgpu::CompositeAlphaMode],
) -> Result<wgpu::CompositeAlphaMode, GpuInitError> {
    modes.first().copied().ok_or(GpuInitError::NoAlphaModes)
}

/// Pick the present mode to configure the swap chain with.
///
/// `wgpu` resolves the `Auto*` modes itself against whatever the surface
/// supports, so they are always safe to request. An explicit mode that the
/// surface does not list (`Mailbox` on a driver that lacks it, `Immediate`
/// under a compositor that forces vsync) is a validation error, so it falls
/// back to `Fifo` — the one mode the spec guarantees every surface supports.
///
/// Pure so the fallback is testable without a live surface.
pub fn pick_present_mode(
    supported: &[wgpu::PresentMode],
    requested: wgpu::PresentMode,
) -> wgpu::PresentMode {
    use wgpu::PresentMode as P;
    match requested {
        P::AutoVsync | P::AutoNoVsync => requested,
        explicit if supported.contains(&explicit) => explicit,
        _ => P::Fifo,
    }
}

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
    device_lost: Arc<std::sync::atomic::AtomicBool>,
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
            // Optional features are masked against what the adapter actually
            // offers, so asking for an absent one degrades instead of failing
            // `request_device`.
            required_features: config.optional_features & adapter.features(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|_| GpuInitError::RequestDevice)?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = pick_surface_format(&caps.formats)?;
        let alpha_mode = pick_alpha_mode(&caps.alpha_modes)?;

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

        // Device loss (TDR, driver update, GPU reset, RDP session change) is
        // reported out-of-band: nothing in the per-frame API returns an error,
        // so a shell that does not watch this flag silently renders nothing
        // forever. `Destroyed` is our own teardown, not a fault.
        let device_lost = Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let flag = Arc::clone(&device_lost);
            device.set_device_lost_callback(move |reason, _message| {
                if reason != wgpu::DeviceLostReason::Destroyed {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }

        let (cfg_w, cfg_h) =
            clamp_surface_size(size.0, size.1, device.limits().max_texture_dimension_2d);
        let surface_config = wgpu::SurfaceConfiguration {
            usage,
            format: surface_format,
            width: cfg_w,
            height: cfg_h,
            present_mode: pick_present_mode(&caps.present_modes, config.present_mode),
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface,
            surface_format,
            config: surface_config,
            device_lost,
        })
    }

    /// Has this device been lost since it was created?
    ///
    /// Set from wgpu's device-lost callback (TDR / driver reset / GPU removal
    /// / RDP session change); our own `Device::destroy` is not counted. A
    /// lost device cannot be revived — every resource created from it is dead
    /// too — so the only recovery is to build a fresh [`Gpu`] for the same
    /// window, rebuild the renderer on the new device, and drop any GPU
    /// resources the app cached. Shells should poll this once per frame.
    ///
    /// The C# port polls the same flag at the top of a frame and rebuilds from
    /// it in `PlatformWin32/win32/WebGpuControl.cs::TryRecoverDevice`
    /// (agg-sharp), which is the closest thing to a reference implementation of
    /// the recovery this flag is meant to drive.
    pub fn device_lost(&self) -> bool {
        self.device_lost.load(std::sync::atomic::Ordering::Relaxed)
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
#[non_exhaustive]
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
    use super::{
        clamp_surface_size, pick_alpha_mode, pick_present_mode, pick_surface_format,
        surface_acquire_action, GpuInitError, SurfaceAcquire,
    };
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
    fn empty_capability_lists_are_an_error_not_a_panic() {
        // A surface that reports no formats / no alpha modes is a broken or
        // torn-down surface (headless RDP session, adapter lost mid-init).
        // Indexing `[0]` there took the whole app down; callers get an error.
        assert!(matches!(
            pick_surface_format(&[]),
            Err(GpuInitError::NoSurfaceFormats)
        ));
        assert!(matches!(
            pick_alpha_mode(&[]),
            Err(GpuInitError::NoAlphaModes)
        ));
    }

    #[test]
    fn non_srgb_format_is_preferred_and_first_is_the_fallback() {
        use wgpu::TextureFormat as F;
        // The renderer writes linear-space colour, so an sRGB surface would
        // gamma-correct it twice — prefer any non-sRGB format on offer.
        assert_eq!(
            pick_surface_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm]).unwrap(),
            F::Bgra8Unorm
        );
        // All-sRGB surface: fall back to the surface's own preference (first).
        assert_eq!(
            pick_surface_format(&[F::Bgra8UnormSrgb, F::Rgba8UnormSrgb]).unwrap(),
            F::Bgra8UnormSrgb
        );
    }

    #[test]
    fn alpha_mode_takes_the_surface_preference() {
        use wgpu::CompositeAlphaMode as A;
        assert_eq!(
            pick_alpha_mode(&[A::Opaque, A::PreMultiplied]).unwrap(),
            A::Opaque
        );
    }

    #[test]
    fn unsupported_present_mode_falls_back_to_fifo() {
        use wgpu::PresentMode as P;
        // Only Fifo on offer (the guaranteed-everywhere mode): an explicit
        // Mailbox/Immediate request would be a validation error.
        assert_eq!(pick_present_mode(&[P::Fifo], P::Mailbox), P::Fifo);
        assert_eq!(pick_present_mode(&[P::Fifo], P::Immediate), P::Fifo);
        // Supported explicit modes pass through.
        assert_eq!(
            pick_present_mode(&[P::Fifo, P::Mailbox], P::Mailbox),
            P::Mailbox
        );
        // wgpu resolves the Auto modes itself, so they are never rewritten —
        // they do not appear in `caps.present_modes`.
        assert_eq!(pick_present_mode(&[P::Fifo], P::AutoVsync), P::AutoVsync);
        assert_eq!(
            pick_present_mode(&[P::Fifo], P::AutoNoVsync),
            P::AutoNoVsync
        );
        // A surface that reports nothing at all still yields a legal mode.
        assert_eq!(pick_present_mode(&[], P::Immediate), P::Fifo);
    }

    #[test]
    fn optional_features_default_empty_and_build() {
        let cfg = super::GpuConfig::new("t");
        assert_eq!(cfg.optional_features, wgpu::Features::empty());
        let cfg = cfg.with_optional_features(wgpu::Features::FLOAT32_BLENDABLE);
        assert_eq!(cfg.optional_features, wgpu::Features::FLOAT32_BLENDABLE);
        // The request mask: an adapter without the feature yields an empty
        // request rather than a failed `request_device`.
        assert_eq!(
            cfg.optional_features & wgpu::Features::empty(),
            wgpu::Features::empty()
        );
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
