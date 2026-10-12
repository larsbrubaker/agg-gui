//! Rust-only tests of the `close_window` callback (C#'s `closeWindow`
//! argument to agg-sharp `AutomationRunner.ShowWindowAndExecuteTests`): it
//! replaces the plain close request, the close budget starts when it
//! returns, and a callback that panics fails the run (ranked after the
//! body's own failure and before a close timeout) while the window still
//! closes. C# has no tests of its own for this; it swallows a throwing
//! callback, which the panic tests pin as fixed.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agg_gui_automation::runner::CLOSE_CHORD;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationWindow, RunOptions,
};

fn opts(close_window_timeout: f64) -> RunOptions {
    RunOptions {
        close_window_timeout,
        ..RunOptions::default()
    }
}

#[test]
fn rust_only_a_close_window_callback_replaces_the_plain_close_request() {
    let asked = Arc::new(AtomicUsize::new(0));
    let ran = Arc::new(AtomicBool::new(false));
    let (asks, runs) = (Arc::clone(&asked), Arc::clone(&ran));

    let result = show_window_and_execute_tests(
        RunOptions::default(),
        move || {
            let window = AutomationWindow::new(300.0, 200.0)
                .on_close_requested(move |_| {
                    asks.fetch_add(1, Ordering::SeqCst);
                    true
                })
                .close_window(move |runner| {
                    runs.store(true, Ordering::SeqCst);
                    runner.type_text(CLOSE_CHORD);
                });
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(result, Ok(()));
    assert!(ran.load(Ordering::SeqCst), "the callback runs");
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the callback's request is the only one"
    );
}

#[test]
fn rust_only_a_close_window_callback_that_never_asks_to_close_is_force_closed() {
    let closed = Arc::new(AtomicBool::new(false));
    let closes = Arc::clone(&closed);

    let result = show_window_and_execute_tests(
        opts(0.2),
        move || {
            let window = AutomationWindow::new(300.0, 200.0)
                .close_window(|_runner| {})
                .on_closed(move |_| closes.store(true, Ordering::SeqCst));
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(
        result,
        Err(AutomationError::CloseTimeout(Duration::from_secs_f64(0.2)))
    );
    assert!(closed.load(Ordering::SeqCst), "the window is force closed");
}

#[test]
fn rust_only_the_close_budget_starts_when_the_close_window_callback_returns() {
    // The shutdown takes longer than the whole close budget, then asks to
    // close: the window agreed in time, so the run passes.
    let result = show_window_and_execute_tests(
        opts(0.3),
        || {
            let window = AutomationWindow::new(300.0, 200.0).close_window(|runner| {
                std::thread::sleep(Duration::from_millis(500));
                runner.type_text(CLOSE_CHORD);
            });
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(result, Ok(()));
}

#[test]
fn rust_only_a_panicking_close_window_callback_fails_the_run_and_the_window_still_closes() {
    let asked = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicBool::new(false));
    let (asks, closes) = (Arc::clone(&asked), Arc::clone(&closed));

    let result = show_window_and_execute_tests(
        RunOptions::default(),
        move || {
            let window = AutomationWindow::new(300.0, 200.0)
                .close_window(|_runner| panic!("shutdown failed"))
                .on_close_requested(move |_| {
                    asks.fetch_add(1, Ordering::SeqCst);
                    true
                })
                .on_closed(move |_| closes.store(true, Ordering::SeqCst));
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "the close_window callback panicked: shutdown failed".to_string()
        ))
    );
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the window is asked to close plainly"
    );
    assert!(closed.load(Ordering::SeqCst));
}

#[test]
fn rust_only_a_panicking_close_window_callback_outranks_the_close_timeout_it_causes() {
    let result = show_window_and_execute_tests(
        opts(0.2),
        || {
            let window = AutomationWindow::new(300.0, 200.0)
                .close_window(|_runner| panic!("shutdown failed"))
                .on_close_requested(|_| false);
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "the close_window callback panicked: shutdown failed".to_string()
        ))
    );
}

#[test]
fn rust_only_a_body_failure_outranks_a_panicking_close_window_callback() {
    let result = show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let window = AutomationWindow::new(300.0, 200.0)
                .close_window(|_runner| panic!("shutdown failed"));
            (window, ())
        },
        |_runner, _| -> () { panic!("the body's own failure") },
    );

    assert_eq!(
        result,
        Err(AutomationError::BodyPanicked(
            "the body's own failure".to_string()
        ))
    );
}
