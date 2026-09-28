//! Retry / backoff policy for a swap chain whose `Surface::configure` failed.
//!
//! [`crate::gpu::Gpu`] captures configure validation errors instead of letting
//! wgpu panic (DX12 `ResizeBuffers` can reject a borderless-fullscreen window
//! that Windows is still settling). A failed configure leaves the surface
//! unusable until a later configure succeeds, so `Gpu` retries — and this
//! module decides *when*, and what each outcome is worth logging.
//!
//! Every configure attempt costs a full GPU wait-idle inside wgpu-core, so a
//! persistently failing surface must not be retried every frame: the first
//! retry is immediate — the very next attempt, which is in the same paint
//! when the failure came from a resize applied at the top of it — and later
//! ones back off exponentially up to a cap. Pure and time-injected — no wgpu
//! types, `now` is passed in — so the policy is unit-tested without a GPU.

use std::time::Duration;
use web_time::Instant;

/// Delay before the second retry. The first retry is immediate: the next
/// attempt runs as soon as one is made, in the same paint if a resize at the
/// top of that paint was what failed.
pub(crate) const FIRST_BACKOFF: Duration = Duration::from_millis(50);
/// Upper bound on the delay between retries of a persistently failing surface.
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(1);

/// What the caller should do about the surface before acquiring a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RetryAction {
    /// The surface is configured — acquire as normal.
    Configured,
    /// Unconfigured and a retry is due — configure now.
    TryNow,
    /// Unconfigured and backing off — skip the frame and wake again after
    /// this long, without calling `configure`.
    WaitFor(Duration),
}

/// What a configure outcome is worth logging. The caller maps these to
/// levels: `warn!` for the first failure only, `debug!` for repeats (so a
/// persistently failing surface does not flood the log), `info!` on recovery.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RetryLog {
    /// Nothing noteworthy — a success with no preceding failures.
    None,
    /// The first failure after the surface was last configured.
    FirstFailure,
    /// Another failure in the same run of failures.
    RepeatFailure,
    /// A success ending a run of `failures` consecutive failures.
    Recovered { failures: u32 },
}

/// Configure state of one swap chain: whether it is configured, and if not,
/// how many attempts have failed in a row and when the next one is due.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct ConfigureRetry {
    configured: bool,
    failures: u32,
    /// `None` = the next attempt may run immediately.
    next_attempt: Option<Instant>,
}

impl ConfigureRetry {
    pub(crate) fn is_configured(&self) -> bool {
        self.configured
    }

    /// Consecutive failed attempts since the surface was last configured.
    pub(crate) fn failures(&self) -> u32 {
        self.failures
    }

    /// Decide what to do at `now`.
    pub(crate) fn attempt(&self, now: Instant) -> RetryAction {
        if self.configured {
            return RetryAction::Configured;
        }
        match self.next_attempt {
            Some(due) if now < due => RetryAction::WaitFor(due - now),
            _ => RetryAction::TryNow,
        }
    }

    /// Record the outcome of a configure attempt made at `now`, returning
    /// what it is worth logging. A success resets all backoff state.
    pub(crate) fn record(&mut self, ok: bool, now: Instant) -> RetryLog {
        if ok {
            let failures = self.failures;
            *self = Self {
                configured: true,
                failures: 0,
                next_attempt: None,
            };
            return match failures {
                0 => RetryLog::None,
                failures => RetryLog::Recovered { failures },
            };
        }
        self.configured = false;
        self.failures = self.failures.saturating_add(1);
        let delay = backoff_after(self.failures);
        self.next_attempt = (!delay.is_zero()).then(|| now + delay);
        if self.failures == 1 {
            RetryLog::FirstFailure
        } else {
            RetryLog::RepeatFailure
        }
    }
}

