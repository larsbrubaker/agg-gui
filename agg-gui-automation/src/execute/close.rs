//! The close phase of an automation run — the port of `RequestWindowClose`
//! and its close watchdog in agg-sharp `AutomationRunner.ShowWindowAndExecuteTests`
//! (`GuiAutomation/AutomationRunner.cs`).
//!
//! Once the body has finished, the run hands the window to its
//! `close_window` callback (C#'s `closeWindow`, the application's own
//! shutdown) when it has one, and otherwise asks the window to close (C#
//! `CloseOnIdle`) unless the body already asked (typing `%{F4}`). It keeps
//! pumping frames so the application's own shutdown work can run. Each close
//! request the window takes is put to its `on_close_requested` hook, which
//! may veto it, the shape of a "save changes?" prompt. A window still open
//! when the close budget (`RunOptions::close_window_timeout`) runs out is
//! force closed — `on_closed` fires and the tree is dropped — and the run
//! fails with [`AutomationError::CloseTimeout`](super::AutomationError::CloseTimeout)
//! rather than hanging or passing silently.
//!
//! This runs on the run's UI thread (`mod.rs`), which owns the tree. The
//! watching thread backs it up: should the UI thread never come back from a
//! frame, the watcher gives up after a second budget, as C#'s grace period
//! after its posted force close does.

use std::thread;
use std::time::{Duration, Instant};

use super::window::CloseHooks;
use crate::driver::{FrameKind, HeadlessDriver, UiDriver, FRAME_INTERVAL};

/// How the window's close went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseResult {
    /// The window agreed to close within the budget.
    Closed,
    /// The budget ran out with the window still open; it was force closed.
    Forced,
}

/// Close the window: request (unless `request` is false: a `close_window`
/// callback has already done the application's shutdown, which asks for the
/// close itself), honour vetoes, and force close after `timeout`, counted
/// from here; see the module docs. The tree is dropped before this returns.
pub(crate) fn close_window(
    mut driver: HeadlessDriver,
    mut hooks: CloseHooks,
    timeout: Duration,
    request: bool,
) -> CloseResult {
    let started = Instant::now();
    // C# `CloseOnIdle`. A request the body already made is the same request.
    if request {
        driver.request_close();
    }
    let result = loop {
        // C#'s close request is an idle action, so the UI work queued before
        // it (the body's or the shutdown callback's) runs first; one frame
        // drains it before the request is answered.
        driver.pump(FrameKind::Reactive);
        if driver.take_close_request() && may_close(&mut driver, &mut hooks) {
            break CloseResult::Closed;
        }
        if started.elapsed() >= timeout {
            eprintln!(
                "AutomationRunner: WINDOW FAILED TO CLOSE - forcing close {:.1} s after close was requested. The window vetoed its close or never answered it.",
                started.elapsed().as_secs_f64()
            );
            break CloseResult::Forced;
        }
        // Let the application's shutdown work run (the next pass's frame),
        // paced as a shell's frame loop is: the budget is wall time, and a
        // virtual-clock frame costs nothing, so an unpaced loop would spin a
        // core for the whole budget.
        thread::sleep(FRAME_INTERVAL);
    };

    // C# `Closed` (and `ForceClose`'s close cascade): the hook sees the tree
    // still alive, then the tree goes.
    if let Some(on_closed) = hooks.on_closed.take() {
        on_closed(driver.app_mut());
    }
    drop(driver);
    result
}

/// C# `ShouldClose`: whether the window lets this close request through. No
/// hook means it does.
fn may_close(driver: &mut HeadlessDriver, hooks: &mut CloseHooks) -> bool {
    match hooks.on_close_requested.as_mut() {
        Some(should_close) => should_close(driver.app_mut()),
        None => true,
    }
}
