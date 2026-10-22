//! Offscreen GPU rendering with no window: the device, target, frame and
//! read-back a test harness paints a whole widget tree with, through the same
//! [`WgpuGfxCtx`] pipeline the platform shells drive.
//!
//! - [`HeadlessGpu::shared`] — one device and queue for the whole process,
//!   requested the way [`crate::Gpu::new`] requests a window's (same backends,
//!   features and limits, same default start-up budget), but with no surface
//!   and with [`GpuMemoryHints::MemoryUsage`] by default.
//!   [`HeadlessGpu::shared_with_options`] takes [`HeadlessOptions`]: wgpu's
//!   software adapter where the platform has one, a longer start-up budget
//!   for a loaded machine, other memory hints. The shared device is built
//!   through `crate::gpu_shared_startup`, so one that arrives after a
//!   caller's budget still reaches later callers. [`HeadlessGpu::create`]
//!   builds a device the caller owns, for the rare test binary that has to
//!   end its device's life (device-loss recovery).
//! - [`HeadlessTarget`] — an offscreen texture of a given device-pixel size in
//!   [`HEADLESS_FORMAT`], with RGBA8 read-back ([`HeadlessTarget::read_rgba`],
//!   top row first, unpadded) and an `agg_gui::Framebuffer` copy (bottom row
//!   first, as the software renderer lays its pixels out).
//! - [`HeadlessFrame`] — a [`WgpuGfxCtx`] bound to a target that runs a
//!   shell's frame (`set_surface_texture`, `begin_frame`, paint, `end_frame`,
//!   release) around a paint closure or an [`App`](agg_gui::App). Custom
//!   render passes ([`crate::WgpuCustomRender`]) queued during paint run
//!   inside it as they do on a window. Frames are paced as a window's are:
//!   at most two on the GPU at once (see [`HeadlessFrame::render`]).
//!
//! Every wait on the GPU here — the frame pacing and the read-back — is
//! bounded ([`crate::GPU_WAIT_BUDGET`] by default, see
//! [`HeadlessTarget::with_wait_budget`]): a GPU that hangs or loses its
//! device fails the wait with [`HeadlessError::GpuTimeout`] or
//! [`HeadlessError::DeviceLost`] (naming wgpu's reason) instead of parking
//! the caller for good (`crate::gpu_wait` says why and how).
//!
//! The read-back copy is `screenshot_readback::read_texture_rgba_bounded`,
//! the one [`WgpuGfxCtx::read_screenshot`] uses. Native only: the device
//! request blocks, which a browser cannot.

use std::sync::Arc;
use std::time::Duration;

use agg_gui::color::Color;
use agg_gui::{App, Framebuffer};

use crate::gpu_budget::{
    create_within_budget, report_to, spawn_thread, BACKGROUND_THREAD_AVAILABLE, GPU_STARTUP_BUDGET,
};
use crate::gpu_shared_startup::SharedStartup;
use crate::gpu_wait::{watch_device_loss_reason, DeviceLoss};
use crate::screenshot_readback::ReadbackError;
use crate::{GpuMemoryHints, WgpuGfxCtx, GPU_WAIT_BUDGET};

#[path = "headless_error.rs"]
mod headless_error;
pub use headless_error::HeadlessError;

/// The format of a [`HeadlessTarget`]: the frame format the native shells get
/// on every primary backend (`Gpu::new` takes the surface's first non-sRGB
/// format, `Bgra8Unorm` on Metal, DX12 and Vulkan), so custom renderers build
/// the pipelines they build for a window.
pub const HEADLESS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// The device label of [`HeadlessGpu::shared`] — shows up in backend
/// validation messages and GPU captures.
const HEADLESS_LABEL: &str = "agg-gui-wgpu headless";

