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
//!    A handler on the thread keeps the first UI-thread panic agg-gui
//!    contains (an idle action's, C#'s `UiThread.UnhandledException`); the
//!    body's next runner call then unwinds, and that panic is the run's
//!    failure.
//!    When the budget runs out the caller sets the run's cancel flag and
//!    returns [`AutomationError::Timeout`]; the thread cannot be killed, so it
//!    finishes detached and its next runner call panics
//!    [`TEST_TIMED_OUT`](crate::runner::TEST_TIMED_OUT).
//! 4. The close phase hands the runner to the window's `close_window`
//!    callback (C#'s `closeWindow`, the application's shutdown) when it has
//!    one, inside `catch_unwind`, and the close budget starts when that
//!    returns. Then `close.rs` asks the window to close (unless the callback
//!    did the asking), honours its
//!    `on_close_requested` veto while the close budget lasts, and then force
//!    closes it; the tree is dropped on its own thread either way. The
//!    caller waits for that, and gives up on a UI thread stuck in a frame
//!    after a second budget, reporting [`AutomationError::CloseTimeout`].
//! 5. The outcome is ranked as C# ranks it ([`rank_outcome`]).
//!
//! The window is [`window::AutomationWindow`]; the failures are
//! [`error::AutomationError`]; the body's handle is
//! [`AutomationRunner`](crate::runner::AutomationRunner).

mod close;
mod error;
#[cfg(test)]
mod tests;
mod window;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agg_gui::unhandled::{panic_message, set_unhandled_handler};

pub use error::{AutomationError, TEST_NOT_COMPLETED_MESSAGE};
pub use window::{AutomationWindow, AUTOMATION_WINDOW_ROOT_NAME};

use crate::driver::ClockPolicy;
use crate::image_source::DirectoryImageSource;
use crate::runner::{AutomationConfig, AutomationRunner, UiFault, UI_THREAD_FAULTED};

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
    /// C# `CloseWindowTimeoutSeconds` (15): how long, in wall-clock seconds,
    /// the window may take to close after the body finished before it is
    /// force closed and the run fails with [`AutomationError::CloseTimeout`].
    /// A per-run setting rather than C#'s static, so parallel runs cannot
    /// change each other's.
    pub close_window_timeout: f64,
    /// C# `imagesDirectory` (""): the folder image gestures read needles
    /// given by name from; empty is the current directory, as in C#. A body
    /// can install another loader with `AutomationRunner::set_image_source`.
    pub images_directory: PathBuf,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            secs_to_test_failure: 30.0,
            timeout_is_the_expected_outcome: false,
            config: AutomationConfig::default(),
            clock: ClockPolicy::Virtual,
            close_window_timeout: 15.0,
            images_directory: PathBuf::new(),
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
    /// The window's `close_window` callback returned (at once when it has
    /// none), with its panic message if it panicked; the close budget
    /// starts now.
    CloseCallbackReturned(Option<String>),
    /// The close phase is over.
    Closed(CloseReport),
}

/// How the close phase ended, as [`rank_outcome`] needs it.
pub(crate) enum CloseReport {
    /// The window closed when asked.
    Closed,
    /// The window was still open when the close budget ran out (or its UI
    /// thread never came back to report); it was force closed.
    TimedOut(Duration),
    /// Closing the window panicked (a close hook, a frame, or the tree's
    /// drop), with the panic's message.
    Panicked(String),
}

/// How a body ended, as [`rank_outcome`] needs it.
pub(crate) struct BodyOutcome<R> {
    result: Result<R, String>,
    /// The first UI-thread panic (an idle action's) while the body ran.
    ui_fault: Option<String>,
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
    let images = DirectoryImageSource::new(opts.images_directory.clone());
    let close_timeout = secs_to_duration(opts.close_window_timeout);
    thread::Builder::new()
        .name(UI_THREAD_NAME.to_string())
        .spawn(move || {
            let setup = UiThreadSetup {
                config,
                clock,
                images,
                close_timeout,
            };
            run_on_ui_thread(build, body, setup, thread_cancel, tx)
        })
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

