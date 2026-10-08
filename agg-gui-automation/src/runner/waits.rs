//! The runner's waits — the port of `Delay`, `WaitForPendingUiWork`,
//! `WaitFor`, `Assert` and `WaitforDraw` (agg-sharp
//! `GuiAutomation/AutomationRunner.cs`).
//!
//! C# runs the UI on another thread, so its waits sleep or block on a reset
//! event while the UI moves on by itself. Here the test body runs on the UI
//! thread, so every wait pumps the window's frames instead (design:
//! `docs/design/gui-automation.md`, section 5):
//!
//! | C# | here |
//! |---|---|
//! | `WaitForPendingUiWork(max)` | one frame (its idle drain is the sentinel); `max <= 0` is false |
//! | `WaitforDraw` | one forced layout + paint frame |
//! | `Delay(s)` | frames until `s` of UI time has passed (`ceil(s / 10 ms)` on the virtual clock) |
//! | `WaitFor` / `Assert` | look; while under `max`, pump frames worth `interval` and look again; the answer is the last look |
//!
//! Time is the thread's UI clock (`agg_gui::clock`): virtual by default, so a
//! 5 s wait that never comes true is 500 cheap frames and no sleeping. On the
//! real clock ([`ClockPolicy::Real`]) each frame is paced to the frame
//! interval, as a shell's frame loop would be. Every pumped frame first
//! checks the run's timeout, so a long wait stops as soon as its run is
//! cancelled. The name-polling waits are in the sibling `named.rs`.

use std::thread;

use agg_gui::clock;

use super::AutomationRunner;
use crate::driver::{ClockPolicy, FrameKind, UiDriver, FRAME_INTERVAL};

/// C# `Delay`'s default, in seconds.
pub const DEFAULT_DELAY_SECONDS: f64 = 0.2;

/// C# `DefaultUiWorkWaitMilliseconds`: the default ceiling of one
/// [`AutomationRunner::wait_for_pending_ui_work`] (25 pump intervals).
pub const DEFAULT_UI_WORK_WAIT_MILLISECONDS: i32 = 250;

/// C# `WaitFor` / `Assert`'s default `maxSeconds`.
pub const DEFAULT_CONDITION_WAIT_SECONDS: f64 = 5.0;

/// C# `WaitFor` / `Assert` / `StaticDelay`'s default `checkInterval`, in
/// milliseconds.
pub const DEFAULT_CHECK_INTERVAL_MILLISECONDS: u64 = 10;

impl AutomationRunner {
    /// Pump one frame, paced to the frame interval on the real clock.
    /// Panics with [`super::TEST_TIMED_OUT`] once the run has been
    /// cancelled.
    pub(crate) fn pump_frame(&mut self, kind: FrameKind) -> bool {
        self.check_not_timed_out();
        let painted = self.driver.pump(kind);
        if self.driver.clock_policy() == ClockPolicy::Real {
            thread::sleep(FRAME_INTERVAL);
        }
        painted
    }

    /// Pump frames until at least `secs` of UI time has passed, always at
    /// least one frame (a zero check interval still lets the UI move).
    fn pump_for(&mut self, secs: f64) {
        let started = clock::now();
        loop {
            self.pump_frame(FrameKind::Reactive);
            if clock::since(started).as_secs_f64() >= secs {
                break;
            }
        }
    }

    /// C# `Delay`: let `secs_to_wait` seconds of UI time pass, pumping
    /// frames all the while. Zero (or less) is no time at all, as C#'s
    /// `Thread.Sleep(0)` is.
    pub fn delay(&mut self, secs_to_wait: f64) -> &mut Self {
        self.check_not_timed_out();
        let started = clock::now();
        while clock::since(started).as_secs_f64() < secs_to_wait {
            self.pump_frame(FrameKind::Reactive);
        }
        self
    }

    /// C# `WaitForPendingUiWork`: return once the UI thread has run
    /// everything queued before this call. The test body is the UI thread,
    /// so one pumped frame (whose idle drain runs that queue) is the whole
    /// wait, and the answer is true. `max_milliseconds <= 0` gives up at
    /// once and reports false, as C# does.
    pub fn wait_for_pending_ui_work(&mut self, max_milliseconds: i32) -> bool {
        self.check_not_timed_out();
        if max_milliseconds <= 0 {
            return false;
        }
        self.pump_frame(FrameKind::Reactive);
        true
    }

    /// C# `StaticDelay` as the runner runs it: look at `check_condition_satisfied`;
    /// while less than `max_seconds` of UI time has passed, pump frames
    /// worth `check_interval_ms` and look again. The answer is the last
    /// look, not the clock: a zero-second wait is a single look.
    ///
    /// The condition is handed the runner, so it can read the tree
    /// ([`app`](Self::app), [`named_widget_exists`](Self::named_widget_exists)).
    pub fn wait_until(
        &mut self,
        mut check_condition_satisfied: impl FnMut(&AutomationRunner) -> bool,
        max_seconds: f64,
        check_interval_ms: u64,
    ) -> bool {
        self.check_not_timed_out();
        let started = clock::now();
        let interval = check_interval_ms as f64 / 1000.0;
        while clock::since(started).as_secs_f64() < max_seconds {
            if check_condition_satisfied(self) {
                return true;
            }
            self.pump_for(interval);
        }
        check_condition_satisfied(self)
    }

    /// C# `WaitFor`: wait up to `max_seconds` for the condition
    /// ([`wait_until`](Self::wait_until)), carrying on either way.
    pub fn wait_for(
        &mut self,
        check_condition_satisfied: impl FnMut(&AutomationRunner) -> bool,
        max_seconds: f64,
        check_interval_ms: u64,
    ) -> &mut Self {
        self.wait_until(check_condition_satisfied, max_seconds, check_interval_ms);
        self
    }

    /// C# `Assert`: wait up to `max_seconds` for the condition, and panic
    /// with `Require Failed: {error_response}` if it never holds.
    pub fn assert(
        &mut self,
        check_condition_satisfied: impl FnMut(&AutomationRunner) -> bool,
        error_response: &str,
        max_seconds: f64,
        check_interval_ms: u64,
    ) -> &mut Self {
        let satisfied = self.wait_until(check_condition_satisfied, max_seconds, check_interval_ms);
        if !satisfied {
            panic!("Require Failed: {error_response}");
        }
        self
    }

    /// C# `WaitforDraw`: return once the window has drawn again. Headless
    /// that is one forced frame — laid out if due, then painted — so what a
    /// gesture changed is on screen when this returns.
    pub fn wait_for_draw(&mut self) -> &mut Self {
        self.check_not_timed_out();
        self.pump_frame(FrameKind::Forced);
        self
    }
}