/// How the headless device is requested: [`HeadlessGpu::shared_with_options`]
/// and [`HeadlessGpu::create_with_options`].
///
/// Build one with [`HeadlessOptions::new`] (or [`Default`]) and the `with_*`
/// setters; the struct is `#[non_exhaustive]` so options can be added:
///
/// ```no_run
/// use std::time::Duration;
/// use agg_gui_wgpu::{HeadlessGpu, HeadlessOptions};
/// let options = HeadlessOptions::new().with_startup_budget(Duration::from_secs(120));
/// let gpu = HeadlessGpu::shared_with_options(options);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HeadlessOptions {
    /// Demand wgpu's software (fallback) adapter, as
    /// [`crate::GpuConfig::force_fallback_adapter`]. Default `false`. The
    /// shared device is kept per choice: asking for both holds two devices.
    pub force_fallback_adapter: bool,
    /// How long a call waits for the device, counted from when its creation
    /// began. Default [`crate::GPU_STARTUP_BUDGET`] (15 s), which a healthy
    /// machine meets with room to spare; a test harness on a machine that may
    /// be saturated by parallel builds asks for more (say 120 s), so a slow
    /// request is not taken for a hung driver.
    pub startup_budget: Duration,
    /// How the device's allocator reserves memory. Default
    /// [`GpuMemoryHints::MemoryUsage`]: a headless device serves tests and
    /// offscreen renders, where the few hundred MiB `Performance` reserves up
    /// front are pure overhead, paid again by every process of a test runner
    /// that gives each test its own.
    pub memory_hints: GpuMemoryHints,
    /// Features the device gets when the adapter offers them, as
    /// [`crate::GpuConfig::optional_features`]: masked against the adapter's,
    /// so an absent one degrades instead of failing the request. Default
    /// empty. The shared device is made with the first call's features, so
    /// every caller in a process should ask for the same ones (the ones its
    /// window's device asks for).
    pub optional_features: wgpu::Features,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            force_fallback_adapter: false,
            startup_budget: GPU_STARTUP_BUDGET,
            memory_hints: GpuMemoryHints::MemoryUsage,
            optional_features: wgpu::Features::empty(),
        }
    }
}

impl HeadlessOptions {
    /// The defaults: the hardware adapter, a 15 s budget, `MemoryUsage`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Demand wgpu's software (fallback) adapter. See
    /// [`HeadlessOptions::force_fallback_adapter`].
    pub fn with_force_fallback_adapter(mut self, force: bool) -> Self {
        self.force_fallback_adapter = force;
        self
    }

    /// Wait up to `budget` for the device. See
    /// [`HeadlessOptions::startup_budget`].
    pub fn with_startup_budget(mut self, budget: Duration) -> Self {
        self.startup_budget = budget;
        self
    }

    /// Choose how the device reserves memory. See
    /// [`HeadlessOptions::memory_hints`].
    pub fn with_memory_hints(mut self, hints: GpuMemoryHints) -> Self {
        self.memory_hints = hints;
        self
    }

    /// Ask for `features` where the adapter offers them. See
    /// [`HeadlessOptions::optional_features`].
    pub fn with_optional_features(mut self, features: wgpu::Features) -> Self {
        self.optional_features = features;
        self
    }
}

/// A wgpu device and queue with no window.
pub struct HeadlessGpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    adapter_info: wgpu::AdapterInfo,
    adapter_limits: wgpu::Limits,
    /// The adapter the device was opened on, kept for what a device cannot
    /// report (see [`HeadlessGpu::wgpu_adapter`]).
    wgpu_adapter: wgpu::Adapter,
    /// Whether, and why, the device was lost.
    loss: Arc<DeviceLoss>,
    /// The hints the device was requested with.
    memory_hints: GpuMemoryHints,
}

