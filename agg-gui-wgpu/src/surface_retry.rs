//! Retry / backoff policy for a swap chain whose `Surface::configure` failed.
//!
//! [`crate::gpu::Gpu`] captures configure validation errors instead of letting
//! wgpu panic (DX12 `ResizeBuffers` can reject a borderless-fullscreen window
//! that Windows is still settling). A failed configure leaves the surface
//! unusable until a later configure succeeds, so `Gpu` retries — and this
//! module decides *when*, *when to give up*, and what each outcome is worth
//! logging.
//!
//! Every configure attempt costs a full GPU wait-idle inside wgpu-core, so a
//! persistently failing surface must not be retried every frame: the first
//! retry is immediate — the very next attempt, which is in the same paint
//! when the failure came from a resize applied at the top of it — and later
//! ones back off exponentially up to a cap. A surface that stays unconfigured
//! for longer than the retry budget, over at least a minimum number of failed
//! attempts, is given up on, so the app can exit with an error instead of
//! showing a black window forever.
//!
//! Pure and time-injected — no wgpu types, `now` is passed in — so the policy
//! is unit-tested without a GPU.

use std::time::Duration;
use web_time::Instant;

/// Delay before the second retry. The first retry is immediate: the next
/// attempt runs as soon as one is made, in the same paint if a resize at the
/// top of that paint was what failed.
pub(crate) const FIRST_BACKOFF: Duration = Duration::from_millis(50);
/// Upper bound on the delay between retries of a persistently failing surface.
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(1);
/// Default for how long a surface may stay continuously unconfigured before
/// the retries are given up on. See `GpuConfig::with_surface_retry_budget`.
pub(crate) const DEFAULT_RETRY_BUDGET: Duration = Duration::from_secs(10);
/// Fewest consecutive failed configure attempts before a run may be given
/// up on, whatever the elapsed time.
///
/// Elapsed time alone is a poor witness whenever the caller pauses attempts.
/// agg-gui-shell stops painting — so stops configuring — while its window is
/// minimized with a failure run in progress, which keeps a minimized window
/// from burning attempts; but the
/// budget clock keeps running from the run's first failure, so a window that
/// failed twice and then sat minimized past the budget would be given up on
/// at its first failed retry after restore. The attempt minimum is what
/// prevents that: after any pause the surface still gets several seconds of
/// real retrying. 12 is what continuous
/// retrying at this module's cadence (immediate, then 50 ms doubling to the
/// 1 s cap) reaches after about 6.5 s — below the 10 s default budget, so for
/// a surface retried the whole time the budget stays the binding limit, while
/// a shorter custom budget gets a floor of roughly 6.5 s of actual retrying.
pub(crate) const MIN_FAILURES_TO_GIVE_UP: u32 = 12;

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
    /// A configure attempt failed more than the budget after the run's first
    /// failure, and at least [`MIN_FAILURES_TO_GIVE_UP`] attempts have
    /// failed. Stop retrying and report; sticky until a configure succeeds.
    GiveUp {
        /// From the first failure of the run to the latest one.
        unconfigured_for: Duration,
    },
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
/// how many attempts have failed in a row, when the run of failures started,
/// and when the next attempt is due.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct ConfigureRetry {
    configured: bool,
    failures: u32,
    /// When the current run of failures began; `None` while configured.
    first_failure: Option<Instant>,
    /// When the latest failure in the run happened.
    last_failure: Option<Instant>,
    /// `None` = the next attempt may run immediately.
    next_attempt: Option<Instant>,
}

impl ConfigureRetry {
    /// State of a swap chain whose initial configure succeeded.
    pub(crate) fn configured() -> Self {
        Self {
            configured: true,
            ..Self::default()
        }
    }

    pub(crate) fn is_configured(&self) -> bool {
        self.configured
    }

    /// Consecutive failed attempts since the surface was last configured.
    pub(crate) fn failures(&self) -> u32 {
        self.failures
    }

    /// Decide what to do at `now`. Gives up only when BOTH hold: a failed
    /// attempt landed more than `budget` after the first failure of the run,
    /// and at least [`MIN_FAILURES_TO_GIVE_UP`] attempts have failed.
    ///
    /// The budget is judged by *failed attempts*, not by the clock alone: a
    /// surface nobody tried to configure for a while (the caller paused, e.g.
    /// agg-gui-shell while its window is minimized mid-run) is retried at the normal
    /// cadence afterwards until it has failed often enough. `Duration::MAX` never gives up.
    pub(crate) fn attempt(&self, now: Instant, budget: Duration) -> RetryAction {
        if self.configured {
            return RetryAction::Configured;
        }
        if let (Some(first), Some(last)) = (self.first_failure, self.last_failure) {
            let unconfigured_for = last.saturating_duration_since(first);
            if unconfigured_for > budget && self.failures >= MIN_FAILURES_TO_GIVE_UP {
                return RetryAction::GiveUp { unconfigured_for };
            }
        }
        match self.next_attempt {
            Some(due) if now < due => RetryAction::WaitFor(due - now),
            _ => RetryAction::TryNow,
        }
    }

