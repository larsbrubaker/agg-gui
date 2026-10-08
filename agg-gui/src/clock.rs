//! The UI clock: one thread-local source of "now" for every time read in
//! agg-gui that changes behaviour.
//!
//! Animation deadlines ([`crate::animation::request_draw_after`], `Tween`),
//! multi-click and double-click windows, tooltip delays, caret blink, spinner
//! and progress-bar phases, the on-screen keyboard's key repeat and the
//! touch-then-mouse suppression window all read [`now`] instead of
//! `Instant::now()`. Pure profiling timers (paint timing, benchmark tests)
//! stay on real time because they measure the machine, not the UI.
//!
//! The clock is **real** by default, so a running app behaves exactly as if it
//! read `web_time::Instant::now()` directly (which is what it does then; on
//! `wasm32` `web_time` maps to `performance.now()`). A test or headless driver
//! can switch the current thread to a **virtual** clock that stands still until
//! [`advance`]d, making every timing state machine deterministic without
//! sleeping. Virtual time is still a [`web_time::Instant`] — the real time the
//! virtual clock started at plus everything advanced since — so deadlines and
//! timestamps keep one type everywhere.
//!
//! The state is per thread, like the rest of agg-gui's UI state: a test that
//! runs on its own thread gets its own clock. Shells keep reading real time
//! for their OS wake-ups; they only ever run on the real clock.
//!
//! Millisecond timers (`ui_thread::current_timer_ms`) count from one
//! process-wide [`epoch`]. The epoch is fixed no later than the first virtual
//! clock starts: a virtual clock standing before the epoch would read 0 there
//! until frames had advanced it past the epoch, freezing every delay keyed on
//! the timer for as long as the driver spent before its first timer read.

use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Duration;
use web_time::Instant;

thread_local! {
    /// `Some(t)` while the virtual clock is active: the current virtual time.
    /// `None` selects the real clock.
    static VIRTUAL_NOW: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// The process's UI epoch, which millisecond timers count from: the first
/// [`set_virtual`] time or the first call here, whichever came first.
static EPOCH: OnceLock<Instant> = OnceLock::new();

/// The process's UI epoch (see [`EPOCH`]); `ui_thread::current_timer_ms` is
/// the time since it on this thread's clock.
pub fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

/// The current time on this thread's UI clock: the virtual time while the
/// virtual clock is active, otherwise the real `Instant::now()`.
pub fn now() -> Instant {
    VIRTUAL_NOW.with(|c| c.get()).unwrap_or_else(Instant::now)
}

/// Time elapsed on the UI clock since `earlier`; zero when `earlier` is in the
/// clock's future (for example a timestamp taken before the virtual clock was
/// set back).
pub fn since(earlier: Instant) -> Duration {
    now().saturating_duration_since(earlier)
}

/// Whether this thread is on the virtual clock.
pub fn is_virtual() -> bool {
    VIRTUAL_NOW.with(|c| c.get()).is_some()
}

/// Put this thread on the virtual clock, standing still at `at`. Calling it
/// again while virtual moves the virtual time to `at` (backwards too).
pub fn set_virtual(at: Instant) {
    // A virtual clock started before the epoch is fixed fixes it, no later
    // than its own start, so its time since the epoch moves from the start.
    EPOCH.get_or_init(|| {
        let real = Instant::now();
        if at < real {
            at
        } else {
            real
        }
    });
    VIRTUAL_NOW.with(|c| c.set(Some(at)));
}

/// Put this thread on the virtual clock, starting at the current real time,
/// and return that start. Already virtual: keeps the current virtual time.
pub fn start_virtual() -> Instant {
    let at = now();
    set_virtual(at);
    at
}

/// Move the virtual clock forward by `by`. On the real clock this does
/// nothing — real time advances by itself — so a driver can advance every
/// frame whichever clock it runs.
pub fn advance(by: Duration) {
    VIRTUAL_NOW.with(|c| {
        if let Some(t) = c.get() {
            c.set(Some(t + by));
        }
    });
}

/// Put this thread back on the real clock.
pub fn use_real() {
    VIRTUAL_NOW.with(|c| c.set(None));
}

/// Restores the clock mode and time that were current when it was created.
/// Returned by [`scoped_virtual`]; lets a test switch to virtual time without
/// leaking it into the next test on a reused thread, even if it panics.
#[must_use = "dropping the guard immediately restores the previous clock"]
pub struct ClockGuard {
    previous: Option<Instant>,
}

impl Drop for ClockGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        VIRTUAL_NOW.with(|c| c.set(previous));
    }
}

/// Switch this thread to the virtual clock, standing at `at` (or at the
/// current time when `None`), until the returned guard drops.
pub fn scoped_virtual(at: Option<Instant>) -> ClockGuard {
    let previous = VIRTUAL_NOW.with(|c| c.get());
    set_virtual(at.unwrap_or_else(now));
    ClockGuard { previous }
}

#[cfg(test)]
mod tests;
