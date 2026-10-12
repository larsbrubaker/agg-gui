//! Rust-only tests of the automation run's close phase (the port of
//! `RequestWindowClose` and its watchdog in agg-sharp
//! `AutomationRunner.ShowWindowAndExecuteTests`): a window that agrees to
//! close closes at once, a body failure outranks a close timeout as in C#,
//! and a close the body requested is the request the close phase answers.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui_automation::runner::CLOSE_CHORD;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationError, AutomationWindow, RunOptions,
};

#[test]
fn rust_only_a_window_that_agrees_to_close_closes_without_waiting_out_the_budget() {
    let asked = Arc::new(AtomicUsize::new(0));
    let closed = Arc::new(AtomicBool::new(false));
    let (asks, closes) = (Arc::clone(&asked), Arc::clone(&closed));
    let started = Instant::now();

    let result = show_window_and_execute_tests(
        RunOptions::default(),
        move || {
            let window = AutomationWindow::new(300.0, 200.0)
                .on_close_requested(move |_| {
                    asks.fetch_add(1, Ordering::SeqCst);
                    true
                })
                .on_closed(move |_| closes.store(true, Ordering::SeqCst));
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(result, Ok(()));
    assert_eq!(asked.load(Ordering::SeqCst), 1, "the window is asked once");
    assert!(closed.load(Ordering::SeqCst));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a close the window agrees to must not wait out the 15 s close budget"
    );
}

#[test]
fn rust_only_a_body_failure_outranks_a_close_timeout() {
    let result = show_window_and_execute_tests(
        RunOptions {
            close_window_timeout: 0.2,
            ..RunOptions::default()
        },
        || {
            let window = AutomationWindow::new(300.0, 200.0).on_close_requested(|_| false);
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

#[test]
fn rust_only_a_close_the_body_requested_is_the_one_the_close_phase_answers() {
    let asked = Arc::new(AtomicUsize::new(0));
    let asks = Arc::clone(&asked);

    let result = show_window_and_execute_tests(
        RunOptions::default(),
        move || {
            let window = AutomationWindow::new(300.0, 200.0).on_close_requested(move |_| {
                asks.fetch_add(1, Ordering::SeqCst);
                true
            });
            (window, ())
        },
        |runner, _| {
            runner.type_text(CLOSE_CHORD);
            runner.mark_test_complete();
        },
    );

    assert_eq!(result, Ok(()));
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "typing the close chord and the run's own close are one request"
    );
}