    // Close: the application's shutdown (the `close_window` callback) gets
    // three close budgets to return, C#'s hard cap for a shutdown that hangs.
    // Once it has, the UI thread closes (or force closes) the window within
    // the close budget; should it never come back from a frame, give up after
    // a second budget, as C# does after its posted force close. A thread
    // that ends without saying so has finished closing too.
    let mut close_deadline = Instant::now() + close_timeout.saturating_mul(3);
    let mut callback_panic = None;
    let close = loop {
        let wait = close_deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(Report::Closed(report)) => break report,
            Ok(Report::CloseCallbackReturned(panic)) => {
                callback_panic = panic;
                close_deadline = Instant::now() + close_timeout.saturating_mul(2);
            }
            Ok(_) => continue,
            Err(RecvTimeoutError::Disconnected) => break CloseReport::Closed,
            Err(RecvTimeoutError::Timeout) => {
                eprintln!(
                    "AutomationRunner: FORCED CLOSE DID NOT RELEASE THE UI THREAD after {} seconds - it is stuck in a frame.",
                    close_timeout.as_secs_f64()
                );
                cancel.store(true, Ordering::SeqCst);
                break CloseReport::TimedOut(close_timeout);
            }
        }
    };

    rank_outcome(outcome, callback_panic, close)
}

/// The message of a run whose `close_window` callback panicked.
pub(crate) fn close_callback_panic_message(panic: &str) -> String {
    format!("the close_window callback panicked: {panic}")
}

/// C#'s order of failures, first match wins: the test timing out (returned
/// before this is reached), a UI-thread panic while the body ran, the body's own
/// failure, a `close_window` callback that panicked, a close that panicked
/// or timed out, and last a body that never called `mark_test_complete` — a
/// real failure is more useful than the generic "did not complete" message,
/// and a blocked shutdown is the more useful diagnosis than a missing
/// completion.
///
/// The callback's panic: C# captures a throwing `closeWindow` as a UI-thread
/// exception, but by then the body has already won its `Task.WhenAny`, so
/// the capture is never thrown and the run passes with its shutdown broken.
/// Here it fails the run, after the body's own failure (C#'s rule that the
/// real test failure wins) and before the close timeout it usually causes
/// (C#'s stated intent: the callback's fault, not a close hang, is "the real
/// failure").
///
/// A UI-thread panic during the close phase is lost in C# for the same
/// reason; here it is a panic while closing (`CloseReport::Panicked`).
pub(crate) fn rank_outcome<R>(
    outcome: BodyOutcome<R>,
    callback_panic: Option<String>,
    close: CloseReport,
) -> Result<R, AutomationError> {
    if let Some(fault) = outcome.ui_fault {
        return Err(AutomationError::UiThreadPanicked(fault));
    }
    let value = outcome.result.map_err(AutomationError::BodyPanicked)?;
    if let Some(panic) = callback_panic {
        return Err(AutomationError::BodyPanicked(close_callback_panic_message(
            &panic,
        )));
    }
    match close {
        CloseReport::Closed => {}
        CloseReport::TimedOut(timeout) => return Err(AutomationError::CloseTimeout(timeout)),
        CloseReport::Panicked(message) => {
            return Err(AutomationError::BodyPanicked(format!(
                "the window panicked while closing: {message}"
            )))
        }
    }
    if outcome.require_test_completion && !outcome.test_was_completed {
        return Err(AutomationError::TestNotCompleted);
    }
    Ok(value)
}

/// The run's settings the UI thread takes from [`RunOptions`].
struct UiThreadSetup {
    config: AutomationConfig,
    clock: ClockPolicy,
    images: DirectoryImageSource,
    close_timeout: Duration,
}

