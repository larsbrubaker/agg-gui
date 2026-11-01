//! The runner's waits — the port of `Delay`, `WaitForPendingUiWork`,
//! `WaitFor`, `Assert` and `WaitforDraw` (agg-sharp
//! `GuiAutomation/AutomationRunner.cs`).
//!
//! C# runs the UI on another thread, so its waits sleep or block on a reset
//! event while the UI moves on by itself. Here the test body runs on the UI
//! thread, so every wait pumps the window's frames instead:
//!
//! | C# | here |
//! |---|---|
//! | `WaitForPendingUiWork(max)` | one frame (its idle drain is the sentinel), then more while background work runs, up to `max`; `max <= 0` is false |
//! | `WaitforDraw` | one forced layout + paint frame |
//! | `Delay(s)` | frames until `s` of UI time has passed (`ceil(s / 10 ms)` on the virtual clock) |
//! | `WaitFor` / `Assert` | look; while under `max`, pump frames worth `interval` and look again; the answer is the last look |
//!
//! Time is the thread's UI clock (`agg_gui::clock`): virtual by default, so a
//! 5 s wait that never comes true, with nothing running in the background, is
//! 500 cheap frames and no sleeping. On the
//! real clock ([`ClockPolicy::Real`]) each frame is paced to the frame
//! interval, as a shell's frame loop would be. Every pumped frame first
//! checks the run's timeout, so a long wait stops as soon as its run is
//! cancelled. The name-polling waits are in the sibling `named.rs`.
//!
//! **Real time in condition waits.** On the virtual clock a frame costs well
//! under a millisecond of real time when nothing paints, so a wait counted in
//! frames could give up long before a worker thread delivers the result its
//! condition looks for. So a condition wait — [`AutomationRunner::wait_until`]
//! (and `wait_for` / `assert` on it), the name waits in `named.rs`, the
//! image waits, and [`AutomationRunner::wait_for_idle`], whose condition is
//! idle — paces itself: while its condition is false and the driver's
//! background-busy hook reports work running, each frame it pumps first
//! sleeps one frame interval of real time ([`Pace::RealWhileBusy`]). A wait
//! of N frames then gives the worker at least N frame intervals, and a wait
//! whose condition already holds, or that runs with nothing in the
//! background, sleeps nothing. A wait whose condition comes true between two
//! looks has paced the frames up to that look (a name wait looks every 50 ms
//! of UI time). Pacing the idle wait also keeps a long one (a harness's 30 s)
//! from racing its virtual clock through its whole budget while a worker
//! runs, which would fire every UI-time timer (hover, tooltip, double-click)
//! on the way. Every other frame stays free: `delay`,
//! [`AutomationRunner::wait_for_pending_ui_work`] and the gestures' own
//! pauses measure UI time (a double-click
//! window, a hover or tooltip delay, the pause after a key), not the arrival
//! of a result, and they run on every gesture, so pacing them would make
//! every test slower while background work runs without letting any wait
//! see more. A test that needs a worker's result asks for it with a
//! condition wait, which is where the real time goes.
//!
//! [`AutomationRunner::wait_for_idle`] has no C# counterpart: C# tests wait on
//! a named widget or a condition, but an application's own harness (MatterCAD's
//! `settle`) needs "pump until nothing is left to do". Idle is
//! [`HeadlessDriver::is_idle`](crate::HeadlessDriver::is_idle) after a frame
//! that did not paint, and work reported by the driver's background-busy hook
//! keeps it from being idle.

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

/// How a wait's frames relate to real time; the module docs' "Real time in
/// condition waits".
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pace {
    /// UI time only: on the virtual clock a frame takes no real time.
    UiTime,
    /// A condition wait whose condition is false: on the virtual clock each
    /// frame first sleeps a frame interval while background work runs.
    RealWhileBusy,
}

impl<D: UiDriver> AutomationRunner<D> {
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

    /// [`pump_frame`](Self::pump_frame) at `pace`: with
    /// [`Pace::RealWhileBusy`] on the virtual clock, a frame pumped while the
    /// background-busy hook reports work first sleeps one frame interval, so
    /// results that land during the sleep are drained by this frame.
    fn pump_paced_frame(&mut self, kind: FrameKind, pace: Pace) -> bool {
        let busy = pace == Pace::RealWhileBusy && self.driver.background_busy();
        self.pump_frame_after(kind, busy)
    }

    /// [`pump_frame`](Self::pump_frame), first sleeping one frame interval
    /// on the virtual clock when `paced_busy`: a wait that has just asked
    /// the background-busy hook passes its answer here instead of asking
    /// twice per frame.
    fn pump_frame_after(&mut self, kind: FrameKind, paced_busy: bool) -> bool {
        self.check_not_timed_out();
        if paced_busy && self.driver.clock_policy() == ClockPolicy::Virtual {
            thread::sleep(FRAME_INTERVAL);
        }
        self.pump_frame(kind)
    }

