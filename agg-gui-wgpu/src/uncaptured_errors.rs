//! The window device's handler for wgpu errors nothing captured — agg-sharp's
//! present-failure containment (`PresentFailureContainmentTests`).
//!
//! wgpu reports a validation error that no error scope captured to the
//! device's uncaptured-error handler, and its default handler panics. So one
//! rejected submission — a custom renderer (`crate::custom_render`) or a
//! `gl_paint` painter recording an invalid copy, say — took the whole app
//! down, where agg-sharp loses that frame, reports the error and carries on.
//! [`crate::Gpu::new`] installs [`UncapturedErrors`] on the window's device:
//! each error is logged and counted, the frame it spoiled is lost, and the
//! next frame starts whole (`WgpuGfxCtx::reset` clears the save, clip and
//! layer stacks, so nothing a failed frame left behind carries over).
//!
//! The headless device ([`crate::HeadlessGpu`]) keeps wgpu's panicking
//! default, so a test that trips validation fails loudly; a test that means
//! to provoke errors installs this on its own device.

use std::sync::{Arc, Mutex};

/// What [`UncapturedErrors::install`] has seen on its device.
#[derive(Default)]
struct Seen {
    count: u64,
    last: Option<String>,
}

/// A device's uncaptured-error handler that logs and counts instead of
/// panicking. Cloning shares the same record.
#[derive(Clone, Default)]
pub struct UncapturedErrors {
    seen: Arc<Mutex<Seen>>,
}

impl UncapturedErrors {
    /// Install on `device`, replacing whatever handler it had (wgpu keeps one
    /// per device), and return the record the handler writes to.
    pub fn install(device: &wgpu::Device) -> Self {
        let errors = Self::default();
        let seen = Arc::clone(&errors.seen);
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let message = e.to_string();
            log::error!("agg-gui-wgpu: wgpu rejected GPU work; this frame is lost: {message}");
            if let Ok(mut seen) = seen.lock() {
                seen.count += 1;
                seen.last = Some(message);
            }
        }));
        errors
    }

    /// How many uncaptured errors the device has reported since `install`.
    pub fn count(&self) -> u64 {
        self.seen.lock().map(|s| s.count).unwrap_or(0)
    }

    /// The most recent error's text, if any.
    pub fn last(&self) -> Option<String> {
        self.seen.lock().ok().and_then(|s| s.last.clone())
    }
}
