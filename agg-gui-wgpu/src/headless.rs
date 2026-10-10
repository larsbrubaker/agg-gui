//! Offscreen GPU rendering with no window: the device, target, frame and
//! read-back a test harness paints a whole widget tree with, through the same
//! [`WgpuGfxCtx`] pipeline the platform shells drive.
//!
//! - [`HeadlessGpu::shared`] — one device and queue for the whole process,
//!   requested the way [`crate::Gpu::new`] requests a window's (same backends,
//!   features and limits, same start-up budget), but with no surface.
//!   [`HeadlessGpu::shared_with`] takes the window's
//!   [`crate::GpuConfig::force_fallback_adapter`] option, so a harness can
//!   paint on wgpu's software adapter where the platform has one.
//! - [`HeadlessTarget`] — an offscreen texture of a given device-pixel size in
//!   [`HEADLESS_FORMAT`], with RGBA8 read-back ([`HeadlessTarget::read_rgba`],
//!   top row first, unpadded) and an `agg_gui::Framebuffer` copy (bottom row
//!   first, as the software renderer lays its pixels out).
//! - [`HeadlessFrame`] — a [`WgpuGfxCtx`] bound to a target that runs a
//!   shell's frame (`set_surface_texture`, `begin_frame`, paint, `end_frame`,
//!   release) around a paint closure or an [`App`](agg_gui::App). Custom
//!   render passes ([`crate::WgpuCustomRender`]) queued during paint run
//!   inside it as they do on a window.
//!
//! The read-back copy is `screenshot_readback::read_texture_rgba`, the one
//! [`WgpuGfxCtx::read_screenshot`] uses. Native only: the device request
//! blocks, which a browser cannot.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use agg_gui::color::Color;
use agg_gui::{App, Framebuffer};

use crate::gpu_budget::{create_within_budget, BACKGROUND_THREAD_AVAILABLE, GPU_STARTUP_BUDGET};
use crate::WgpuGfxCtx;

/// The format of a [`HeadlessTarget`]: the frame format the native shells get
/// on every primary backend (`Gpu::new` takes the surface's first non-sRGB
/// format, `Bgra8Unorm` on Metal, DX12 and Vulkan), so custom renderers build
/// the pipelines they build for a window.
pub const HEADLESS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// The device label of [`HeadlessGpu::shared`] — shows up in backend
/// validation messages and GPU captures.
const HEADLESS_LABEL: &str = "agg-gui-wgpu headless";

/// Why headless rendering could not run.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeadlessError {
    /// No adapter on the primary backends (Vulkan, Metal, DX12): a machine
    /// with no GPU and no software Vulkan driver. Carries wgpu's text.
    NoAdapter(String),
    /// The software (fallback) adapter was demanded and the platform has
    /// none — always on macOS, on Linux without lavapipe. Carries wgpu's
    /// text. A caller skips, as on [`Self::NoAdapter`].
    NoFallbackAdapter(String),
    /// The adapter refused the device request. Carries wgpu's text.
    RequestDevice(String),
    /// The adapter or device request did not return within the start-up
    /// budget (a hung driver). The half-built device is leaked on its own
    /// thread, as `Gpu::new` does.
    StartupTimedOut {
        /// The budget that expired.
        budget: Duration,
    },
    /// The target's texels are not 8-bit RGBA or BGRA, which is all the
    /// read-back converts.
    UnreadableFormat(wgpu::TextureFormat),
    /// Mapping the read-back buffer failed (the device was lost).
    Readback(String),
}

impl std::fmt::Display for HeadlessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(e) => write!(
                f,
                "no wgpu adapter for headless rendering (Vulkan, Metal or DX12): {e}"
            ),
            Self::NoFallbackAdapter(e) => write!(
                f,
                "no software (fallback) wgpu adapter for headless rendering on this platform: {e}"
            ),
            Self::RequestDevice(e) => write!(f, "could not request a headless wgpu device: {e}"),
            Self::StartupTimedOut { budget } => write!(
                f,
                "the headless wgpu device could not be created: the adapter or device request \
                 did not return within {}s",
                crate::gpu_budget::format_budget_seconds(*budget)
            ),
            Self::UnreadableFormat(format) => write!(
                f,
                "cannot read back a {format:?} target: only 8-bit RGBA and BGRA formats are \
                 converted"
            ),
            Self::Readback(e) => write!(f, "headless read-back failed: {e}"),
        }
    }
}

