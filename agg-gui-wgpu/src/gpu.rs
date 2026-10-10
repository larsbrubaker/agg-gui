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
//! Frame acquisition, and the retry of a failed `Surface::configure` it
//! drives, live in the child module `gpu_acquire.rs`; the pure retry policy
//! is `crate::surface_retry`.
//!
//! The adapter and device requests run inside agg-sharp's start-up budget
//! (`crate::gpu_budget::create_within_budget`, [`GpuConfig::startup_budget`]),
//! and [`Gpu::release_within_budget`] drains the GPU inside its teardown
//! budget before releasing, so neither a hung driver at start-up nor a slow
//! one at close can hold the UI thread forever.
//!
//! The adapter request itself — hardware first, wgpu's software (fallback)
//! adapter only on demand ([`GpuConfig::force_fallback_adapter`]) — and the
//! [`AdapterSummary`] of the adapter chosen live in the child module
//! `gpu_adapter.rs`, shared with `crate::headless`.
//!
//! wasm shells configure their canvas surface through the browser and never
//! block on an adapter request, so this module is native-only.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::gpu_budget::{create_within_budget, BACKGROUND_THREAD_AVAILABLE, GPU_STARTUP_BUDGET};
use crate::surface_retry::{ConfigureRetry, DEFAULT_RETRY_BUDGET};

#[path = "gpu_acquire.rs"]
mod acquire;
pub use acquire::{FrameAcquire, RetryWake, SurfaceError};
#[path = "gpu_adapter.rs"]
mod adapter;
pub(crate) use acquire::watch_device_loss;
pub(crate) use adapter::adapter_options;
pub use adapter::{is_fallback_adapter, AdapterSummary};

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
    /// How long the swap chain may stay continuously unconfigured — every
    /// `Surface::configure` retry failing — before [`Gpu::try_acquire_frame`]
    /// gives up with [`SurfaceError`]. Default 10 s. See
    /// [`GpuConfig::with_surface_retry_budget`].
    ///
    /// Measured from the first failure of the run and judged when a retry
    /// fails. Giving up also needs a minimum number of failed attempts (12,
    /// about 6.5 s of continuous retrying), so a caller that paused attempts
    /// past the budget — agg-gui-shell does while its window is minimized
    /// mid-run —
    /// still gets a real retry run afterwards rather than an error at its
    /// first failed retry.
    pub surface_retry_budget: Duration,
    /// How long [`Gpu::new`] waits for the adapter and device requests before
    /// giving up with [`GpuInitError::StartupTimedOut`]. Default
    /// [`crate::GPU_STARTUP_BUDGET`] (15 s, agg-sharp `GpuStartup`).
    pub startup_budget: Duration,
    /// Demand wgpu's software (fallback) adapter instead of the GPU — WARP
    /// on Windows, lavapipe/llvmpipe on Linux; macOS has none — and run the
    /// same shaders on it. Default `false`. For a host whose user's GPU
    /// driver is broken (MatterCAD's `FORCE_SOFTWARE_RENDERING`); a software
    /// rasterizer costs roughly 100x the frame time, so it is opt-in only.
    /// When the system has no fallback adapter, [`Gpu::new`] returns
    /// [`GpuInitError::NoFallbackAdapter`]. [`Gpu::adapter`] reports what
    /// was chosen.
    pub force_fallback_adapter: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            label: "agg-gui-wgpu",
            copy_src: CopySrc::Never,
            present_mode: wgpu::PresentMode::AutoVsync,
            optional_features: wgpu::Features::empty(),
            surface_retry_budget: DEFAULT_RETRY_BUDGET,
            startup_budget: GPU_STARTUP_BUDGET,
            force_fallback_adapter: false,
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

    /// Override how long a failing swap chain is retried before
    /// [`Gpu::try_acquire_frame`] reports [`SurfaceError`] (default 10 s).
    /// A minimum number of failed attempts is also required, so a very short
    /// budget still gets roughly 6.5 s of actual retrying. `Duration::MAX`
    /// retries forever. See [`GpuConfig::surface_retry_budget`].
    pub fn with_surface_retry_budget(mut self, budget: Duration) -> Self {
        self.surface_retry_budget = budget;
        self
    }

    /// Override how long [`Gpu::new`] waits for its adapter and device
    /// (default 15 s). See [`GpuConfig::startup_budget`].
    pub fn with_startup_budget(mut self, budget: Duration) -> Self {
        self.startup_budget = budget;
        self
    }

    /// Demand wgpu's software (fallback) adapter. See
    /// [`GpuConfig::force_fallback_adapter`].
    pub fn with_force_fallback_adapter(mut self, force: bool) -> Self {
        self.force_fallback_adapter = force;
        self
    }
}

