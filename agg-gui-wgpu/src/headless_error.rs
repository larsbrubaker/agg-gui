//! [`HeadlessError`]: why headless rendering could not run — the device
//! request, a GPU wait that ran out of budget or found the device lost, or a
//! read-back. Split out of `headless.rs` (the device, target and frame that
//! return it), which re-exports it.

use std::time::Duration;

use crate::gpu_wait::GpuWaitError;

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
    /// budget (a hung driver, or a machine too loaded to meet the budget).
    /// For the shared device this is not final: the request keeps running,
    /// and a later call gets the device if it arrives. For
    /// [`HeadlessGpu::create`](super::HeadlessGpu::create) the half-built device is leaked on its own
    /// thread, as `Gpu::new` does.
    StartupTimedOut {
        /// The budget that expired.
        budget: Duration,
    },
    /// The target's texels are not 8-bit RGBA or BGRA, which is all the
    /// read-back converts.
    UnreadableFormat(wgpu::TextureFormat),
    /// Mapping the read-back buffer failed, or wgpu refused the wait for it.
    Readback(String),
    /// The GPU did not finish a frame or a read-back within `budget` (the
    /// target's wait budget, [`crate::GPU_WAIT_BUDGET`] by default): it hung,
    /// or lost its device without reporting it.
    GpuTimeout {
        /// The budget that expired.
        budget: Duration,
    },
    /// The device was lost (or destroyed) — a GPU reset, a driver update, a
    /// virtual GPU going away. Carries wgpu's reason. Nothing more renders on
    /// this device; [`HeadlessGpu::create`](super::HeadlessGpu::create) builds a new one.
    DeviceLost(String),
    /// The shared device's adapter or device request panicked inside wgpu
    /// (only [`HeadlessGpu::shared_with_options`](super::HeadlessGpu::shared_with_options) and the calls built on it:
    /// every caller of the shared device gets the same answer). Carries the
    /// panic's text. [`HeadlessGpu::create`](super::HeadlessGpu::create) and
    /// [`HeadlessGpu::create_with_options`](super::HeadlessGpu::create_with_options) resume the panic on their caller
    /// instead, since the device is that caller's alone.
    CreationPanicked(String),
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
            Self::GpuTimeout { budget } => write!(
                f,
                "the GPU did not finish within {}s: it is hung or its device was lost",
                crate::gpu_budget::format_budget_seconds(*budget)
            ),
            Self::DeviceLost(reason) => write!(f, "the headless GPU device was lost: {reason}"),
            Self::CreationPanicked(e) => {
                write!(f, "the headless wgpu device request panicked: {e}")
            }
        }
    }
}

impl std::error::Error for HeadlessError {}

impl From<GpuWaitError> for HeadlessError {
    fn from(error: GpuWaitError) -> Self {
        match error {
            GpuWaitError::TimedOut { budget } => Self::GpuTimeout { budget },
            GpuWaitError::DeviceLost(reason) => Self::DeviceLost(reason),
            GpuWaitError::Failed(e) => Self::Readback(e),
        }
    }
}
