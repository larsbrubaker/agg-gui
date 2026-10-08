//! Condition waits that need no window — the port of
//! `AutomationRunner.StaticDelay` (agg-sharp `GuiAutomation/AutomationRunner.cs`).
//!
//! `static_delay` runs on the wall clock and polls a condition until it holds
//! or the time is up.  The runner's frame-pumping waits (`wait_for`,
//! `assert`, `delay`) build on the drivers in later slices; this one is used
//! where no UI is involved.

use std::thread;
use std::time::{Duration, Instant};

/// Poll `check_condition_satisfied` every `check_interval_ms` milliseconds
/// until it returns true or `max_seconds` of wall-clock time have passed.
///
/// Returns whether the condition was satisfied.  After the deadline it
/// takes one last look: a zero-second wait never enters the loop, and the
/// condition may have come true during the final sleep — either way the
/// answer is the condition, not the clock.
///
/// The deadline is total elapsed time (C# fixed a bug where it compared the
/// 0-59 `Seconds` component, so any wait of a minute or more never expired).
pub fn static_delay(
    mut check_condition_satisfied: impl FnMut() -> bool,
    max_seconds: f64,
    check_interval_ms: u64,
) -> bool {
    let timer = Instant::now();

    while timer.elapsed().as_secs_f64() < max_seconds {
        if check_condition_satisfied() {
            return true;
        }

        thread::sleep(Duration::from_millis(check_interval_ms));
    }

    check_condition_satisfied()
}