impl HeadlessGpu {
    /// The process-wide headless device, created on first use and never
    /// dropped; every later call returns the same one (or the same error —
    /// except a timeout, see [`Self::shared_with_options`]). The default
    /// [`HeadlessOptions`].
    ///
    /// One device for the whole process, because a fresh instance + device
    /// per test broke in two ways on Windows/NVIDIA, both seen in thread
    /// stacks captured from the hung/crashed test binary:
    ///
    /// - **Deadlock under the parallel runner** with `Backends::all()`: each
    ///   instance's GL backend spawns a "wgpu-hal WGL Instance Thread".  When
    ///   that thread exits, `nvoglv64!DllMain` (thread-detach, run under the
    ///   OS loader lock) blocks on a driver-internal lock, while other test
    ///   threads inside Vulkan calls — served by the same `nvoglv64.dll` —
    ///   block too; no thread made progress again.
    /// - **Access violation in `vulkan-1.dll`** (Vulkan loader 1.3.280),
    ///   inside `vkSetDebugUtilsObjectNameEXT` called from
    ///   `wgpu_hal::vulkan::DeviceShared::set_object_name` while building
    ///   `WgpuPipelines`, after earlier tests had created and destroyed their
    ///   own instances/devices — even with `--test-threads=1`.
    ///
    /// Sharing one device avoids repeated instance/device create/destroy
    /// altogether (it is also far cheaper per test).  `Backends::PRIMARY`
    /// matches production `Gpu::new` and keeps the GL/WGL path out.  Sharing
    /// is safe because every caller builds its own `WgpuGfxCtx`, textures and
    /// read-back buffers; the only device-wide effect is that a blocking
    /// `device.poll` may also wait for other threads' submissions.
    ///
    /// No environment variable is read: whether a missing adapter skips or
    /// fails a test suite is the caller's policy. agg-gui's own GPU tests skip
    /// (pass trivially) on [`HeadlessError::NoAdapter`].
    pub fn shared() -> Result<&'static HeadlessGpu, HeadlessError> {
        Self::shared_with_options(HeadlessOptions::default())
    }

    /// [`Self::shared`], or with `force_fallback_adapter` the process-wide
    /// device on wgpu's software (fallback) adapter — the same option as
    /// [`crate::GpuConfig::force_fallback_adapter`]. Each choice is created
    /// once and never dropped, so a process that asks for both holds two
    /// devices for its lifetime, never creating or destroying one per test
    /// (see [`Self::shared`]). A platform with no fallback adapter returns
    /// [`HeadlessError::NoFallbackAdapter`] every time.
    pub fn shared_with(
        force_fallback_adapter: bool,
    ) -> Result<&'static HeadlessGpu, HeadlessError> {
        Self::shared_with_options(
            HeadlessOptions::default().with_force_fallback_adapter(force_fallback_adapter),
        )
    }

    /// The process-wide device for `options.force_fallback_adapter` (one per
    /// choice, as [`Self::shared_with`]), waiting up to
    /// `options.startup_budget` for it.
    ///
    /// **The first call creates it, so its memory hints win**: a later call
    /// asking for other hints gets the existing device ([`Self::memory_hints`]
    /// says which it was made with). Rebuilding the device per hint is what
    /// [`Self::shared`] exists to avoid.
    ///
    /// **Every call has its own budget, counted from when the creation
    /// began.** A call whose budget runs out gets
    /// [`HeadlessError::StartupTimedOut`] (and a `log::warn!`), but that is
    /// not cached: the request keeps running, and once it returns every call
    /// gets the device. A request that never returns times out every call,
    /// and a call made after the deadline is told at once rather than waiting
    /// again. A failure the request itself returns (no adapter, a refused
    /// device, a panic) is every later call's answer, without a retry.
    pub fn shared_with_options(
        options: HeadlessOptions,
    ) -> Result<&'static HeadlessGpu, HeadlessError> {
        static HARDWARE: SharedStartup<HeadlessGpu, HeadlessError> = SharedStartup::new();
        static FALLBACK: SharedStartup<HeadlessGpu, HeadlessError> = SharedStartup::new();
        let shared = if options.force_fallback_adapter {
            &FALLBACK
        } else {
            &HARDWARE
        };
        shared
            .get_or_create(
                move || request_headless_device(options),
                HEADLESS_LABEL,
                options.startup_budget,
                HeadlessError::CreationPanicked,
                BACKGROUND_THREAD_AVAILABLE,
                &report_to(None),
                spawn_thread,
            )?
            .ok_or(HeadlessError::StartupTimedOut {
                budget: options.startup_budget,
            })
    }

    /// A headless device of the caller's own, requested exactly as
    /// [`Self::shared`]'s (same backends, features, limits, memory hints and
    /// start-up budget) but not shared: dropping it releases it, and
    /// destroying it affects no one else.
    ///
    /// For a test binary that has to end its device's life — destroy it to
    /// simulate a loss and rebuild, as a shell recovers a lost window
    /// device. Everything else uses [`Self::shared`]: see its doc for why a
    /// device per test is unsafe on some drivers.
    pub fn create() -> Result<HeadlessGpu, HeadlessError> {
        Self::create_with_options(HeadlessOptions::default())
    }

    /// [`Self::create`] with `options`. A device that arrives after
    /// `options.startup_budget` is leaked on its own thread, as a window's
    /// is: it belongs to this one caller, so no later call could use it.
    pub fn create_with_options(options: HeadlessOptions) -> Result<HeadlessGpu, HeadlessError> {
        create_within_budget(
            move || request_headless_device(options),
            HEADLESS_LABEL,
            options.startup_budget,
            BACKGROUND_THREAD_AVAILABLE,
            None,
        )?
        .ok_or(HeadlessError::StartupTimedOut {
            budget: options.startup_budget,
        })
    }

    /// The device every target and frame of this GPU allocates on.
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// The queue frames submit to.
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    /// What the device runs on — a harness picks its golden set by backend.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Which adapter the device runs on — name, backend, and whether it is
    /// the software fallback.
    pub fn adapter(&self) -> crate::AdapterSummary {
        crate::AdapterSummary::from_info(&self.adapter_info)
    }

    /// The adapter the device was opened on: what a `wgpu::Device` cannot
    /// report — downlevel capabilities, per-format texture features — is
    /// asked of it (the same as [`crate::Gpu::wgpu_adapter`]).
    pub fn wgpu_adapter(&self) -> &wgpu::Adapter {
        &self.wgpu_adapter
    }

    /// The adapter's own limits, which the device's request raised its
    /// texture size limit to (see `device_descriptor`).
    pub fn adapter_limits(&self) -> &wgpu::Limits {
        &self.adapter_limits
    }

    /// Has this device been lost (or destroyed) since it was created? Set
    /// from wgpu's device-lost callback, as [`crate::Gpu::device_lost`] is;
    /// [`Self::device_lost_reason`] says why.
    pub fn device_lost(&self) -> bool {
        self.loss.is_lost()
    }

    /// Why the device was lost, as wgpu reported it (its `DeviceLostReason`
    /// and message); `None` while it is not lost.
    pub fn device_lost_reason(&self) -> Option<String> {
        self.loss.lost_reason()
    }

    /// The memory hints the device was requested with — for the shared
    /// device, the first caller's (see [`Self::shared_with_options`]).
    pub fn memory_hints(&self) -> GpuMemoryHints {
        self.memory_hints
    }
}

