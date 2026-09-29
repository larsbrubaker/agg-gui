//! Frame acquisition for [`Gpu`], and recovery from a failed
//! `Surface::configure`.
//!
//! Pulled in via `#[path]` as the `acquire` child module of `gpu.rs` so it
//! can reach `Gpu`'s private fields while keeping that file well under the
//! line limit.
//!
//! wgpu's default handler panics on a configure validation error, and DX12's
//! `ResizeBuffers` can raise one transiently (`DXGI_ERROR_INVALID_CALL`
//! while Windows is still settling a borderless-fullscreen window). Every
//! native configure therefore goes through [`try_configure`], which captures
//! the error instead. A failed configure leaves the swap chain unusable, so
//! [`Gpu::try_acquire_frame`] retries it, paced by the pure policy in
//! `crate::surface_retry`, and reports [`SurfaceError`] once the retry budget
//! ([`crate::GpuConfig::surface_retry_budget`]) is spent. The *caller*
//! decides how to wake for a retry — [`RetryWake`] — so this crate never
//! reaches into the event loop's draw scheduling on the modern path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::MutexGuard;
use std::time::Duration;

use web_time::Instant;

use super::{surface_acquire_action, Gpu, SurfaceAcquire};
use crate::surface_retry::{ConfigureRetry, RetryAction, RetryLog};

/// Result of [`Gpu::try_acquire_frame`].
#[derive(Debug)]
#[non_exhaustive]
pub enum FrameAcquire {
    /// A texture to render into and present.
    Frame(wgpu::SurfaceTexture),
    /// No texture this time — skip the frame, and arrange the wake the
    /// [`RetryWake`] asks for so the frame is retried.
    Skip(RetryWake),
}

/// When the caller should try [`Gpu::try_acquire_frame`] again after a
/// [`FrameAcquire::Skip`].
///
/// The caller owns the event loop, so it turns this into its own wake-up —
/// e.g. `Window::request_redraw` for [`RetryWake::Now`], a
/// `ControlFlow::WaitUntil` deadline for [`RetryWake::After`], nothing for
/// [`RetryWake::OnEvent`]. None of them should keep a reactive loop polling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum RetryWake {
    /// Retry on the next frame (a `Timeout`, a surface that is still stale
    /// right after a reconfigure, or the immediate first retry of a failed
    /// configure).
    Now,
    /// A failed configure is backing off: retry no sooner than this.
    After(Duration),
    /// Do not self-schedule a retry: wait for the next window event. The
    /// window is `Occluded` (a redraw request when it becomes visible again
    /// brings the frame back) or the acquire hit a validation error that a
    /// redraw loop would only repeat.
    OnEvent,
}

/// Why [`Gpu::try_acquire_frame`] could not produce frames any more.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum SurfaceError {
    /// The swap chain stayed unconfigured — every `Surface::configure`
    /// retry failing — for longer than
    /// [`crate::GpuConfig::surface_retry_budget`]. Returned again on every
    /// call until a configure succeeds (e.g. via [`Gpu::resize`]). The usual
    /// response is to exit with this error rather than keep a black window.
    #[non_exhaustive]
    ConfigureRetriesExhausted {
        /// wgpu's text for the most recent failure.
        last_error: String,
        /// From the first failure of the run to the most recent one.
        unconfigured_for: Duration,
        /// Consecutive failed configure attempts.
        attempts: u32,
    },
}

impl std::fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigureRetriesExhausted {
                last_error,
                unconfigured_for,
                attempts,
            } => write!(
                f,
                "wgpu surface could not be configured for {:.1} s \
                 ({attempts} failed attempts); last error: {last_error}",
                unconfigured_for.as_secs_f64()
            ),
        }
    }
}

impl std::error::Error for SurfaceError {}

/// Configure state behind [`Gpu`]'s mutex: the retry policy, plus the text
/// of the latest failure for logging and [`SurfaceError`].
#[derive(Debug)]
pub(super) struct RetryState {
    policy: ConfigureRetry,
    last_error: String,
}

impl RetryState {
    pub(super) fn new(policy: ConfigureRetry) -> Self {
        Self {
            policy,
            last_error: String::new(),
        }
    }
}