/// Why [`Gpu::new`] could not produce a usable surface.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuInitError {
    CreateSurface(wgpu::CreateSurfaceError),
    RequestAdapter,
    /// [`GpuConfig::force_fallback_adapter`] demanded wgpu's software adapter
    /// and the system has none (macOS never does; Linux without lavapipe).
    /// Its text is written for the host to show the user.
    NoFallbackAdapter,
    RequestDevice,
    /// [`CopySrc::Required`] was asked for and the surface does not offer it.
    CopySrcUnsupported,
    /// The surface reported no supported texture formats — a torn-down or
    /// otherwise unusable surface.
    NoSurfaceFormats,
    /// The surface reported no supported composite alpha modes.
    NoAlphaModes,
    /// The initial `Surface::configure` failed validation (or the device was
    /// lost during it). The first configure creates the swap chain rather
    /// than resizing one, so unlike a later resize this is not a transient
    /// surface state; it is reported instead of retried. Carries wgpu's
    /// error text.
    ConfigureSurface(String),
    /// The adapter or device request did not return inside
    /// [`GpuConfig::startup_budget`] — a driver that hung. The half-built
    /// device is leaked on its own thread (see `crate::gpu_budget`). Its
    /// text is agg-sharp `WebGpuControl`'s start-up error, the message a user
    /// is shown.
    StartupTimedOut {
        /// The budget that expired.
        budget: Duration,
    },
}

impl std::fmt::Display for GpuInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateSurface(e) => write!(f, "create wgpu surface: {e}"),
            Self::RequestAdapter => write!(f, "no suitable wgpu adapter"),
            Self::NoFallbackAdapter => write!(
                f,
                "Software rendering was requested, but this computer has no software graphics \
                 adapter. Start without software rendering to use the GPU."
            ),
            Self::RequestDevice => write!(f, "could not request a wgpu device"),
            Self::CopySrcUnsupported => {
                write!(f, "surface does not support COPY_SRC read-back")
            }
            Self::NoSurfaceFormats => write!(f, "surface reports no supported texture formats"),
            Self::NoAlphaModes => write!(f, "surface reports no supported alpha modes"),
            Self::ConfigureSurface(e) => write!(f, "configure wgpu surface: {e}"),
            Self::StartupTimedOut { budget } => write!(
                f,
                "The GPU device could not be created: the adapter or device request did not \
                 return within {}s.",
                crate::gpu_budget::format_budget_seconds(*budget)
            ),
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

/// What the budgeted half of [`Gpu::new`] hands back.
type RequestedDevice = (
    wgpu::Surface<'static>,
    wgpu::Adapter,
    wgpu::Device,
    wgpu::Queue,
);

/// The adapter and device requests [`Gpu::new`] runs inside its start-up
/// budget. Owns the instance and surface so that a request which never
/// returns keeps them on its own (abandoned) thread.
fn request_device(
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    label: &'static str,
    optional_features: wgpu::Features,
    force_fallback_adapter: bool,
) -> Result<RequestedDevice, GpuInitError> {
    let adapter = pollster::block_on(
        instance.request_adapter(&adapter_options(force_fallback_adapter, Some(&surface))),
    )
    .map_err(|e| {
        // wgpu's text names every backend it tried and why each had no
        // adapter; the error a host shows is plainer, so it goes to the log.
        log::warn!("agg-gui-wgpu: {label} adapter request failed: {e}");
        adapter::adapter_request_error(force_fallback_adapter)
    })?;

    let (device, queue) = pollster::block_on(adapter.request_device(&device_descriptor(
        label,
        optional_features,
        &adapter,
    )))
    .map_err(|_| GpuInitError::RequestDevice)?;
    Ok((surface, adapter, device, queue))
}