/// The adapter and device requests of the headless device, which its callers
/// run under a start-up budget so a hung driver fails them with
/// [`HeadlessError::StartupTimedOut`] instead of hanging them: `Gpu::new`'s
/// requests, with no surface to be compatible with.
fn request_headless_device(options: HeadlessOptions) -> Result<HeadlessGpu, HeadlessError> {
    let (force_fallback_adapter, memory_hints) =
        (options.force_fallback_adapter, options.memory_hints);
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(
        instance.request_adapter(&crate::gpu::adapter_options(force_fallback_adapter, None)),
    )
    .map_err(|e| {
        if force_fallback_adapter {
            HeadlessError::NoFallbackAdapter(e.to_string())
        } else {
            HeadlessError::NoAdapter(e.to_string())
        }
    })?;
    let descriptor = crate::gpu::device_descriptor(
        HEADLESS_LABEL,
        options.optional_features,
        memory_hints,
        &adapter,
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
        .map_err(|e| HeadlessError::RequestDevice(e.to_string()))?;
    let loss = watch_device_loss_reason(&device);
    Ok(HeadlessGpu {
        device: Arc::new(device),
        queue: Arc::new(queue),
        adapter_info: adapter.get_info(),
        adapter_limits: adapter.limits(),
        wgpu_adapter: adapter,
        loss,
        memory_hints,
    })
}

/// An offscreen render target with CPU read-back.
pub struct HeadlessTarget {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The device's loss record when the target was made from a
    /// [`HeadlessGpu`]: a known loss fails a wait at once, with its reason.
    loss: Option<Arc<DeviceLoss>>,
    /// How long a read-back (and a [`HeadlessFrame`]'s pacing) waits.
    wait_budget: Duration,
}

impl HeadlessTarget {
    /// A `width` x `height` device-pixel target in [`HEADLESS_FORMAT`] on
    /// `gpu`.
    pub fn new(gpu: &HeadlessGpu, width: u32, height: u32) -> Self {
        let mut target = Self::with_format(
            Arc::clone(&gpu.device),
            Arc::clone(&gpu.queue),
            HEADLESS_FORMAT,
            width,
            height,
        );
        target.loss = Some(Arc::clone(&gpu.loss));
        target
    }

    /// A target in `format` on any device — for a caller whose
    /// [`WgpuGfxCtx`] was built for another format. Read-back needs an 8-bit
    /// RGBA or BGRA format.
    ///
    /// The size is clamped to `[1, max_texture_dimension_2d]` on both axes as
    /// a shell clamps its surface ([`crate::clamp_surface_size`]); the usage
    /// is a `CopySrc` surface's (`RENDER_ATTACHMENT | COPY_SRC`), so blend-mode
    /// composites read the target directly as they read such a window.
    pub fn with_format(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let max_dim = device.limits().max_texture_dimension_2d;
        let (width, height) = crate::clamp_surface_size(width, height, max_dim);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            device,
            queue,
            texture,
            view,
            loss: None,
            wait_budget: GPU_WAIT_BUDGET,
        }
    }

    /// Wait at most `budget` for the GPU in a read-back (and in the frame
    /// pacing of a [`HeadlessFrame`] built on this target) before failing
    /// with [`HeadlessError::GpuTimeout`]. Default [`crate::GPU_WAIT_BUDGET`].
    pub fn with_wait_budget(mut self, budget: Duration) -> Self {
        self.wait_budget = budget;
        self
    }

    /// How long a wait on the GPU may take (see [`Self::with_wait_budget`]).
    pub fn wait_budget(&self) -> Duration {
        self.wait_budget
    }

    /// The same target settings (device-loss record, wait budget) on a new
    /// texture of `width` x `height`.
    fn resized(&self, width: u32, height: u32) -> Self {
        let mut target = Self::with_format(
            Arc::clone(&self.device),
            Arc::clone(&self.queue),
            self.format(),
            width,
            height,
        );
        target.loss = self.loss.clone();
        target.wait_budget = self.wait_budget;
        target
    }

    /// Wait at most the wait budget for the GPU to reach `index`.
    fn wait_for(&self, index: wgpu::SubmissionIndex) -> Result<(), HeadlessError> {
        crate::gpu_wait::wait_for_submission(
            &self.device,
            Some(index),
            self.loss.as_deref(),
            self.wait_budget,
        )
        .map_err(HeadlessError::from)
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.texture.format()
    }

    /// `(width, height)` in device pixels, after clamping.
    pub fn size(&self) -> (u32, u32) {
        (self.texture.width(), self.texture.height())
    }

    /// The target's pixels as RGBA8, `width * 4` bytes per row with no
    /// padding, top row first (the y-down order of a PNG or a screenshot).
    /// Blocks until everything submitted to the device so far has rendered,
    /// for at most the wait budget ([`HeadlessError::GpuTimeout`] after it,
    /// [`HeadlessError::DeviceLost`] on a lost device).
    pub fn read_rgba(&self) -> Result<Vec<u8>, HeadlessError> {
        let format = self.format();
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(HeadlessError::UnreadableFormat(format));
        }
        crate::screenshot_readback::read_texture_rgba_bounded(
            &self.device,
            &self.queue,
            &self.texture,
            format,
            self.loss.as_deref(),
            self.wait_budget,
        )
        .map_err(|e| match e {
            ReadbackError::Wait(wait) => HeadlessError::from(wait),
            other => HeadlessError::Readback(other.to_string()),
        })
    }

    /// The target's pixels as an `agg_gui::Framebuffer`: RGBA8, bottom row
    /// first (row 0 is y = 0 in agg-gui's y-up coordinates), the layout the
    /// software renderer paints into, so pixel checks written against a
    /// software frame index the same bytes.
    pub fn read_framebuffer(&self) -> Result<Framebuffer, HeadlessError> {
        let top_down = self.read_rgba()?;
        let (width, height) = self.size();
        let mut fb = Framebuffer::new(width, height);
        let row = width as usize * 4;
        for (y, dst) in fb.pixels_mut().chunks_exact_mut(row).enumerate() {
            let src = (height as usize - 1 - y) * row;
            dst.copy_from_slice(&top_down[src..src + row]);
        }
        Ok(fb)
    }
}

