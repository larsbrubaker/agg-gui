//! [`DeviceLostFlag`]: the handle a shell, a render context and a custom
//! render pass share to ask "has this device been lost?", and
//! [`mapped_read_allowed`], the one decision every read of a mapped buffer
//! in this crate makes before taking its range.
//!
//! **Why a read needs the check.** A lost device destroys every buffer made
//! on it, a buffer already mapped for reading included, and wgpu offers no
//! fallible way to take a mapped range: `get_mapped_range` on a destroyed
//! buffer panics. A map that completed before the loss therefore cannot be
//! read after it, and the only guard is to ask, immediately before the read,
//! whether the device is lost.
//!
//! **What the flag guarantees.** The device-lost callback raises it. On a
//! native device a loss destroys buffers only inside a `Device::poll`: after
//! `Device::destroy` the poll that finds the queue empty destroys the
//! buffers and then runs the callback before it returns, and a
//! driver-reported loss runs the callback where the device is marked lost,
//! before any later poll releases the buffers. So a caller that polls
//! the device on one thread and checks the flag after its poll sees the loss
//! no later than the poll that destroyed its buffers. The one window left is
//! another thread polling the same device between the check and the read;
//! agg-gui drives a device from one thread, and a renderer that polls from a
//! worker must check on that worker, after its own poll.
//!
//! **In the browser** nothing records a loss (the web shell has no
//! device-lost callback to hand over), so the flag stays false there; the
//! browser's WebGPU does not destroy a mapped buffer's range on loss, so the
//! reads it would guard do not panic.
//!
//! Made by [`crate::Gpu`] and [`crate::HeadlessGpu`] for their devices, and
//! by [`DeviceLostFlag::watch`] for a device an app opens itself; handed to
//! [`crate::WgpuGfxCtx::set_device_lost`], and seen by custom render passes
//! as [`crate::WgpuCustomRenderCtx::device_lost`]. Read by
//! `screenshot_readback.rs`, `screenshot_capture.rs` and
//! `screenshot_scaled.rs`. Tests: `device_lost_flag_tests.rs`,
//! `tests/device_lost_flag_watch_tests.rs` and
//! `tests/lost_device_frame_tests.rs`.

use std::sync::Arc;

use crate::gpu_wait::DeviceLoss;

/// Whether a device has been lost (or destroyed), shared between the
/// device's owner, its render context and any custom render pass.
///
/// Cheap to clone (one `Arc`); every clone reads the same record, so a pass
/// can keep one and check [`Self::is_lost`] frames later, from a worker or a
/// later tick. See the module docs for when the flag is raised relative to
/// the loss destroying the device's buffers. A flag made with
/// [`Default::default`] is never raised: it belongs to no device's callback.
#[derive(Clone, Debug, Default)]
pub struct DeviceLostFlag {
    loss: Arc<DeviceLoss>,
}

impl DeviceLostFlag {
    /// Watch `device` for loss: the flag for a device the caller opened
    /// itself (a renderer's own compute or test device), as
    /// [`crate::Gpu::device_lost_flag`] and
    /// [`crate::HeadlessGpu::device_lost_flag`] are for the devices agg-gui
    /// opens.
    ///
    /// **Replaces the device's lost callback.** wgpu keeps one callback per
    /// device, so a callback set on `device` before this call stops being
    /// called, and a later `set_device_lost_callback` (or a second `watch`)
    /// leaves this flag unraised. Watch a device once and share the flag by
    /// cloning it. Never call it on the device of a [`crate::Gpu`] or a
    /// [`crate::HeadlessGpu`]: take theirs instead, which their own waits
    /// and read-backs depend on.
    ///
    /// `Destroyed` counts as a loss: a destroyed device is as dead as a lost
    /// one, and `Device::destroy` is how tests simulate a loss. Native only:
    /// the browser's device-lost promise belongs to the web shell.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn watch(device: &wgpu::Device) -> Self {
        Self {
            loss: crate::gpu_wait::watch_device_loss_reason(device),
        }
    }

    /// Has the device been lost (or destroyed)? Once true, stays true.
    pub fn is_lost(&self) -> bool {
        self.loss.is_lost()
    }

    /// Why the device was lost, as wgpu reported it (its `DeviceLostReason`
    /// and message); `None` while it is not lost.
    pub fn lost_reason(&self) -> Option<String> {
        self.loss.lost_reason()
    }

    /// The record itself, for the crate's bounded waits.
    pub(crate) fn loss(&self) -> &DeviceLoss {
        &self.loss
    }

    /// The shared record, for an owner that keeps it beside the flag.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn shared(&self) -> Arc<DeviceLoss> {
        Arc::clone(&self.loss)
    }

    /// Record a loss by hand, for tests that cannot make wgpu lose a device
    /// between a map and its read.
    #[cfg(test)]
    pub(crate) fn record_for_test(&self, reason: &str) {
        self.loss.record(reason.to_owned());
    }
}

impl crate::WgpuGfxCtx {
    /// Record the loss flag of this context's device
    /// ([`crate::Gpu::device_lost_flag`], [`crate::HeadlessGpu::device_lost_flag`]),
    /// so the context's screenshot read-backs refuse to read a lost device's
    /// buffers and custom render passes see it as
    /// [`crate::WgpuCustomRenderCtx::device_lost`]. Without it the context
    /// keeps a flag of its own that is never raised (as in the browser).
    pub fn set_device_lost(&mut self, flag: &DeviceLostFlag) {
        self.device_lost = flag.clone();
    }

    /// [`Self::set_device_lost`] in builder form, for the line that makes the
    /// context.
    pub fn with_device_lost(mut self, flag: &DeviceLostFlag) -> Self {
        self.set_device_lost(flag);
        self
    }

    /// The loss flag of this context's device (see [`Self::set_device_lost`]).
    pub fn device_lost(&self) -> &DeviceLostFlag {
        &self.device_lost
    }
}

/// May a buffer mapped on the device `loss` watches be read now?
///
/// `Err` with the loss's reason when the device is lost: its buffers are
/// destroyed, and taking a mapped range of a destroyed buffer panics. Called
/// immediately before every `get_mapped_range` in this crate, after the
/// caller's last poll of the device (see the module docs). `None` (a device
/// nobody watches) always allows the read.
pub(crate) fn mapped_read_allowed(loss: Option<&DeviceLoss>) -> Result<(), String> {
    match loss.and_then(DeviceLoss::lost_reason) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// Poll `device` without blocking unless the device is known lost, which
/// wgpu would panic on. Returns false when the device is lost, so the caller
/// drops any read in flight instead of harvesting it.
///
/// A panic inside the poll is caught (`crate::gpu_wait::catch_poll`): when
/// the driver's fence wait or read fails, wgpu 29's `Device::poll` raises its
/// fatal error after wgpu-core's `lose()` has raised this flag, so that panic
/// is a loss already recorded. The catch has no effect on wasm, where a panic
/// aborts; the WebGPU backend's poll does nothing there, so nothing reaches it.
pub(crate) fn poll_unless_lost(device: &wgpu::Device, loss: &DeviceLoss) -> bool {
    if loss.is_lost() {
        return false;
    }
    let polled = crate::gpu_wait::catch_poll(|| {
        let _ = device.poll(wgpu::PollType::Poll);
    });
    polled.is_ok() && !loss.is_lost()
}

#[cfg(test)]
#[path = "device_lost_flag_tests.rs"]
mod tests;