    /// Pump frames until at least `secs` of UI time has passed, always at
    /// least one frame (a zero check interval still lets the UI move).
    fn pump_for(&mut self, secs: f64, pace: Pace) {
        let started = clock::now();
        loop {
            self.pump_paced_frame(FrameKind::Reactive, pace);
            if clock::since(started).as_secs_f64() >= secs {
                break;
            }
        }
    }

    /// C# `Delay`: let `secs_to_wait` seconds of UI time pass, pumping
    /// frames all the while. Zero (or less) is no time at all, as C#'s
    /// `Thread.Sleep(0)` is.
    /// It measures UI time, so it never sleeps on the virtual clock, even
    /// while background work runs (module docs).
    pub fn delay(&mut self, secs_to_wait: f64) -> &mut Self {
        self.delay_paced(secs_to_wait, Pace::UiTime)
    }

    /// [`delay`](Self::delay) at `pace`: the image waits poll with
    /// [`Pace::RealWhileBusy`] between looks.
    pub(crate) fn delay_paced(&mut self, secs_to_wait: f64, pace: Pace) -> &mut Self {
        self.check_not_timed_out();
        let started = clock::now();
        while clock::since(started).as_secs_f64() < secs_to_wait {
            self.pump_paced_frame(FrameKind::Reactive, pace);
        }
        self
    }

    /// C# `WaitForPendingUiWork`: return once the UI thread has run
    /// everything queued before this call. The test body is the UI thread,
    /// so one pumped frame (whose idle drain runs that queue) is the wait,
    /// and the answer is true — unless the driver's background-busy hook
    /// ([`HeadlessDriver::set_background_busy`](crate::HeadlessDriver::set_background_busy))
    /// reports work still running: then frames keep coming until it is done
    /// (true) or `max_milliseconds` of UI time has passed (false).
    /// `max_milliseconds <= 0` gives up at once and reports false, as C# does.
    /// It never sleeps on the virtual clock (module docs): the gestures pause
    /// with it, and its ceiling is gesture-sized.
    pub fn wait_for_pending_ui_work(&mut self, max_milliseconds: i32) -> bool {
        self.wait_for_pending_ui_work_paced(max_milliseconds, Pace::UiTime)
    }

    /// [`wait_for_pending_ui_work`](Self::wait_for_pending_ui_work) at
    /// `pace`: the name waits poll with [`Pace::RealWhileBusy`] between looks.
    pub(crate) fn wait_for_pending_ui_work_paced(
        &mut self,
        max_milliseconds: i32,
        pace: Pace,
    ) -> bool {
        self.check_not_timed_out();
        if max_milliseconds <= 0 {
            return false;
        }
        let started = clock::now();
        let max = f64::from(max_milliseconds) / 1000.0;
        self.pump_paced_frame(FrameKind::Reactive, pace);
        while self.driver.background_busy() {
            if clock::since(started).as_secs_f64() >= max {
                return false;
            }
            self.pump_frame_after(FrameKind::Reactive, pace == Pace::RealWhileBusy);
        }
        true
    }

    /// Pump frames until the UI is idle — a frame that did not paint, after
    /// which [`HeadlessDriver::is_idle`](crate::HeadlessDriver::is_idle)
    /// holds (no background work, no due idle work, no draw requested) — or
    /// until `max_seconds` of UI time has passed. Returns whether it got
    /// idle. Always pumps at least one frame. Idle is its condition, so
    /// while background work keeps it from being idle each frame takes a
    /// frame interval of real time (module docs): the budget lasts as long
    /// as the work in real time, and UI time keeps pace with it.
    pub fn wait_for_idle(&mut self, max_seconds: f64) -> bool {
        self.check_not_timed_out();
        let started = clock::now();
        // The hook is asked once per frame: its answer after a frame both
        // decides idle and paces the next frame.
        let mut busy = false;
        loop {
            let painted = self.pump_frame_after(FrameKind::Reactive, busy);
            busy = self.driver.background_busy();
            if !painted && !busy && self.driver.is_idle() {
                return true;
            }
            if clock::since(started).as_secs_f64() >= max_seconds {
                return false;
            }
        }
    }

    /// C# `StaticDelay` as the runner runs it: look at `check_condition_satisfied`;
    /// while less than `max_seconds` of UI time has passed, pump frames
    /// worth `check_interval_ms` and look again. The answer is the last
    /// look, not the clock: a zero-second wait is a single look. While the
    /// condition is false and background work runs, each frame also takes a
    /// frame interval of real time (module docs), so the wait can't give up
    /// before the work it waits on has had that time.
    ///
    /// The condition is handed the runner, so it can read the tree
    /// ([`app`](Self::app), [`named_widget_exists`](Self::named_widget_exists)).
    pub fn wait_until(
        &mut self,
        mut check_condition_satisfied: impl FnMut(&Self) -> bool,
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
            self.pump_for(interval, Pace::RealWhileBusy);
        }
        check_condition_satisfied(self)
    }

    /// C# `WaitFor`: wait up to `max_seconds` for the condition
    /// ([`wait_until`](Self::wait_until)), carrying on either way.
    pub fn wait_for(
        &mut self,
        check_condition_satisfied: impl FnMut(&Self) -> bool,
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
        check_condition_satisfied: impl FnMut(&Self) -> bool,
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