impl std::error::Error for HeadlessError {}

/// A wgpu device and queue with no window.
pub struct HeadlessGpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    adapter_info: wgpu::AdapterInfo,
}

impl HeadlessGpu {
    /// The process-wide headless device, created on first use and never
    /// dropped; every later call returns the same one (or the same error).
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
        Self::shared_with(false)
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
        static HARDWARE: OnceLock<Result<HeadlessGpu, HeadlessError>> = OnceLock::new();
        static FALLBACK: OnceLock<Result<HeadlessGpu, HeadlessError>> = OnceLock::new();
        let shared = if force_fallback_adapter {
            &FALLBACK
        } else {
            &HARDWARE
        };
        shared
            .get_or_init(|| create_shared(force_fallback_adapter))
            .as_ref()
            .map_err(Clone::clone)
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
}

/// Build the shared device inside the start-up budget `Gpu::new` uses, so a
/// hung driver fails the caller with [`HeadlessError::StartupTimedOut`]
/// instead of hanging it.
fn create_shared(force_fallback_adapter: bool) -> Result<HeadlessGpu, HeadlessError> {
    create_within_budget(
        move || request_headless_device(force_fallback_adapter),
        HEADLESS_LABEL,
        GPU_STARTUP_BUDGET,
        BACKGROUND_THREAD_AVAILABLE,
        None,
    )?
    .ok_or(HeadlessError::StartupTimedOut {
        budget: GPU_STARTUP_BUDGET,
    })
}

/// The adapter and device requests of [`create_shared`]: `Gpu::new`'s, with
/// no surface to be compatible with.
fn request_headless_device(force_fallback_adapter: bool) -> Result<HeadlessGpu, HeadlessError> {
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
    let descriptor =
        crate::gpu::device_descriptor(HEADLESS_LABEL, wgpu::Features::empty(), &adapter);
    let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
        .map_err(|e| HeadlessError::RequestDevice(e.to_string()))?;
    Ok(HeadlessGpu {
        device: Arc::new(device),
        queue: Arc::new(queue),
        adapter_info: adapter.get_info(),
    })
}

/// An offscreen render target with CPU read-back.
pub struct HeadlessTarget {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl HeadlessTarget {
    /// A `width` x `height` device-pixel target in [`HEADLESS_FORMAT`] on
    /// `gpu`.
    pub fn new(gpu: &HeadlessGpu, width: u32, height: u32) -> Self {
        Self::with_format(
            Arc::clone(&gpu.device),
            Arc::clone(&gpu.queue),
            HEADLESS_FORMAT,
            width,
            height,
        )
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
        }
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
    /// Blocks until everything submitted to the device so far has rendered.
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
        crate::screenshot_readback::read_texture_rgba(
            &self.device,
            &self.queue,
            &self.texture,
            format,
        )
        .map_err(|e| HeadlessError::Readback(e.to_string()))
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
pub struct HeadlessFrame {
    ctx: WgpuGfxCtx,
    target: HeadlessTarget,
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
        );
        Self { ctx, target }
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
        self.target = HeadlessTarget::with_format(
            Arc::clone(&self.target.device),
            Arc::clone(&self.target.queue),
            self.target.format(),
            width,
            height,
        );
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
    /// `clear`, where a swap-chain image starts from whatever it held — so
    /// `paint` must not call `reset` (that drops the clear) and every frame's
    /// pixels are determined by what it paints. LCD text follows
    /// `agg_gui::font_settings::lcd_enabled`, as `begin_frame` sets it.
    pub fn render(&mut self, clear: Color, paint: impl FnOnce(&mut WgpuGfxCtx)) {
        let (w, h) = self.target.size();
        self.ctx.reset(w as f32, h as f32);
        self.ctx.set_surface_texture(self.target.texture.clone());
        self.ctx
            .begin_frame_cleared(self.target.view.clone(), clear);
        paint(&mut self.ctx);
        self.ctx.end_frame();
        self.ctx.release_frame_texture();
    }

    /// [`Self::render`] painting `app`: agg-gui-shell's `default_paint`
    /// without its layout, which stays the caller's decision (lay `app` out
    /// at the frame's size, in logical pixels, before the first render).
    pub fn render_app(&mut self, clear: Color, app: &mut App) {
        self.render(clear, |ctx| app.paint(ctx));
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
