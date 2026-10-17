//! Bounded waits for the GPU: every blocking `Device::poll` a caller cannot
//! afford to sit in forever goes through [`wait_for_submission`], and a
//! device's loss is recorded with the reason wgpu gave ([`DeviceLoss`]).
//!
//! **Why a wait needs a bound.** `PollType::Wait { timeout: None }` returns
//! only once the GPU reaches the submission. A GPU that hangs, or loses its
//! device without saying so (a paravirtual Metal GPU on a CI runner, a
//! driver reset), never gets there: on Metal the backend's wait loops on the
//! command buffer's status, which stays short of `Completed` when the command
//! buffer failed. The thread is then parked for good, and a test harness or a
//! read-back has nothing to report but a test runner's timeout. With a bound
//! the wait ends with [`GpuWaitError::TimedOut`] and the caller can say what
//! happened.
//!
//! **Why [`GPU_WAIT_BUDGET`] is 30 s.** A frame or a read-back on a healthy
//! GPU takes milliseconds, and even a software rasterizer on a loaded CI
//! machine finishes one well inside a few seconds; 30 s is far past any real
//! frame, so a wait that reaches it is a GPU that is not coming back, not a
//! slow one. It is short enough that a test reports the hung GPU well before
//! a runner's own per-test timeout (MatterCAD's is 300 s).
//!
//! **The bound is wgpu's own.** The budget is handed to `Device::poll` as its
//! timeout; every native backend's wait honours it (Metal polls the command
//! buffer's status, Vulkan and DX12 wait on their fence with it), so no
//! helper thread is spent per frame. A lost device is checked before the poll
//! (wgpu panics when a lost device is polled, since a loss has no
//! `PollError`) and a panic inside the poll is caught and reported as a loss,
//! the only way wgpu reports one it finds while polling.
//!
//! Used by `crate::headless` (frame pacing) and `crate::screenshot_readback`
//! (the read-back copy). Rust-only tests live in `gpu_wait_tests.rs`.

use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// How long a frame wait or a read-back waits for the GPU before reporting it
/// hung — far longer than any real frame (see the module docs).
pub const GPU_WAIT_BUDGET: Duration = Duration::from_secs(30);

/// Why a bounded GPU wait ended without the GPU finishing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GpuWaitError {
    /// The GPU did not reach the submission within `budget`.
    TimedOut { budget: Duration },
    /// The device was lost; carries wgpu's reason, or the text of the panic
    /// wgpu raised when it found the loss while polling.
    DeviceLost(String),
    /// wgpu refused the wait itself (a submission index from another queue).
    Failed(String),
}

impl std::fmt::Display for GpuWaitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut { budget } => write!(
                f,
                "the GPU did not finish its work within {}s; it is hung or its device was lost",
                crate::gpu_budget::format_budget_seconds(*budget)
            ),
            Self::DeviceLost(reason) => write!(f, "the GPU device was lost: {reason}"),
            Self::Failed(e) => write!(f, "waiting for the GPU failed: {e}"),
        }
    }
}

/// A device's loss, as its device-lost callback reported it: whether it
/// happened and the reason wgpu gave. Shared between the callback (called
/// from whichever thread wgpu notices on) and the device's owner.
#[derive(Debug, Default)]
pub(crate) struct DeviceLoss {
    lost: AtomicBool,
    reason: Mutex<Option<String>>,
}

impl DeviceLoss {
    /// Has the device been lost (or destroyed)?
    pub(crate) fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// The loss's reason when the device is lost, `None` while it is not.
    pub(crate) fn lost_reason(&self) -> Option<String> {
        if !self.is_lost() {
            return None;
        }
        self.reason
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Record a loss. The reason is stored before the flag is raised, so a
    /// reader that sees the flag also sees the reason.
    pub(crate) fn record(&self, reason: String) {
        *self.reason.lock().unwrap_or_else(|e| e.into_inner()) = Some(reason);
        self.lost.store(true, Ordering::Release);
    }
}

/// Watch `device` for loss, keeping the reason wgpu reports.
///
/// Replaces any device-lost callback set before (wgpu keeps one). A
/// `Destroyed` reason counts as a loss: a destroyed device is as dead as a
/// lost one, and `Device::destroy` is how tests simulate a loss.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn watch_device_loss_reason(device: &wgpu::Device) -> std::sync::Arc<DeviceLoss> {
    let loss = std::sync::Arc::new(DeviceLoss::default());
    let record = std::sync::Arc::clone(&loss);
    device.set_device_lost_callback(move |reason, message| {
        let text = if message.is_empty() {
            format!("{reason:?}")
        } else {
            format!("{reason:?}: {message}")
        };
        record.record(text);
    });
    loss
}

/// Wait at most `budget` for the GPU to reach `index` (every submission so
/// far when `None`) on `device`.
///
/// `loss`, when the caller watches the device, turns a known loss into
/// [`GpuWaitError::DeviceLost`] without polling, and names the reason when a
/// wait fails because the device went away during it.
pub(crate) fn wait_for_submission(
    device: &wgpu::Device,
    index: Option<wgpu::SubmissionIndex>,
    loss: Option<&DeviceLoss>,
    budget: Duration,
) -> Result<(), GpuWaitError> {
    bounded_wait(
        |timeout| {
            device.poll(wgpu::PollType::Wait {
                submission_index: index,
                timeout: Some(timeout),
            })
        },
        loss,
        budget,
    )
}

/// The decision behind [`wait_for_submission`], with the poll handed in so
/// tests can stand in for a GPU that never finishes. `poll` is given the
/// budget as its timeout and must return by then, as wgpu's does.
pub(crate) fn bounded_wait<P>(
    poll: P,
    loss: Option<&DeviceLoss>,
    budget: Duration,
) -> Result<(), GpuWaitError>
where
    P: FnOnce(Duration) -> Result<wgpu::PollStatus, wgpu::PollError>,
{
    if let Some(reason) = loss.and_then(DeviceLoss::lost_reason) {
        return Err(GpuWaitError::DeviceLost(reason));
    }
    let failure = match panic::catch_unwind(AssertUnwindSafe(|| poll(budget))) {
        Ok(Ok(_)) => return Ok(()),
        Ok(Err(wgpu::PollError::Timeout)) => GpuWaitError::TimedOut { budget },
        Ok(Err(e)) => GpuWaitError::Failed(e.to_string()),
        Err(payload) => GpuWaitError::DeviceLost(crate::gpu_budget::panic_text(payload.as_ref())),
    };
    // A loss reported while the wait ran explains a timeout or a panic better
    // than either does on its own.
    match loss.and_then(DeviceLoss::lost_reason) {
        Some(reason) => Err(GpuWaitError::DeviceLost(reason)),
        None => Err(failure),
    }
}

#[cfg(test)]
#[path = "gpu_wait_tests.rs"]
mod tests;