/// A [`WgpuGfxCtx`] and the [`HeadlessTarget`] it paints into: one window's
/// worth of frames with no window.
///
/// Keep one per harness and render many frames through it — the context's
/// pipelines, glyph and texture caches and retained layers persist between
/// frames as they do in a shell.
///
/// Frames are paced as a window's are: at most two on the GPU at once, as a
/// double-buffered swap chain's present allows (see [`Self::render`]).
pub struct HeadlessFrame {
    ctx: WgpuGfxCtx,
    target: HeadlessTarget,
    /// The submission that ended the previous frame, waited on once the next
    /// frame is submitted.
    previous: Option<wgpu::SubmissionIndex>,
}

impl HeadlessFrame {
    /// A `width` x `height` device-pixel frame on `gpu`, in
    /// [`HEADLESS_FORMAT`].
    pub fn new(gpu: &HeadlessGpu, width: u32, height: u32) -> Self {
        let target = HeadlessTarget::new(gpu, width, height);
        let (w, h) = target.size();
        let ctx = WgpuGfxCtx::new(
            Arc::clone(&gpu.device),
            Arc::clone(&gpu.queue),
            target.format(),
            w as f32,
            h as f32,
        )
        .with_adapter(gpu.wgpu_adapter());
        Self {
            ctx,
            target,
            previous: None,
        }
    }