/// `Surface::configure` with its validation and internal errors captured
/// instead of handed to wgpu's default handler, which panics.
///
/// A device lost before or during the configure also counts as a failure:
/// wgpu-core reports that as a `DeviceLost` error, which the `Validation`
/// scope does not capture, so the surface could look configured when it is
/// not — and acquiring from it would be fatal. Not unit-tested: it needs a
/// live device that can be made to fail on demand; the policy that consumes
/// the result is tested in `surface_retry`.
pub(super) fn try_configure(
    device: &wgpu::Device,
    surface: &wgpu::Surface<'_>,
    config: &wgpu::SurfaceConfiguration,
    device_lost: &AtomicBool,
) -> Result<(), String> {
    // Scopes nest: pop in reverse push order. A backend failure inside the
    // swap-chain call (e.g. DXGI refusing the resize) can surface as either
    // kind, so both are captured and either is a failed configure.
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    surface.configure(device, config);
    let internal_err = pollster::block_on(internal.pop());
    let validation_err = pollster::block_on(validation.pop());
    if let Some(err) = validation_err.or(internal_err) {
        return Err(err.to_string());
    }
    if device_lost.load(Ordering::Relaxed) {
        return Err("device lost".to_owned());
    }
    Ok(())
}

/// The wake for a surface-acquire status that yields no frame and needs no
/// reconfigure; `None` for the statuses that do (`Present`, `Reconfigure`).
/// Preserves the long-standing semantics: `Timeout` asks for another frame,
/// `Occluded`/`Validation` wait for an event.
fn status_wake(action: SurfaceAcquire) -> Option<RetryWake> {
    match action {
        SurfaceAcquire::SkipAndRetry => Some(RetryWake::Now),
        SurfaceAcquire::Skip => Some(RetryWake::OnEvent),
        SurfaceAcquire::Present | SurfaceAcquire::Reconfigure => None,
    }
}

/// What an unconfigured surface's retry decision means for the caller:
/// skip with a wake, or give up. `Configured` cannot reach here (the caller
/// acquires instead); it maps to an immediate retry to stay total.
fn unconfigured_outcome(
    action: RetryAction,
    state: &RetryState,
) -> Result<FrameAcquire, SurfaceError> {
    let wake = match action {
        RetryAction::Configured | RetryAction::TryNow => RetryWake::Now,
        RetryAction::WaitFor(d) => RetryWake::After(d),
        RetryAction::GiveUp { unconfigured_for } => {
            return Err(SurfaceError::ConfigureRetriesExhausted {
                last_error: state.last_error.clone(),
                unconfigured_for,
                attempts: state.policy.failures(),
            })
        }
    };
    Ok(FrameAcquire::Skip(wake))
}

impl Gpu {
    fn retry_state(&self) -> MutexGuard<'_, RetryState> {
        // Poisoning would need a panic inside the few lines that hold the
        // lock; the state is plain data either way, so keep using it.
        self.retry.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Configure the surface with the current config, fold the outcome into
    /// the retry state and log it. Returns whether the surface is now
    /// configured.
    ///
    /// Logging: `warn` on the first failure of a run (with the size, since
    /// that is what the backend rejected), `debug` on repeats so a
    /// persistently failing surface does not flood the log, `info` on
    /// recovery.
    pub(super) fn configure_and_record(&self, now: Instant) -> bool {
        let result = try_configure(&self.device, &self.surface, &self.config, &self.device_lost);
        let ok = result.is_ok();
        let mut state = self.retry_state();
        let log = state.policy.record(ok, now);
        state.last_error = result.err().unwrap_or_default();
        let (w, h) = (self.config.width, self.config.height);
        let err = &state.last_error;
        match log {
            RetryLog::None => {}
            RetryLog::FirstFailure => log::warn!(
                "agg-gui-wgpu: surface configure at {w}x{h} failed, retrying with backoff \
                 (budget {:?}): {err}",
                self.retry_budget
            ),
            RetryLog::RepeatFailure => log::debug!(
                "agg-gui-wgpu: surface configure at {w}x{h} failed again ({} in a row): {err}",
                state.policy.failures()
            ),
            RetryLog::Recovered { failures } => log::info!(
                "agg-gui-wgpu: surface configured at {w}x{h} after {failures} failed attempt(s)"
            ),
        }
        ok
    }