/// Everything that happens on the run's UI thread. Sends fail only once the
/// watching thread has given up (a timeout), which is ignored: the thread
/// just finishes.
fn run_on_ui_thread<S, R>(
    build: impl FnOnce() -> (AutomationWindow, S),
    body: impl FnOnce(&mut AutomationRunner, &S) -> R,
    setup: UiThreadSetup,
    cancel: Arc<AtomicBool>,
    tx: Sender<Report<R>>,
) {
    let UiThreadSetup {
        config,
        clock,
        images,
        close_timeout,
    } = setup;
    // C# `UiThread.UnhandledException += CaptureUiThreadException`: the
    // first contained UI-thread panic is kept, and the rest are dropped.
    let ui_fault = UiFault::default();
    let handler_fault = Rc::clone(&ui_fault);
    let _handler = set_unhandled_handler(move |report| {
        let mut fault = handler_fault.borrow_mut();
        if fault.is_none() {
            *fault = Some(report.message.clone());
        }
    });
    let brought_up = catch_unwind(AssertUnwindSafe(|| {
        let (window, state) = build();
        (window.bring_up(clock), state)
    }));
    let ((driver, close_hooks), state) = match brought_up {
        Ok(up) => up,
        Err(payload) => {
            let _ = tx.send(Report::BringUpFailed(panic_message(&*payload)));
            return;
        }
    };

    if let Some(fault) = ui_fault.borrow_mut().take() {
        let _ = tx.send(Report::BringUpFailed(fault));
        return;
    }

    // A window that loaded after the caller gave up never runs its body.
    if tx.send(Report::Loaded).is_err() || cancel.load(Ordering::SeqCst) {
        return;
    }

    let mut runner = AutomationRunner::new(driver, config, cancel);
    runner.set_image_source(images);
    runner.set_ui_fault(Rc::clone(&ui_fault));
    let result = catch_unwind(AssertUnwindSafe(|| body(&mut runner, &state)))
        .map_err(|payload| panic_message(&*payload));
    // The fault ranks with the body; one during the close is the close's.
    let body_fault = ui_fault.borrow_mut().take();
    let outcome = BodyOutcome {
        result,
        ui_fault: body_fault,
        require_test_completion: runner.config.require_test_completion,
        test_was_completed: runner.test_was_completed(),
    };
    let _ = tx.send(Report::Finished(outcome));

    // The application's shutdown, when the window has one. A panic in it
    // still has to reach a close request, or the run would be misreported
    // as a close hang (C# then calls `CloseOnIdle` itself).
    let mut close_hooks = close_hooks;
    let mut request = true;
    let mut callback_panic = None;
    if let Some(close_window) = close_hooks.close_window.take() {
        match catch_unwind(AssertUnwindSafe(|| close_window(&mut runner))) {
            Ok(()) => request = false,
            Err(payload) => {
                // A callback stopped by a UI-thread panic reports that panic.
                let message = panic_message(&*payload);
                let fault = ui_fault.borrow_mut().take();
                callback_panic = Some(match fault {
                    Some(fault) if message == UI_THREAD_FAULTED => fault,
                    _ => message,
                });
            }
        }
    }
    let _ = tx.send(Report::CloseCallbackReturned(callback_panic));

    // The close phase: the window's tree goes on the thread that owns it.
    let driver = runner.into_driver();
    let closed = catch_unwind(AssertUnwindSafe(|| {
        close::close_window(driver, close_hooks, close_timeout, request)
    }));
    let report = match closed {
        Ok(close::CloseResult::Closed) => CloseReport::Closed,
        Ok(close::CloseResult::Forced) => CloseReport::TimedOut(close_timeout),
        Err(payload) => CloseReport::Panicked(panic_message(&*payload)),
    };
    let report = match ui_fault.borrow_mut().take() {
        Some(fault) if !matches!(report, CloseReport::Panicked(_)) => CloseReport::Panicked(fault),
        _ => report,
    };
    drop(state);
    let _ = tx.send(Report::Closed(report));
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
            Ok(Report::Finished(_) | Report::CloseCallbackReturned(_) | Report::Closed(_))
            | Err(RecvTimeoutError::Disconnected) => {
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