    /// `(width, height)` in device pixels.
    pub fn size(&self) -> (u32, u32) {
        self.target.size()
    }

    /// Resize the frame, as a shell's surface reconfigure does: a new target
    /// at the new size, the same context (its caches survive). No-op when the
    /// size is unchanged.
    pub fn resize(&mut self, width: u32, height: u32) {
        let max_dim = self.target.device.limits().max_texture_dimension_2d;
        if self.size() == crate::clamp_surface_size(width, height, max_dim) {
            return;
        }
        let resized = self.target.resized(width, height);
        let old = std::mem::replace(&mut self.target, resized);
        // Destroyed after the next frame's submit rather than left to the
        // browser's garbage collector (see `WgpuGfxCtx::retire_texture`).
        self.ctx.retire_texture(old.texture);
    }

    /// Wait at most `budget` for the GPU in this frame's pacing and
    /// read-backs (see [`HeadlessTarget::with_wait_budget`]).
    pub fn set_wait_budget(&mut self, budget: Duration) {
        self.target.wait_budget = budget;
    }

    pub fn ctx(&self) -> &WgpuGfxCtx {
        &self.ctx
    }

    /// The context, for state that lives across frames (LCD text, cached
    /// layers) or for [`WgpuGfxCtx::last_end_frame_stats`].
    pub fn ctx_mut(&mut self) -> &mut WgpuGfxCtx {
        &mut self.ctx
    }

    pub fn target(&self) -> &HeadlessTarget {
        &self.target
    }

    /// Render one frame: clear the target to `clear`, run `paint`, flush.
    ///
    /// The frame a shell runs (agg-gui-shell `Painter::paint`): the target is
    /// handed over as the frame's surface texture (so blend-mode composites
    /// and screenshot capture can read it), the frame begins, `paint` queues
    /// 2-D commands and custom render passes, `end_frame` submits them, and
    /// the per-frame handles are released as `present` releases them. The
    /// context is reset to the target's size first and the frame starts from
    /// `clear`, as a shell's frame starts from the theme's background, unless
    /// `paint` clears first, before drawing anything (then it starts from
    /// that colour). A `reset` inside `paint`, as the shells' `default_paint`
    /// makes, keeps the clear, so every frame's pixels are determined by what
    /// it paints.
    /// LCD text follows `agg_gui::font_settings::lcd_enabled`, as
    /// `begin_frame` sets it.
    ///
    /// Once the frame is submitted, `render` waits until the frame before it
    /// has finished on the GPU, so at most two frames (this one and its
    /// predecessor) are ever in flight, as with a window's double-buffered
    /// swap chain; the CPU still prepares a frame while the GPU draws the
    /// last. A headless frame has no present to block on, so unpaced a
    /// harness ran as far ahead of the GPU as the backend's queue let it
    /// (Metal allows 4096 outstanding command buffers), and several test
    /// processes sharing the GPU spent their time blocked in the driver
    /// rather than drawing. The wait covers every submission on the device
    /// up to that frame, other threads' included (the device may be
    /// [`HeadlessGpu::shared`]).
    ///
    /// The wait is bounded by the target's wait budget
    /// ([`crate::GPU_WAIT_BUDGET`] by default): when the GPU does not finish
    /// the previous frame in time, or the device is lost, `render` logs it
    /// (`log::warn!`) and returns; [`Self::try_render`] returns it instead.
    ///
    /// Returns the submission index that ends this frame: everything the
    /// frame submitted (its 2-D commands and custom render passes) is done
    /// once the GPU has reached it, e.g. through
    /// `device.poll(wgpu::PollType::Wait { submission_index: Some(index), .. })`.
    pub fn render(
        &mut self,
        clear: Color,
        paint: impl FnOnce(&mut WgpuGfxCtx),
    ) -> wgpu::SubmissionIndex {
        let (index, waited) = self.render_frame(clear, paint);
        if let Err(e) = waited {
            log::warn!("waiting for the previous headless frame on the GPU: {e}");
        }
        index
    }

