//! `show_window_and_execute_tests`: run one automation test — the port of
//! `AutomationRunner.ShowWindowAndExecuteTests` (agg-sharp
//! `GuiAutomation/AutomationRunner.cs`).
//!
//! C# makes the calling thread the window's message loop and runs the body on
//! a pool thread, racing it against the test's clock with `Task.WhenAny`.
//! agg-gui's widgets and thread-locals cannot cross threads, so here the body
//! runs on the UI thread and the *calling* thread is the watchdog:
//!
//! 1. One fresh thread per run (named [`UI_THREAD_NAME`]): fresh thread-locals
//!    (focus, idle queue, clock), which is what lets runs go in parallel.
//! 2. On it, `build` creates the [`AutomationWindow`] and the body's state;
//!    the window comes up (first paint, then `on_load`) and the thread
//!    reports Loaded. The caller waits for that within the bring-up budget,
//!    `max(secs_to_test_failure, 30 s)`; a load watchdog speaks up two
//!    seconds before it runs out unless the timeout is the expected outcome.
//! 3. The body runs inside `catch_unwind`, on the wall clock from Loaded.
//!    When the budget runs out the caller sets the run's cancel flag and
//!    returns [`AutomationError::Timeout`]; the thread cannot be killed, so it
//!    finishes detached and its next runner call panics
//!    [`TEST_TIMED_OUT`](crate::runner::TEST_TIMED_OUT).
//! 4. The close phase drops the window's tree on its own thread.
//! 5. The outcome is ranked as C# ranks it ([`rank_outcome`]).
//!
//! The window is [`window::AutomationWindow`]; the failures are
//! [`error::AutomationError`]; the body's handle is
//! [`AutomationRunner`](crate::runner::AutomationRunner).

mod error;
#[cfg(test)]
mod tests;
mod window;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agg_gui::unhandled::panic_message;

pub use error::{AutomationError, TEST_NOT_COMPLETED_MESSAGE};
pub use window::{AutomationWindow, AUTOMATION_WINDOW_ROOT_NAME};

use crate::driver::ClockPolicy;
use crate::runner::{AutomationConfig, AutomationRunner};

/// The name of every run's UI thread (C# registers its message-pump thread
/// under the same label for its stack dumps).
pub const UI_THREAD_NAME: &str = "<<< UI THREAD";

/// The least bring-up budget: a machine property, not a test one (C# measured
/// window bring-up at up to 14 s on a loaded machine, nearly all of it GPU
/// device creation).
pub const MIN_BRING_UP: Duration = Duration::from_secs(30);

/// How long before the bring-up budget runs out the load watchdog speaks up,
/// so its report describes a window that is still stuck.
const LOAD_WATCHDOG_LEAD: Duration = Duration::from_secs(2);

/// How a run is set up: C#'s optional parameters, with C#'s defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct RunOptions {
    /// C# `secondsToTestFailure` (30): the body's wall-clock budget, counted
    /// from the window's load, not its creation.
    pub secs_to_test_failure: f64,
    /// C# `timeoutIsTheExpectedOutcome` (false): the test asserts the timeout
    /// itself, so a window that never loads is the result, not a hang to
    /// report, and the load watchdog stays quiet.
    pub timeout_is_the_expected_outcome: bool,
    /// The runner's settings at the start of the body.
    pub config: AutomationConfig,
    /// The clock the run's UI thread uses (virtual unless the test's
    /// background workers need real time).
    pub clock: ClockPolicy,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            secs_to_test_failure: 30.0,
            timeout_is_the_expected_outcome: false,
            config: AutomationConfig::default(),
            clock: ClockPolicy::Virtual,
        }
    }
}

impl RunOptions {
    /// Default options with a test budget of `secs` seconds.
    pub fn with_budget(secs: f64) -> Self {
        Self {
            secs_to_test_failure: secs,
            ..Self::default()
        }
    }
}

/// What the run's UI thread tells the watching thread.
enum Report<R> {
    /// The window painted its first frame and `on_load` returned.
    Loaded,
    /// `build` or bring-up panicked.
    BringUpFailed(String),
    /// The body finished (returned or panicked).
    Finished(BodyOutcome<R>),
    /// The close phase is over.
    Closed,
}

/// How a body ended, as [`rank_outcome`] needs it.
pub(crate) struct BodyOutcome<R> {
    result: Result<R, String>,
    require_test_completion: bool,
    test_was_completed: bool,
}

/// Show the window `build` creates, run `body` against it, and close it; see
/// the module docs. `build` and `body` run on the run's own UI thread; the
/// state `build` returns beside the window (shared logs, handles) is lent to
/// the body and dropped with the window.
pub fn show_window_and_execute_tests<S, R>(
    opts: RunOptions,
    build: impl FnOnce() -> (AutomationWindow, S) + Send + 'static,
    body: impl FnOnce(&mut AutomationRunner, &S) -> R + Send + 'static,
) -> Result<R, AutomationError>
where
    R: Send + 'static,
{
    run_with_bring_up_floor(opts, MIN_BRING_UP, build, body)
}

