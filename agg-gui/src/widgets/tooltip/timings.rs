//! Tooltip timing model and the cross-widget "reshow" state that all
//! [`Tooltip`](super::Tooltip) instances share.
//!
//! Mirrors the Windows common-controls tooltip conventions and egui's
//! `Style.interaction` tooltip delay/grace design:
//!
//! * **`initial_delay`** — how long the pointer must rest on a control before
//!   its tip appears the first time (Windows `SPI_GETMOUSEHOVERTIME`, egui's
//!   `tooltip_delay`).
//! * **`reshow_delay`** — the much shorter delay used when the pointer moves
//!   directly from one tipped control to another while the tooltip subsystem is
//!   still "warm" (Windows `TTDT_RESHOW`, egui's `tooltip_grace_time`). This is
//!   the move-along-the-toolbar behaviour.
//! * **`autopop`** — how long a tip stays up before auto-dismissing when the
//!   pointer just sits there (Windows `TTDT_AUTOPOP`).
//!
//! The values live in a thread-local (GUI is single-threaded; mirrors
//! [`crate::device_scale`] / [`crate::platform`]). Native shells override them
//! from the OS at startup; the wasm shell keeps the defaults. A `#[doc(hidden)]`
//! injectable clock lets the timing state machine be unit-tested deterministically
//! (mirrors `touch_state`'s `clear_last_touch_event_for_testing`).

use std::cell::Cell;
use std::time::Duration;
use web_time::Instant;

/// Default initial hover delay. Windows common controls default to roughly
/// 500ms; egui uses 0.3s. We use 500ms and derive the other two from it.
pub(super) const DEFAULT_INITIAL_DELAY: Duration = Duration::from_millis(500);

/// Tunable tooltip delays, shared by every [`Tooltip`](super::Tooltip).
///
/// `reshow_delay` and `autopop` follow the documented Windows relationships to
/// the initial delay (reshow = initial / 5, autopop = initial * 10), so a host
/// only needs the OS hover time to configure all three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooltipTimings {
    /// Delay before a tip first appears on a freshly-hovered control.
    pub initial_delay: Duration,
    /// Shorter delay applied when the pointer moves between tipped controls
    /// while the subsystem is still warm (see [`TooltipTimings::reshow_window`]).
    pub reshow_delay: Duration,
    /// How long a continuously-displayed tip stays up before auto-dismissing.
    pub autopop: Duration,
}

impl Default for TooltipTimings {
    fn default() -> Self {
        Self::from_initial_delay(DEFAULT_INITIAL_DELAY)
    }
}

impl TooltipTimings {
    /// Build a full timing set from just the OS initial hover time, deriving
    /// `reshow_delay` and `autopop` per the Windows tooltip conventions.
    pub fn from_initial_delay(initial: Duration) -> Self {
        Self {
            initial_delay: initial,
            reshow_delay: initial / 5,
            autopop: initial * 10,
        }
    }

    /// Window, measured from when a tip was last visible, during which a newly
    /// hovered control uses [`Self::reshow_delay`] instead of the full initial
    /// delay. One initial-delay's worth of time keeps the subsystem "warm" long
    /// enough for a natural move to an adjacent control, then goes cold again.
    pub(super) fn reshow_window(&self) -> Duration {
        self.initial_delay
    }
}

thread_local! {
    static TIMINGS: Cell<TooltipTimings> = Cell::new(TooltipTimings::default());
    /// Wall-clock time a tooltip was last visible anywhere. Frozen the moment
    /// the pointer leaves, so it measures the gap since the last tip vanished.
    static LAST_VISIBLE_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Injectable test clock. When `Some`, all tooltip timing reads it instead
    /// of the real wall clock so the state machine is deterministic in tests.
    static TEST_CLOCK: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Install the tooltip delays used by every [`Tooltip`](super::Tooltip).
///
/// Native shells call this at startup with values derived from the OS hover
/// time; other hosts keep the defaults.
pub fn set_tooltip_timings(timings: TooltipTimings) {
    TIMINGS.with(|c| c.set(timings));
}

/// Current tooltip delays.
pub fn tooltip_timings() -> TooltipTimings {
    TIMINGS.with(|c| c.get())
}

/// Current time on the tooltip clock — the injected test clock when set,
/// otherwise the real wall clock.
pub(super) fn tooltip_now() -> Instant {
    TEST_CLOCK.with(|c| c.get()).unwrap_or_else(Instant::now)
}

/// Record that a tip is visible right now, warming the reshow window.
pub(super) fn note_tooltip_visible() {
    LAST_VISIBLE_AT.with(|c| c.set(Some(tooltip_now())));
}

/// When a tip was last visible anywhere, if recently enough to matter.
pub(super) fn last_tooltip_visible_at() -> Option<Instant> {
    LAST_VISIBLE_AT.with(|c| c.get())
}

// --- Test hooks ---------------------------------------------------------------

/// Pin the tooltip clock to `now` (or release it back to the wall clock with
/// `None`). Test-only: lets a unit test drive the delay/reshow/autopop state
/// machine without sleeping.
#[doc(hidden)]
pub fn set_tooltip_test_clock(now: Option<Instant>) {
    TEST_CLOCK.with(|c| c.set(now));
}

/// Advance the pinned test clock by `by`. Panics if no test clock is set.
#[doc(hidden)]
pub fn advance_tooltip_test_clock(by: Duration) {
    TEST_CLOCK.with(|c| {
        let base = c.get().expect("advance_tooltip_test_clock without a test clock");
        c.set(Some(base + by));
    });
}

/// Reset all tooltip timing globals (clock, reshow window, delays) so tests do
/// not leak state into one another on a reused harness thread.
#[doc(hidden)]
pub fn reset_tooltip_test_state() {
    TEST_CLOCK.with(|c| c.set(None));
    LAST_VISIBLE_AT.with(|c| c.set(None));
    TIMINGS.with(|c| c.set(TooltipTimings::default()));
}