    /// [`Self::render`], failing when the wait for the previous frame does:
    /// [`HeadlessError::GpuTimeout`] when the GPU did not finish it within
    /// the wait budget, [`HeadlessError::DeviceLost`] (with wgpu's reason)
    /// when the device is gone. This frame has been painted and submitted
    /// either way. A harness that must not run on after a hung GPU uses this.
    pub fn try_render(
        &mut self,
        clear: Color,
        paint: impl FnOnce(&mut WgpuGfxCtx),
    ) -> Result<wgpu::SubmissionIndex, HeadlessError> {
        let (index, waited) = self.render_frame(clear, paint);
        waited.map(|()| index)
    }

    /// One frame, and the outcome of the wait for the frame before it.
    fn render_frame(
        &mut self,
        clear: Color,
        paint: impl FnOnce(&mut WgpuGfxCtx),
    ) -> (wgpu::SubmissionIndex, Result<(), HeadlessError>) {
        let (w, h) = self.target.size();
        self.ctx.reset(w as f32, h as f32);
        self.ctx.set_surface_texture(self.target.texture.clone());
        self.ctx
            .begin_frame_cleared(self.target.view.clone(), clear);
        paint(&mut self.ctx);
        self.ctx.end_frame();
        self.ctx.release_frame_texture();
        self.pace()
    }

    /// End the frame's submissions and wait for the frame before it.
    ///
    /// The frame may make several submissions (`end_frame`'s, and none at all
    /// when there was nothing to flush), so its end is marked with an empty
    /// submission, numbered after all of them; waiting on that index waits
    /// for the whole frame. The wait is bounded by the target's budget.
    fn pace(&mut self) -> (wgpu::SubmissionIndex, Result<(), HeadlessError>) {
        let this_frame = self.target.queue.submit(std::iter::empty());
        let waited = match self.previous.replace(this_frame.clone()) {
            Some(previous) => self.target.wait_for(previous),
            None => Ok(()),
        };
        (this_frame, waited)
    }

    /// [`Self::render`] painting `app`: agg-gui-shell's `default_paint`
    /// without its layout, which stays the caller's decision (lay `app` out
    /// at the frame's size, in logical pixels, before the first render).
    pub fn render_app(&mut self, clear: Color, app: &mut App) -> wgpu::SubmissionIndex {
        self.render(clear, |ctx| app.paint(ctx))
    }

    /// [`Self::try_render`] painting `app`, as [`Self::render_app`] does.
    pub fn try_render_app(
        &mut self,
        clear: Color,
        app: &mut App,
    ) -> Result<wgpu::SubmissionIndex, HeadlessError> {
        self.try_render(clear, |ctx| app.paint(ctx))
    }

    /// The last rendered frame as RGBA8, top row first — see
    /// [`HeadlessTarget::read_rgba`].
    pub fn read_rgba(&self) -> Result<Vec<u8>, HeadlessError> {
        self.target.read_rgba()
    }

    /// The last rendered frame bottom row first — see
    /// [`HeadlessTarget::read_framebuffer`].
    pub fn read_framebuffer(&self) -> Result<Framebuffer, HeadlessError> {
        self.target.read_framebuffer()
    }
}
