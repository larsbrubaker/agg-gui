//! Rust-only tests of `show_window_and_execute_tests` (agg-gui-automation
//! `src/execute/`): the parts of C#'s `ShowWindowAndExecuteTests` that its
//! own tests reach only indirectly — the per-run UI thread, `on_load`
//! ordering, the cancel flag a timed-out body meets at its next runner call,
//! `MarkTestComplete` enforcement and the order failures are reported in.
//! The ported C# tests of the same code are in `automation_runner_tests.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agg_gui_automation::execute::{TEST_NOT_COMPLETED_MESSAGE, UI_THREAD_NAME};
use agg_gui_automation::runner::TEST_TIMED_OUT;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationWindow, RunOptions,
};

fn window() -> (AutomationWindow, ()) {
    (AutomationWindow::new(300.0, 200.0), ())
}

#[test]
fn rust_only_a_completed_body_returns_its_value() {
    let result = show_window_and_execute_tests(RunOptions::default(), window, |runner, _| {
        runner.mark_test_complete();
        42
    });

    assert_eq!(result, Ok(42));
}

#[test]
fn rust_only_each_run_gets_its_own_named_ui_thread() {
    let caller = thread::current().id();

    let ui_thread = show_window_and_execute_tests(RunOptions::default(), window, |runner, _| {
        runner.mark_test_complete();
        let current = thread::current();
        (current.id(), current.name().map(str::to_string))
    })
    .expect("run");

    assert_ne!(ui_thread.0, caller);
    assert_eq!(ui_thread.1.as_deref(), Some(UI_THREAD_NAME));
}

#[test]
fn rust_only_on_load_runs_after_the_first_paint_and_before_the_body() {
    let (painted_at_load, loaded_before_body) = show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let loaded = Arc::new(AtomicBool::new(false));
            let loaded_in_hook = Arc::clone(&loaded);
            let painted = Arc::new(AtomicBool::new(false));
            let painted_in_hook = Arc::clone(&painted);
            let window = AutomationWindow::new(300.0, 200.0).on_load(move |app| {
                // The first frame laid the root out at the window's size.
                painted_in_hook.store(app.root().bounds().width > 0.0, Ordering::SeqCst);
                loaded_in_hook.store(true, Ordering::SeqCst);
            });
            (window, (painted, loaded))
        },
        |runner, (painted, loaded)| {
            runner.mark_test_complete();
            (
                painted.load(Ordering::SeqCst),
                loaded.load(Ordering::SeqCst),
            )
        },
    )
    .expect("run");

    assert!(painted_at_load);
    assert!(loaded_before_body);
}

#[test]
fn rust_only_a_timed_out_body_panics_at_its_next_runner_call() {
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();

    let result = show_window_and_execute_tests(
        RunOptions {
            secs_to_test_failure: 0.2,
            timeout_is_the_expected_outcome: true,
            ..RunOptions::default()
        },
        window,
        move |runner, _| {
            thread::sleep(Duration::from_millis(600));
            let call = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runner.mark_test_complete();
            }));
            let message = call
                .err()
                .map(|payload| agg_gui::unhandled::panic_message(&*payload));
            let _ = tx.send((message, runner.test_was_completed()));
        },
    );

    assert_eq!(result, Err(AutomationError::Timeout));
    assert!(
        started.elapsed() < Duration::from_millis(600),
        "the caller returns at the budget, not when the body wakes"
    );
    let (message, completed) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the detached body reports back");
    assert_eq!(message.as_deref(), Some(TEST_TIMED_OUT));
    assert!(!completed);
}

#[test]
fn rust_only_a_body_that_does_not_mark_completion_fails_with_the_csharp_message() {
    let result = show_window_and_execute_tests(RunOptions::default(), window, |_, _| ());

    assert_eq!(result, Err(AutomationError::TestNotCompleted));
    assert_eq!(
        AutomationError::TestNotCompleted.to_string(),
        TEST_NOT_COMPLETED_MESSAGE
    );
    assert_eq!(
        TEST_NOT_COMPLETED_MESSAGE,
        "Test did not call MarkTestComplete(). The test may have exited before reaching its last statement."
    );
}

#[test]
fn rust_only_completion_is_not_required_when_the_body_turns_it_off() {
    let result = show_window_and_execute_tests(RunOptions::default(), window, |runner, _| {
        runner.config.require_test_completion = false;
    });

    assert_eq!(result, Ok(()));
}

#[test]
fn rust_only_a_body_panic_outranks_a_missing_completion() {
    let result: Result<(), _> =
        show_window_and_execute_tests(RunOptions::default(), window, |_, _| {
            panic!("the body failed here");
        });

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "the body failed here".to_string()
        ))
    );
}

#[test]
fn rust_only_a_window_that_fails_to_build_is_a_bring_up_failure() {
    let result = show_window_and_execute_tests(
        RunOptions::default(),
        || -> (AutomationWindow, ()) { panic!("no window today") },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(
        result,
        Err(AutomationError::BringUpFailed(
            "no window today".to_string()
        ))
    );
}