    /// Is the swap chain currently configured?
    ///
    /// `false` while a configure-failure run is in progress: a
    /// `Surface::configure` (in [`Self::resize`] or [`Self::try_acquire_frame`])
    /// failed and no retry has succeeded yet, so [`Self::try_acquire_frame`]
    /// will skip frames until one does. A shell can use this to hold off
    /// attempts that cannot succeed — agg-gui-shell stops painting a
    /// minimized window only while this is `false`. Read-only; `true` again
    /// as soon as a configure succeeds.
    pub fn surface_configured(&self) -> bool {
        self.retry_state().policy.is_configured()
    }

    /// The skip-or-give-up outcome for a surface that is not configured.
    fn skip_unconfigured(&self, now: Instant) -> Result<FrameAcquire, SurfaceError> {
        let state = self.retry_state();
        unconfigured_outcome(state.policy.attempt(now, self.retry_budget), &state)
    }

    /// Acquire the next surface texture.
    ///
    /// - A stale swap chain (`Outdated`/`Lost`) is reconfigured and the
    ///   acquire retried once, this call.
    /// - A swap chain left unconfigured by a failed configure (here or in
    ///   [`Self::resize`]) is configured again first, paced by a backoff: the
    ///   first retry is immediate, later ones wait 50 ms doubling to 1 s, and
    ///   a call made before the next attempt is due skips without calling
    ///   `configure` (each attempt costs a full GPU wait-idle).
    /// - Otherwise the frame is skipped with the [`RetryWake`] the caller
    ///   should arrange; see [`FrameAcquire`].
    ///
    /// # Errors
    ///
    /// [`SurfaceError::ConfigureRetriesExhausted`] once a configure retry
    /// fails after the surface has been unconfigured for longer than
    /// [`crate::GpuConfig::surface_retry_budget`] *and* at least 12 attempts
    /// in the run have failed.
    ///
    /// While the window is minimized and [`Self::surface_configured`] is
    /// `false`, don't call this: every configure retry against a minimized
    /// window fails and counts against the budget (agg-gui-shell skips the
    /// paint in exactly that state). The budget clock still runs from the
    /// run's first failure while attempts are paused, so it is the attempt
    /// minimum that gives a restored window a real retry run (about 6.5 s)
    /// instead of an exit at its first failed retry.
    ///
    /// A lost device also fails configure; shells should check
    /// [`Self::device_lost`] before each frame and rebuild the `Gpu`, which
    /// starts with a fresh retry state, rather than let the budget run out.
    pub fn try_acquire_frame(&self) -> Result<FrameAcquire, SurfaceError> {
        use wgpu::CurrentSurfaceTexture as T;
        let now = Instant::now();
        // Bound first: a guard temporary in the `match` scrutinee would be
        // held across `configure_and_record`, which locks again.
        let action = self.retry_state().policy.attempt(now, self.retry_budget);
        match action {
            RetryAction::Configured => {}
            RetryAction::TryNow => {
                if !self.configure_and_record(now) {
                    return self.skip_unconfigured(now);
                }
            }
            RetryAction::WaitFor(_) | RetryAction::GiveUp { .. } => {
                return self.skip_unconfigured(now);
            }
        }
        let first = self.surface.get_current_texture();
        let action = surface_acquire_action(&first);
        if let Some(wake) = status_wake(action) {
            return Ok(FrameAcquire::Skip(wake));
        }
        match action {
            SurfaceAcquire::Reconfigure => {
                // The config already carries the current (nonzero,
                // `resize`-clamped) size, so this just re-binds. A failure
                // starts a retry run like any other failed configure.
                if !self.configure_and_record(now) {
                    return self.skip_unconfigured(now);
                }
                match self.surface.get_current_texture() {
                    T::Success(f) | T::Suboptimal(f) => Ok(FrameAcquire::Frame(f)),
                    // Still nothing after the reconfigure: come back next
                    // frame rather than wait for an unrelated event.
                    _ => Ok(FrameAcquire::Skip(RetryWake::Now)),
                }
            }
            _ => match first {
                T::Success(f) | T::Suboptimal(f) => Ok(FrameAcquire::Frame(f)),
                _ => Ok(FrameAcquire::Skip(RetryWake::Now)),
            },
        }
    }

