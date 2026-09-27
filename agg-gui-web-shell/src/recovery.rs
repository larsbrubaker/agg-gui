//! GPU device-loss rebuild pacing: exponential backoff with a retry cap.
//!
//! Without it a rebuild that fails (the adapter is gone, the browser is
//! refusing new devices) is retried on the very next `requestAnimationFrame`
//! — sixty device requests a second, forever, each logging an error. The
//! frame loop in the crate's `web::frame` module asks [`RebuildBackoff`]
//! before each attempt and shows the fatal panel once it gives up.
//! Platform-neutral so the schedule is unit tested natively.

use std::time::Duration;

use web_time::Instant;

/// Rebuild attempts before the shell gives up and reports a fatal error.
pub(crate) const MAX_REBUILD_ATTEMPTS: u32 = 5;
/// Delay after the first failed rebuild; doubled after each further failure.
pub(crate) const REBUILD_BASE_DELAY: Duration = Duration::from_millis(250);
/// Upper bound on a single backoff delay.
pub(crate) const REBUILD_MAX_DELAY: Duration = Duration::from_secs(8);

/// What to do after a failed rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RebuildVerdict {
    /// Try again once this much time has passed.
    RetryAfter(Duration),
    /// Out of attempts — report the loss as fatal.
    GiveUp,
}

/// Failure count and the earliest instant the next rebuild may start.
#[derive(Debug, Default)]
pub(crate) struct RebuildBackoff {
    failures: u32,
    retry_at: Option<Instant>,
}

impl RebuildBackoff {
    pub(crate) const fn new() -> Self {
        Self {
            failures: 0,
            retry_at: None,
        }
    }

    /// Whether a rebuild may start at `now`.
    pub(crate) fn may_attempt(&self, now: Instant) -> bool {
        self.failures < MAX_REBUILD_ATTEMPTS && self.retry_at.is_none_or(|t| now >= t)
    }

    /// Record a failed rebuild at `now`.
    pub(crate) fn record_failure(&mut self, now: Instant) -> RebuildVerdict {
        self.failures += 1;
        if self.failures >= MAX_REBUILD_ATTEMPTS {
            self.retry_at = None;
            return RebuildVerdict::GiveUp;
        }
        let delay = REBUILD_BASE_DELAY
            .saturating_mul(1 << (self.failures - 1).min(16))
            .min(REBUILD_MAX_DELAY);
        self.retry_at = Some(now + delay);
        RebuildVerdict::RetryAfter(delay)
    }

    /// A rebuild succeeded — the next loss starts a fresh schedule.
    pub(crate) fn record_success(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_attempt_is_immediate() {
        assert!(RebuildBackoff::new().may_attempt(Instant::now()));
    }

    #[test]
    fn failures_back_off_exponentially_then_give_up() {
        let now = Instant::now();
        let mut b = RebuildBackoff::new();
        let mut delays = Vec::new();
        while let RebuildVerdict::RetryAfter(d) = b.record_failure(now) {
            delays.push(d);
        }
        assert_eq!(delays.len() as u32, MAX_REBUILD_ATTEMPTS - 1);
        assert_eq!(delays[0], REBUILD_BASE_DELAY);
        assert!(delays.windows(2).all(|w| w[1] >= w[0]));
        assert!(delays.iter().all(|&d| d <= REBUILD_MAX_DELAY));
        assert!(!b.may_attempt(now + Duration::from_secs(3600)));
    }

    #[test]
    fn a_failed_rebuild_is_not_retried_on_the_next_frame() {
        let now = Instant::now();
        let mut b = RebuildBackoff::new();
        let RebuildVerdict::RetryAfter(d) = b.record_failure(now) else {
            panic!("first failure retries");
        };
        assert!(!b.may_attempt(now + Duration::from_millis(16)));
        assert!(b.may_attempt(now + d));
    }

    #[test]
    fn success_resets_the_schedule() {
        let now = Instant::now();
        let mut b = RebuildBackoff::new();
        b.record_failure(now);
        b.record_success();
        assert!(b.may_attempt(now));
    }
}
