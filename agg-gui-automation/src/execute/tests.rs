//! Unit tests of `show_window_and_execute_tests` that need its crate-private
//! bring-up floor: a window that does not load within the bring-up budget
//! fails the run with [`AutomationError::LoadTimeout`] and its body never
//! runs. The real floor ([`MIN_BRING_UP`], 30 s) is lowered so the timeout
//! is reached in well under a second.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::*;

#[test]
fn rust_only_a_window_that_does_not_load_in_its_bring_up_budget_is_a_load_timeout() {
    let body_ran = Arc::new(AtomicBool::new(false));
    let body_flag = Arc::clone(&body_ran);
    let started = Instant::now();

    let result = run_with_bring_up_floor(
        RunOptions {
            secs_to_test_failure: 0.1,
            // The timeout is what this test asserts; keep the load watchdog quiet.
            timeout_is_the_expected_outcome: true,
            ..RunOptions::default()
        },
        Duration::from_millis(200),
        || {
            let window = AutomationWindow::new(300.0, 200.0)
                .on_load(|_| thread::sleep(Duration::from_millis(1500)));
            (window, ())
        },
        move |runner, _| {
            body_flag.store(true, Ordering::SeqCst);
            runner.mark_test_complete();
        },
    );

    assert_eq!(result.err(), Some(AutomationError::LoadTimeout));
    assert!(
        started.elapsed() < Duration::from_millis(1200),
        "the run must give up at the bring-up budget, not wait for the load"
    );

    // Once the load finishes, the cancelled run still must not start its body.
    thread::sleep(Duration::from_millis(1600));
    assert!(!body_ran.load(Ordering::SeqCst));
}

#[test]
fn rust_only_the_bring_up_budget_is_the_test_budget_when_that_is_longer_than_the_floor() {
    // A 0.6 s test budget over a 0.2 s floor: a 0.4 s load is inside it.
    let result = run_with_bring_up_floor(
        RunOptions::with_budget(0.6),
        Duration::from_millis(200),
        || {
            let window = AutomationWindow::new(300.0, 200.0)
                .on_load(|_| thread::sleep(Duration::from_millis(400)));
            (window, ())
        },
        |runner, _| runner.mark_test_complete(),
    );

    assert_eq!(result, Ok(()));
}