    /// Legacy form of [`Self::try_acquire_frame`], kept for existing callers.
    /// Prefer `try_acquire_frame`; this will be deprecated in 0.6.
    ///
    /// Returns `None` when the frame must be skipped, and arranges the retry
    /// itself: [`RetryWake::Now`] calls `request_redraw` (pass a closure
    /// that calls `Window::request_redraw`), [`RetryWake::After`] schedules
    /// [`agg_gui::animation::request_draw_after`] — agg_gui's thread-local
    /// draw state, so call this on the UI thread — and
    /// [`RetryWake::OnEvent`] does nothing.
    ///
    /// # Panics
    ///
    /// When [`Self::try_acquire_frame`] returns [`SurfaceError`] — the
    /// surface could not be configured for the whole retry budget. Before the
    /// retry existed the first failed configure panicked inside wgpu, so this
    /// keeps crash-and-restart supervision working; handle the error instead
    /// by moving to `try_acquire_frame`.
    pub fn acquire_frame(&self, request_redraw: impl Fn()) -> Option<wgpu::SurfaceTexture> {
        match self.try_acquire_frame() {
            Ok(FrameAcquire::Frame(frame)) => Some(frame),
            Ok(FrameAcquire::Skip(wake)) => {
                match wake {
                    RetryWake::Now => request_redraw(),
                    RetryWake::After(d) => agg_gui::animation::request_draw_after(d),
                    RetryWake::OnEvent => {}
                }
                None
            }
            Err(e) => panic!("agg-gui-wgpu: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_stays_send_and_sync() {
        // The retry state lives behind `&self`; a `Cell` there would silently
        // make `Gpu` `!Sync` — a breaking change for apps that share it.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Gpu>();
    }

    #[test]
    fn skip_statuses_keep_their_wake_semantics() {
        // Timeout is transient: ask for another frame. Occluded/Validation:
        // a self-requested redraw would just burn the CPU.
        assert_eq!(
            status_wake(SurfaceAcquire::SkipAndRetry),
            Some(RetryWake::Now)
        );
        assert_eq!(status_wake(SurfaceAcquire::Skip), Some(RetryWake::OnEvent));
        assert_eq!(status_wake(SurfaceAcquire::Present), None);
        assert_eq!(status_wake(SurfaceAcquire::Reconfigure), None);
    }

    fn state(failures: u32, last_error: &str) -> RetryState {
        let mut policy = ConfigureRetry::default();
        for _ in 0..failures {
            policy.record(false, Instant::now());
        }
        RetryState {
            policy,
            last_error: last_error.to_owned(),
        }
    }

    #[test]
    fn backing_off_surface_skips_with_the_matching_wake() {
        let s = state(1, "");
        assert!(matches!(
            unconfigured_outcome(RetryAction::TryNow, &s),
            Ok(FrameAcquire::Skip(RetryWake::Now))
        ));
        let d = Duration::from_millis(200);
        assert!(matches!(
            unconfigured_outcome(RetryAction::WaitFor(d), &s),
            Ok(FrameAcquire::Skip(RetryWake::After(got))) if got == d
        ));
    }

    #[test]
    fn exhausted_budget_is_an_error_carrying_the_last_wgpu_text() {
        let s = state(7, "DXGI_ERROR_INVALID_CALL");
        let span = Duration::from_millis(10_500);
        let err = match unconfigured_outcome(
            RetryAction::GiveUp {
                unconfigured_for: span,
            },
            &s,
        ) {
            Err(e) => e,
            Ok(other) => panic!("expected an error, got {other:?}"),
        };
        assert_eq!(
            err,
            SurfaceError::ConfigureRetriesExhausted {
                last_error: "DXGI_ERROR_INVALID_CALL".to_owned(),
                unconfigured_for: span,
                attempts: 7,
            }
        );
        let text = err.to_string();
        assert!(text.contains("10.5 s"), "{text}");
        assert!(text.contains("7 failed attempts"), "{text}");
        assert!(text.contains("DXGI_ERROR_INVALID_CALL"), "{text}");
    }
}