/// The device request every agg-gui-wgpu device is made with: a window's
/// ([`Gpu::new`]) and the offscreen one ([`crate::headless::HeadlessGpu`]),
/// so a frame painted headlessly runs under the same features and limits as
/// the app's.
pub(crate) fn device_descriptor<'a>(
    label: &'a str,
    optional_features: wgpu::Features,
    adapter: &wgpu::Adapter,
) -> wgpu::DeviceDescriptor<'a> {
    wgpu::DeviceDescriptor {
        label: Some(label),
        // Optional features are masked against what the adapter actually
        // offers, so asking for an absent one degrades instead of failing
        // `request_device`.
        required_features: optional_features & adapter.features(),
        // The default limits, raised to the adapter's real texture size limit
        // (usually 16384, against the default 8192): a supersampled 3D view on
        // a fullscreen HiDPI window needs textures several times the window's
        // size, and the default would force it down to a softer frame.
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }
}

/// wgpu device + surface bundle for one OS window.
pub struct Gpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    adapter_info: wgpu::AdapterInfo,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    config: wgpu::SurfaceConfiguration,
    device_lost: Arc<std::sync::atomic::AtomicBool>,
    /// Whether the swap chain is configured, and if not, the retry backoff
    /// and the latest error. wgpu drops the swap chain on a failed
    /// configure, and acquiring from an unconfigured surface is itself fatal,
    /// so no frame is acquired until a configure succeeds again. A `Mutex`
    /// (not a `Cell`) because acquisition takes `&self` and `Gpu` must stay
    /// `Sync`; it is never contended.
    retry: Mutex<acquire::RetryState>,
    /// [`GpuConfig::surface_retry_budget`].
    retry_budget: Duration,
}

impl Gpu {
    /// Create the surface, adapter, device and queue for `target` (typically an
    /// `Arc<winit::window::Window>`) and configure the swap chain at
    /// `size` physical pixels, clamped by [`clamp_surface_size`].
    ///
    /// A non-sRGB surface format is preferred so the renderer's colour maths —
    /// which writes linear-space values — isn't gamma-corrected twice by the
    /// surface.
    ///
    /// The adapter and device requests run on their own thread within
    /// [`GpuConfig::startup_budget`]; when it expires this returns
    /// [`GpuInitError::StartupTimedOut`] instead of waiting on the driver.
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
        // Budgeted (agg-sharp `WebGpuControl.InitializeWebGpu` through
        // `GpuStartup`): these two requests are synchronous native calls that
        // have been seen not to return on a loaded software rasterizer. The
        // instance and surface go to the build thread with them, so a request
        // that never returns leaks them there instead of the caller waiting.
        let label = config.label;
        let optional_features = config.optional_features;
        let force_fallback = config.force_fallback_adapter;
        let built = create_within_budget(
            move || request_device(instance, surface, label, optional_features, force_fallback),
            &format!("{label} device"),
            config.startup_budget,
            BACKGROUND_THREAD_AVAILABLE,
            None,
        )?;
        let Some((surface, adapter, device, queue)) = built else {
            return Err(GpuInitError::StartupTimedOut {
                budget: config.startup_budget,
            });
        };

        let adapter_info = adapter.get_info();
        log::info!(
            "agg-gui-wgpu: {label} renders on {}",
            AdapterSummary::from_info(&adapter_info)
        );

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