/// Delay to wait after the `failures`-th consecutive failure before trying
/// again: zero after the first, then [`FIRST_BACKOFF`] doubling per failure,
/// capped at [`MAX_BACKOFF`].
pub(crate) fn backoff_after(failures: u32) -> Duration {
    if failures <= 1 {
        return Duration::ZERO;
    }
    // failures = 2 → FIRST_BACKOFF × 1; the shift is bounded well before it
    // could overflow, and anything past the cap is the cap anyway.
    let doublings = (failures - 2).min(16);
    FIRST_BACKOFF
        .saturating_mul(1u32 << doublings)
        .min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A state that has just failed `n` times in a row, the last at `now`.
    fn failed(n: u32, now: Instant) -> ConfigureRetry {
        let mut r = ConfigureRetry::default();
        for _ in 0..n {
            r.record(false, now);
        }
        r
    }

    #[test]
    fn configured_surface_needs_no_attempt() {
        let now = Instant::now();
        let mut r = ConfigureRetry::default();
        assert_eq!(r.record(true, now), RetryLog::None);
        assert!(r.is_configured());
        assert_eq!(r.attempt(now), RetryAction::Configured);
    }

    #[test]
    fn never_configured_surface_tries_now() {
        let r = ConfigureRetry::default();
        assert!(!r.is_configured());
        assert_eq!(r.attempt(Instant::now()), RetryAction::TryNow);
    }

    #[test]
    fn first_retry_is_immediate() {
        // The transient DX12 case: Windows is still settling the window, and
        // the very next frame's configure usually succeeds.
        let now = Instant::now();
        let r = failed(1, now);
        assert_eq!(r.attempt(now), RetryAction::TryNow);
    }

    #[test]
    fn backoff_doubles_from_first_backoff_and_caps() {
        assert_eq!(backoff_after(0), Duration::ZERO);
        assert_eq!(backoff_after(1), Duration::ZERO);
        assert_eq!(backoff_after(2), FIRST_BACKOFF);
        assert_eq!(backoff_after(3), ms(100));
        assert_eq!(backoff_after(4), ms(200));
        assert_eq!(backoff_after(5), ms(400));
        assert_eq!(backoff_after(6), ms(800));
        assert_eq!(backoff_after(7), MAX_BACKOFF);
        assert_eq!(backoff_after(8), MAX_BACKOFF);
        // No overflow however long the surface stays broken.
        assert_eq!(backoff_after(u32::MAX), MAX_BACKOFF);
    }

    #[test]
    fn waits_before_the_deadline_and_tries_after_it() {
        let t0 = Instant::now();
        let r = failed(3, t0); // third failure → 100 ms
        assert_eq!(r.attempt(t0), RetryAction::WaitFor(ms(100)));
        assert_eq!(r.attempt(t0 + ms(40)), RetryAction::WaitFor(ms(60)));
        assert_eq!(r.attempt(t0 + ms(100)), RetryAction::TryNow);
        assert_eq!(r.attempt(t0 + ms(500)), RetryAction::TryNow);
    }

    #[test]
    fn deadline_is_measured_from_the_latest_failure() {
        let t0 = Instant::now();
        let mut r = failed(1, t0);
        let t1 = t0 + ms(16);
        r.record(false, t1); // second failure → 50 ms from t1
        assert_eq!(r.attempt(t1 + ms(10)), RetryAction::WaitFor(ms(40)));
        assert_eq!(r.attempt(t1 + ms(50)), RetryAction::TryNow);
    }

    #[test]
    fn first_failure_is_logged_once_then_repeats() {
        let now = Instant::now();
        let mut r = ConfigureRetry::default();
        r.record(true, now);
        assert_eq!(r.record(false, now), RetryLog::FirstFailure);
        assert_eq!(r.record(false, now), RetryLog::RepeatFailure);
        assert_eq!(r.record(false, now), RetryLog::RepeatFailure);
        assert_eq!(r.failures(), 3);
        assert!(!r.is_configured());
    }

    #[test]
    fn initial_failure_is_a_first_failure() {
        let mut r = ConfigureRetry::default();
        assert_eq!(r.record(false, Instant::now()), RetryLog::FirstFailure);
    }

    #[test]
    fn recovery_reports_the_failure_count_and_resets() {
        let t0 = Instant::now();
        let mut r = failed(5, t0);
        assert_eq!(r.record(true, t0), RetryLog::Recovered { failures: 5 });
        assert!(r.is_configured());
        assert_eq!(r.failures(), 0);
        assert_eq!(r.attempt(t0), RetryAction::Configured);
        // A later failure starts a fresh run: logged as first, retried at once.
        assert_eq!(r.record(false, t0), RetryLog::FirstFailure);
        assert_eq!(r.attempt(t0), RetryAction::TryNow);
    }
}
