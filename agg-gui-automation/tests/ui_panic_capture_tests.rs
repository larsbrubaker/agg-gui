//! Rust-only tests of the run's UI-panic capture (C#'s
//! `CaptureUiThreadException` on `UiThread.UnhandledException` in agg-sharp
//! `AutomationRunner.ShowWindowAndExecuteTests`): a panicking idle action
//! while the body runs fails the run, stops the body at its next runner
//! call and outranks the body's own failure; one while the window closes,
//! which C# loses, fails the run as a panic while closing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agg_gui::ui_thread;
use agg_gui_automation::runner::CLOSE_CHORD;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationWindow, RunOptions,
};

#[test]
fn rust_only_an_idle_panic_while_the_body_runs_fails_the_run_and_stops_the_body() {
    let reached_the_end = Arc::new(AtomicBool::new(false));
    let reached = Arc::clone(&reached_the_end);

    let result = show_window_and_execute_tests(
        RunOptions::default(),
        || (AutomationWindow::new(200.0, 100.0), ()),
        move |runner, _| {
            ui_thread::run_on_idle(|| panic!("idle action failed"));
            runner.delay(0.1);
            runner.delay(0.1);
            reached.store(true, Ordering::SeqCst);
            runner.mark_test_complete();
        },
    );

    assert_eq!(
        result,
        Err(AutomationError::UiThreadPanicked(
            "idle action failed".to_string()
        ))
    );
    assert!(
        !reached_the_end.load(Ordering::SeqCst),
        "the body stops at its next runner call"
    );
}

#[test]
fn rust_only_a_ui_panic_outranks_the_bodys_own_failure() {
    let result = show_window_and_execute_tests(
        RunOptions::default(),
        || (AutomationWindow::new(200.0, 100.0), ()),
        |runner, _| -> () {
            ui_thread::run_on_idle(|| panic!("idle action failed"));
            runner.delay(0.1);
            panic!("the body's own failure");
        },
    );

    assert_eq!(
        result,
        Err(AutomationError::UiThreadPanicked(
            "idle action failed".to_string()
        ))
    );
}

#[test]
fn rust_only_an_idle_panic_while_the_window_closes_fails_the_run() {
    let result = show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let window = AutomationWindow::new(200.0, 100.0).close_window(|runner| {
                ui_thread::run_on_idle(|| panic!("shutdown work failed"));
                // Asks for the close without pumping, so the panic happens in
                // the close phase's own frames.
                agg_gui_automation::UiDriver::request_close(runner.driver_mut());
            });
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "the window panicked while closing: shutdown work failed".to_string()
        ))
    );
}

#[test]
fn rust_only_a_close_window_callback_stopped_by_a_ui_panic_reports_that_panic() {
    let result = show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let window = AutomationWindow::new(200.0, 100.0).close_window(|runner| {
                ui_thread::run_on_idle(|| panic!("shutdown work failed"));
                runner.delay(0.1);
                runner.type_text(CLOSE_CHORD);
            });
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "the close_window callback panicked: shutdown work failed".to_string()
        ))
    );
}