        // Device loss is reported out-of-band; see `watch_device_loss`.
        let device_lost = acquire::watch_device_loss(&device);

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
        // The first configure creates the swap chain (`CreateSwapChainForHwnd`
        // on DX12), not the `ResizeBuffers` that fails transiently. A
        // validation error here means an unsupported format / alpha mode /
        // size — a real bug — so fail loudly instead of retrying forever
        // behind a blank window.
        acquire::try_configure(&device, &surface, &surface_config, &device_lost)
            .map_err(GpuInitError::ConfigureSurface)?;

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            adapter_info,
            surface,
            surface_format,
            config: surface_config,
            device_lost,
            retry: Mutex::new(acquire::RetryState::new(ConfigureRetry::configured())),
            retry_budget: config.surface_retry_budget,
        })
    }

    /// Has this device been lost since it was created?
    ///
    /// Set from wgpu's device-lost callback (TDR / driver reset / GPU removal
    /// / RDP session change, or `Device::destroy`). A
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

    /// Everything wgpu reports about the adapter this device runs on.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Which adapter this device runs on — name, backend, and whether it is
    /// the software fallback — for a host to report.
    pub fn adapter(&self) -> AdapterSummary {
        AdapterSummary::from_info(&self.adapter_info)
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

    /// Release this device and its surface at window close, waiting at most
    /// `budget` for the GPU to finish what it was given (agg-sharp
    /// `WebGpuControl.DisposeDeviceResources(budgetTheGpuDrain: true)`
    /// through `GpuTeardown`; [`crate::GPU_TEARDOWN_BUDGET`] is its 5 s).
    ///
    /// The drain — `Device::poll` waiting for the queue, which touches no
    /// window — runs on its own thread. If it comes back in time the surface,
    /// device and queue are released here, on the caller's thread, while the
    /// window still exists, and this returns `true`. If not, nothing is
    /// released: the bundle is leaked until the process exits (the surface's
    /// release would wait on the same fence, and race the window's
    /// destruction), and this returns `false`. Other holders of the device or
    /// queue (`WgpuGfxCtx`, an app renderer) can drop theirs afterwards
    /// without waiting: this bundle's references keep both alive.
    ///
    /// A device already lost is released without a drain (wgpu 29 panics
    /// when a lost device is polled), and a drain that fails — including a
    /// loss found while polling, whose panic is caught inside the drain — is
    /// followed by the release too; both return `true`. See
    /// `gpu_budget::release_after_drain` for the whole decision.
    ///
    /// Device-loss recovery should simply drop the old `Gpu` instead: it is
    /// not on a deadline and wants the old device really gone.
    pub fn release_within_budget(self, budget: Duration) -> bool {
        let device = Arc::clone(&self.device);
        let already_lost = self.device_lost();
        crate::gpu_budget::release_after_drain(
            self,
            already_lost,
            move || device.poll(wgpu::PollType::wait_indefinitely()).map(|_| ()),
            "agg-gui-wgpu device",
            budget,
            BACKGROUND_THREAD_AVAILABLE,
            None,
        )
    }

    /// Reconfigure the swap chain for a new physical size. A zero-sized
    /// (minimized) window is ignored — there is no presentable surface then,
    /// and wgpu rejects a zero extent.
    ///
    /// A failed configure (DX12 `ResizeBuffers` rejecting a window Windows
    /// is still settling) does not panic: it is recorded, logged, and retried
    /// by [`Self::try_acquire_frame`] with backoff. A new size is new
    /// information, so it is configured immediately even while an earlier
    /// failure is backing off.
    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let (w, h) = clamp_surface_size(w, h, self.device.limits().max_texture_dimension_2d);
        self.config.width = w;
        self.config.height = h;
        self.configure_and_record(web_time::Instant::now());
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
    fn surface_retry_budget_defaults_to_ten_seconds_and_builds() {
        use std::time::Duration;
        let cfg = super::GpuConfig::new("t");
        assert_eq!(cfg.surface_retry_budget, Duration::from_secs(10));
        let cfg = cfg.with_surface_retry_budget(Duration::from_secs(3));
        assert_eq!(cfg.surface_retry_budget, Duration::from_secs(3));
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