/// [`show_window_and_execute_tests`] with the bring-up floor as a setting
/// rather than [`MIN_BRING_UP`], so this crate's tests can reach the
/// bring-up timeout without waiting out a real machine's 30 s.
pub(crate) fn run_with_bring_up_floor<S, R>(
    opts: RunOptions,
    min_bring_up: Duration,
    build: impl FnOnce() -> (AutomationWindow, S) + Send + 'static,
    body: impl FnOnce(&mut AutomationRunner, &S) -> R + Send + 'static,
) -> Result<R, AutomationError>
where
    R: Send + 'static,
{
    let budget = secs_to_duration(opts.secs_to_test_failure);
    let bring_up = budget.max(min_bring_up);
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();

    let thread_cancel = Arc::clone(&cancel);
    let config = opts.config.clone();
    let clock = opts.clock;
    thread::Builder::new()
        .name(UI_THREAD_NAME.to_string())
        .spawn(move || run_on_ui_thread(build, body, config, clock, thread_cancel, tx))
        .map_err(|e| {
            AutomationError::BringUpFailed(format!("could not start the UI thread: {e}"))
        })?;

    // Bring-up: its own budget, so a slow window is not charged to the test.
    match wait_for_load(&rx, bring_up, opts.timeout_is_the_expected_outcome) {
        LoadWait::Loaded => {}
        LoadWait::Failed(message) => return Err(AutomationError::BringUpFailed(message)),
        LoadWait::TimedOut => {
            cancel.store(true, Ordering::SeqCst);
            return Err(AutomationError::LoadTimeout);
        }
    }

    // The test: the wall clock starts now, at Loaded.
    let outcome = match rx.recv_timeout(budget) {
        Ok(Report::Finished(outcome)) => outcome,
        Ok(_) | Err(RecvTimeoutError::Disconnected) => {
            return Err(AutomationError::BodyPanicked(
                "the UI thread ended without finishing the test".to_string(),
            ))
        }
        Err(RecvTimeoutError::Timeout) => {
            cancel.store(true, Ordering::SeqCst);
            return Err(AutomationError::Timeout);
        }
    };

    // Close: the tree is dropped on its own thread. A thread that ends
    // without saying so has finished closing too.
    while let Ok(report) = rx.recv() {
        if matches!(report, Report::Closed) {
            break;
        }
    }

    rank_outcome(outcome)
}

/// C#'s order of failures, first match wins: the test timing out (returned
/// before this is reached), a UI-thread failure (slice 27), the body's own
/// failure, a close that timed out (slice 28), and last a body that never
/// called `mark_test_complete` — a real failure is more useful than the
/// generic "did not complete" message.
pub(crate) fn rank_outcome<R>(outcome: BodyOutcome<R>) -> Result<R, AutomationError> {
    let value = outcome.result.map_err(AutomationError::BodyPanicked)?;
    if outcome.require_test_completion && !outcome.test_was_completed {
        return Err(AutomationError::TestNotCompleted);
    }
    Ok(value)
}

/// Everything that happens on the run's UI thread. Sends fail only once the
/// watching thread has given up (a timeout), which is ignored: the thread
/// just finishes.
fn run_on_ui_thread<S, R>(
    build: impl FnOnce() -> (AutomationWindow, S),
    body: impl FnOnce(&mut AutomationRunner, &S) -> R,
    config: AutomationConfig,
    clock: ClockPolicy,
    cancel: Arc<AtomicBool>,
    tx: Sender<Report<R>>,
) {
    let brought_up = catch_unwind(AssertUnwindSafe(|| {
        let (window, state) = build();
        (window.bring_up(clock), state)
    }));
    let (driver, state) = match brought_up {
        Ok(up) => up,
        Err(payload) => {
            let _ = tx.send(Report::BringUpFailed(panic_message(&*payload)));
            return;
        }
    };

    // A window that loaded after the caller gave up never runs its body.
    if tx.send(Report::Loaded).is_err() || cancel.load(Ordering::SeqCst) {
        return;
    }

    let mut runner = AutomationRunner::new(driver, config, cancel);
    let result = catch_unwind(AssertUnwindSafe(|| body(&mut runner, &state)))
        .map_err(|payload| panic_message(&*payload));
    let outcome = BodyOutcome {
        result,
        require_test_completion: runner.config.require_test_completion,
        test_was_completed: runner.test_was_completed(),
    };
    let _ = tx.send(Report::Finished(outcome));

    // The close phase: the window's tree goes on the thread that owns it.
    drop(runner);
    drop(state);
    let _ = tx.send(Report::Closed);
}

enum LoadWait {
    Loaded,
    Failed(String),
    TimedOut,
}

/// Wait up to `bring_up` for the window to load. Unless the timeout is the
/// expected outcome, the load watchdog reports a window still unpainted
/// [`LOAD_WATCHDOG_LEAD`] before the budget runs out (stack dumps join it in
/// slice 30c).
fn wait_for_load<R>(rx: &Receiver<Report<R>>, bring_up: Duration, quiet: bool) -> LoadWait {
    let started = Instant::now();
    let watchdog_at = bring_up
        .saturating_sub(LOAD_WATCHDOG_LEAD)
        .max(Duration::from_millis(10));
    let mut watchdog_fired = quiet;
    loop {
        let limit = if watchdog_fired {
            bring_up
        } else {
            watchdog_at
        };
        let wait = limit.saturating_sub(started.elapsed());
        match rx.recv_timeout(wait) {
            Ok(Report::Loaded) => return LoadWait::Loaded,
            Ok(Report::BringUpFailed(message)) => return LoadWait::Failed(message),
            Ok(Report::Finished(_) | Report::Closed) | Err(RecvTimeoutError::Disconnected) => {
                return LoadWait::Failed("the UI thread ended before its window loaded".to_string())
            }
            Err(RecvTimeoutError::Timeout) if watchdog_fired => return LoadWait::TimedOut,
            Err(RecvTimeoutError::Timeout) => {
                eprintln!(
                    "AutomationRunner: LOAD WATCHDOG - the window has not loaded {} ms after being shown",
                    watchdog_at.as_millis()
                );
                watchdog_fired = true;
            }
        }
    }
}

/// Seconds as a duration: a negative or NaN budget is no time at all, one too
/// large to represent (infinity) never runs out.
fn secs_to_duration(secs: f64) -> Duration {
    if secs.is_nan() || secs <= 0.0 {
        return Duration::ZERO;
    }
    Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX)
}