    /// Record the outcome of a configure attempt made at `now`, returning
    /// what it is worth logging. A success resets all backoff state,
    /// including the budget clock.
    pub(crate) fn record(&mut self, ok: bool, now: Instant) -> RetryLog {
        if ok {
            let failures = self.failures;
            *self = Self::configured();
            return match failures {
                0 => RetryLog::None,
                failures => RetryLog::Recovered { failures },
            };
        }
        self.configured = false;
        self.failures = self.failures.saturating_add(1);
        self.first_failure.get_or_insert(now);
        self.last_failure = Some(now);
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

    /// Budget large enough that the backoff tests never hit it.
    const NO_LIMIT: Duration = Duration::MAX;

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
        assert_eq!(r.attempt(now, NO_LIMIT), RetryAction::Configured);
        assert_eq!(ConfigureRetry::configured(), r);
    }

    #[test]
    fn never_configured_surface_tries_now() {
        let r = ConfigureRetry::default();
        assert!(!r.is_configured());
        assert_eq!(r.attempt(Instant::now(), NO_LIMIT), RetryAction::TryNow);
    }

    #[test]
    fn first_retry_is_immediate() {
        // The transient DX12 case: Windows is still settling the window, and
        // the very next frame's configure usually succeeds.
        let now = Instant::now();
        let r = failed(1, now);
        assert_eq!(r.attempt(now, NO_LIMIT), RetryAction::TryNow);
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
        assert_eq!(r.attempt(t0, NO_LIMIT), RetryAction::WaitFor(ms(100)));
        assert_eq!(
            r.attempt(t0 + ms(40), NO_LIMIT),
            RetryAction::WaitFor(ms(60))
        );
        assert_eq!(r.attempt(t0 + ms(100), NO_LIMIT), RetryAction::TryNow);
        assert_eq!(r.attempt(t0 + ms(500), NO_LIMIT), RetryAction::TryNow);
    }

    #[test]
    fn deadline_is_measured_from_the_latest_failure() {
        let t0 = Instant::now();
        let mut r = failed(1, t0);
        let t1 = t0 + ms(16);
        r.record(false, t1); // second failure → 50 ms from t1
        assert_eq!(
            r.attempt(t1 + ms(10), NO_LIMIT),
            RetryAction::WaitFor(ms(40))
        );
        assert_eq!(r.attempt(t1 + ms(50), NO_LIMIT), RetryAction::TryNow);
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
        assert_eq!(r.attempt(t0, NO_LIMIT), RetryAction::Configured);
        // A later failure starts a fresh run: logged as first, retried at once.
        assert_eq!(r.record(false, t0), RetryLog::FirstFailure);
        assert_eq!(r.attempt(t0, NO_LIMIT), RetryAction::TryNow);
    }

    /// Keep retrying at exactly the cadence the policy asks for, every
    /// attempt failing, until it gives up (or `max_attempts` runs out).
    /// Returns the time of the final decision and the decision.
    fn fail_at_cadence(
        r: &mut ConfigureRetry,
        mut now: Instant,
        budget: Duration,
        max_attempts: u32,
    ) -> (Instant, RetryAction) {
        for _ in 0..max_attempts {
            match r.attempt(now, budget) {
                RetryAction::TryNow => {
                    r.record(false, now);
                }
                RetryAction::WaitFor(d) => now += d,
                other => return (now, other),
            }
        }
        (now, r.attempt(now, budget))
    }

    #[test]
    fn gives_up_once_a_failure_lands_past_the_budget() {
        // A surface that never comes back must end in an error the app can
        // exit on, not a black window forever. Enough attempts have failed
        // here, so the time budget is what decides.
        let budget = Duration::from_secs(10);
        let t0 = Instant::now();
        let mut r = failed(MIN_FAILURES_TO_GIVE_UP - 1, t0);
        // Failing right up to the budget keeps retrying.
        r.record(false, t0 + budget);
        assert!(matches!(
            r.attempt(t0 + budget, budget),
            RetryAction::TryNow | RetryAction::WaitFor(_)
        ));
        // The first failure beyond it gives up, reporting the unconfigured span.
        let late = t0 + budget + ms(1);
        r.record(false, late);
        assert_eq!(
            r.attempt(late, budget),
            RetryAction::GiveUp {
                unconfigured_for: budget + ms(1)
            }
        );
        // Sticky: later calls keep reporting instead of retrying.
        assert!(matches!(
            r.attempt(late + budget, budget),
            RetryAction::GiveUp { .. }
        ));
    }

