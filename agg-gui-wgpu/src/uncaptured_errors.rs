//! The window device's handler for wgpu errors nothing captured — agg-sharp's
//! present-failure containment (`PresentFailureContainmentTests`).
//!
//! wgpu reports a validation error that no error scope captured to the
//! device's uncaptured-error handler, and its default handler panics. So one
//! rejected submission — a custom renderer (`crate::custom_render`) or a
//! `gl_paint` painter recording an invalid copy, say — took the whole app
//! down, where agg-sharp loses that frame, reports the error and carries on.
//! `Gpu::new` (native windows) and agg-gui-web-shell (browser canvases)
//! install [`UncapturedErrors`] on the window's device: each error is logged and counted, the frame it spoiled is lost, and the
//! next frame starts whole (`WgpuGfxCtx::reset` clears the save, clip and
//! layer stacks, so nothing a failed frame left behind carries over).
//!
//! The headless device ([`crate::HeadlessGpu`]) keeps wgpu's panicking
//! default, so a test that trips validation fails loudly; a test that means
//! to provoke errors installs this on its own device.
//!
//! A shell also hands the record to its [`WgpuGfxCtx`] with
//! [`WgpuGfxCtx::set_uncaptured_errors`], and `end_frame` passes the count to
//! every custom render pass as
//! [`crate::WgpuCustomRenderCtx::lost_frames`]: a 3-D view that keeps its
//! finished picture between frames learns from it that a frame it recorded
//! into was thrown away, so the picture it kept never reached its texture.

use std::sync::{Arc, Mutex};

use crate::WgpuGfxCtx;

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
        Self::install_reporting(device, |message| {
            log::error!("agg-gui-wgpu: wgpu rejected GPU work; this frame is lost: {message}");
        })
    }

    /// [`Self::install`] with the caller's own report of each error's text
    /// instead of `log::error!` — the web shell writes to the browser
    /// console, which is where a page's GPU errors are looked for. The error
    /// is counted the same way.
    pub fn install_reporting(
        device: &wgpu::Device,
        report: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let errors = Self::default();
        let seen = Arc::clone(&errors.seen);
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let message = e.to_string();
            report(&message);
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

impl WgpuGfxCtx {
    /// Record the uncaptured-error record of this context's device, so every
    /// custom render pass sees its count as
    /// [`crate::WgpuCustomRenderCtx::lost_frames`]. Call it again with the new
    /// device's record when a device-loss rebuild keeps the context.
    pub fn set_uncaptured_errors(&mut self, errors: &UncapturedErrors) {
        self.uncaptured_errors = Some(errors.clone());
    }

    /// [`Self::set_uncaptured_errors`] in builder form, for the line that
    /// makes the context.
    pub fn with_uncaptured_errors(mut self, errors: &UncapturedErrors) -> Self {
        self.set_uncaptured_errors(errors);
        self
    }

    /// How many frames this context's device has lost to rejected GPU work:
    /// the recorded [`UncapturedErrors::count`], or 0 when the context's
    /// creator recorded none.
    pub fn lost_frames(&self) -> u64 {
        self.uncaptured_errors
            .as_ref()
            .map_or(0, UncapturedErrors::count)
    }
}