    #[test]
    fn failure_count_alone_does_not_give_up() {
        // Many fast failures inside the budget (a drag-resize over a broken
        // surface configures on every step) are not grounds to give up.
        let budget = Duration::from_secs(10);
        let t0 = Instant::now();
        let mut r = failed(MIN_FAILURES_TO_GIVE_UP * 3, t0);
        r.record(false, t0 + budget / 2);
        assert!(!matches!(
            r.attempt(t0 + budget / 2, budget),
            RetryAction::GiveUp { .. }
        ));
    }

    #[test]
    fn minimized_window_is_not_given_up_on_its_first_retry_after_restore() {
        // Two failures, then the window sits minimized (no paints, no
        // attempts) far past the budget. The first retry after restore fails
        // too: that is one data point, not ten seconds of trying, so it must
        // not exit the app.
        let budget = Duration::from_secs(10);
        let t0 = Instant::now();
        let mut r = failed(2, t0);
        let restored = t0 + budget * 6;
        assert_eq!(r.attempt(restored, budget), RetryAction::TryNow);
        r.record(false, restored);
        assert!(
            !matches!(r.attempt(restored, budget), RetryAction::GiveUp { .. }),
            "one failed retry after a long idle must not give up"
        );
        // Still failing at the normal backoff cadence: it does give up, once
        // the minimum number of failed attempts is reached — and well before
        // another full budget has passed.
        let (gave_up_at, action) = fail_at_cadence(&mut r, restored, budget, 1000);
        assert!(
            matches!(action, RetryAction::GiveUp { .. }),
            "a surface that keeps failing is eventually given up on: {action:?}"
        );
        assert_eq!(r.failures(), MIN_FAILURES_TO_GIVE_UP);
        assert!(gave_up_at - restored < budget);
    }

    #[test]
    fn continuous_retrying_gives_up_at_about_the_budget() {
        // From a fresh failure, retried at the policy's own cadence, the
        // default budget (not the attempt minimum) is what ends the run —
        // within one max backoff of it.
        let budget = DEFAULT_RETRY_BUDGET;
        let t0 = Instant::now();
        let mut r = failed(1, t0);
        let (gave_up_at, action) = fail_at_cadence(&mut r, t0, budget, 1000);
        assert!(matches!(action, RetryAction::GiveUp { .. }), "{action:?}");
        let elapsed = gave_up_at - t0;
        assert!(
            elapsed > budget && elapsed <= budget + MAX_BACKOFF,
            "gave up after {elapsed:?}"
        );
        assert!(r.failures() >= MIN_FAILURES_TO_GIVE_UP);
    }

    #[test]
    fn budget_needs_a_failed_attempt_not_just_elapsed_time() {
        // Nobody attempted a configure for a long while (minimized window, no
        // paints): the surface still gets another attempt before giving up.
        let budget = Duration::from_secs(10);
        let t0 = Instant::now();
        let r = failed(MIN_FAILURES_TO_GIVE_UP, t0);
        assert_eq!(
            r.attempt(t0 + budget * 6, budget),
            RetryAction::TryNow,
            "elapsed time alone must not give up"
        );
    }

    #[test]
    fn budget_is_measured_from_the_first_failure_of_the_run() {
        let budget = Duration::from_secs(10);
        let t0 = Instant::now();
        // A run that recovered does not count against the next one — even
        // with enough failures in the new run that only time can decide.
        let mut r = failed(3, t0);
        r.record(true, t0 + ms(500));
        let t1 = t0 + Duration::from_secs(9);
        for _ in 0..MIN_FAILURES_TO_GIVE_UP {
            r.record(false, t1);
        }
        r.record(false, t1 + Duration::from_secs(9));
        assert!(
            !matches!(
                r.attempt(t1 + Duration::from_secs(9), budget),
                RetryAction::GiveUp { .. }
            ),
            "18 s after the first ever failure, but only 9 s into this run"
        );
    }

    #[test]
    fn success_after_giving_up_resets_everything() {
        let budget = Duration::from_secs(1);
        let t0 = Instant::now();
        let mut r = failed(MIN_FAILURES_TO_GIVE_UP, t0);
        r.record(false, t0 + Duration::from_secs(2));
        assert!(matches!(
            r.attempt(t0 + Duration::from_secs(2), budget),
            RetryAction::GiveUp { .. }
        ));
        let t1 = t0 + Duration::from_secs(3);
        assert_eq!(
            r.record(true, t1),
            RetryLog::Recovered {
                failures: MIN_FAILURES_TO_GIVE_UP + 1
            }
        );
        assert_eq!(r.attempt(t1, budget), RetryAction::Configured);
    }

    #[test]
    fn max_budget_never_gives_up() {
        let t0 = Instant::now();
        let mut r = failed(1, t0);
        let late = t0 + Duration::from_secs(60 * 60 * 24);
        r.record(false, late);
        assert!(!matches!(
            r.attempt(late, NO_LIMIT),
            RetryAction::GiveUp { .. }
        ));
    }
}
