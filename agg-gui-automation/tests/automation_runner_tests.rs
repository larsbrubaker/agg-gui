//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/AutomationRunnerTests.cs`.
//!
//! Only the tests whose subjects have landed are here; the rest of the class
//! (clicks, typing, image waits) arrives with the runner slices listed
//! in `docs/design/gui-automation.md`. C#'s `SystemWindow` is an
//! [`AutomationWindow`], built on the run's own UI thread; C#'s captured
//! locals that the body sets are shared atomics, as the body runs on that
//! thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agg_gui::widgets::Button;
use agg_gui::{Font, Rect, Size, Widget};
use agg_gui_automation::runner::DEFAULT_WIDGET_WAIT_SECONDS;
use agg_gui_automation::{
    show_window_and_execute_tests, static_delay, AutomationError, AutomationWindow, RunOptions,
};

#[test]
fn static_delay_expires_on_total_elapsed_time_not_the_seconds_component() {
    let timer = Instant::now();

    let satisfied = static_delay(|| false, 0.2, 10);

    assert!(!satisfied);
    assert!(
        timer.elapsed().as_secs_f64() < 0.9,
        "a .2 second wait must expire on total elapsed time, not on whole seconds ticking over"
    );
}

/// A zero-second wait is a single look: it answers whether the widget is
/// there right now.
///
/// The waits used to decide their answer by the clock (any elapsed time past
/// secondsToWait meant "not found"), so with 0 seconds they reported a
/// missing widget even when it was on screen.
#[test]
fn zero_second_waits_report_what_is_there_now() {
    show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let mut system_window = AutomationWindow::new(300.0, 200.0);

            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            system_window.add_child(Box::new(placed_button("present", 10.0, 40.0, font)));
            (system_window, ())
        },
        |test_runner, _| {
            test_runner.wait_for_name("present", DEFAULT_WIDGET_WAIT_SECONDS);

            assert!(
                test_runner.wait_for_name("present", 0.0),
                "the widget is on screen, so a single look finds it"
            );
            assert!(
                test_runner.name_exists("present", 0.0, true),
                "NameExists is WaitForName by another name"
            );
            assert!(
                !test_runner.wait_for_name("absent", 0.0),
                "no widget has that name"
            );
            assert!(
                test_runner.wait_for_widget_disappear("absent", 0.0),
                "a widget that is not there has already disappeared"
            );
            assert!(
                !test_runner.wait_for_widget_disappear("present", 0.0),
                "the widget is still on screen"
            );

            assert!(
                static_delay(|| true, 0.0, 10),
                "a condition already met is met, however short the wait"
            );

            test_runner.wait_for_widget_enabled("present", 0.0);

            test_runner.mark_test_complete();
        },
    )
    .expect("zero-second waits report what is there now");
}

/// The test's budget measures the test, not the window coming up before it.
///
/// Bringing a window up creates its GPU device, and on a loaded machine
/// bring-up was measured at up to 14s, almost all of it wgpu device creation,
/// while the PopupAnchorTests body it preceded took about 3s - so a 25s test
/// that needed 3s timed out in the full suite. The slow Load here stands in
/// for that device creation: the body itself is instant, so the only way this
/// run can time out is if the load is charged to the test.
#[test]
fn window_load_time_is_not_charged_to_the_test_budget() {
    let body_ran = Arc::new(AtomicBool::new(false));
    let body_flag = Arc::clone(&body_ran);

    show_window_and_execute_tests(
        RunOptions::with_budget(1.0),
        || {
            let system_window = AutomationWindow::new(300.0, 200.0)
                .on_load(|_| thread::sleep(Duration::from_millis(1500)));
            (system_window, ())
        },
        move |test_runner, _| {
            body_flag.store(true, Ordering::SeqCst);
            test_runner.mark_test_complete();
        },
    )
    .expect("the run must not be charged for its window's load");

    assert!(body_ran.load(Ordering::SeqCst));
}

#[test]
fn automation_runner_timeout_test() {
    // Ensure AutomationRunner returns timeout errors
    let result = show_window_and_execute_tests(
        RunOptions {
            // Timeout after 1 second
            secs_to_test_failure: 1.0,
            // The timeout is what this test asserts, so the runner must not
            // treat the run as a hang worth reporting.
            timeout_is_the_expected_outcome: true,
            ..RunOptions::default()
        },
        || {
            let mut system_window = AutomationWindow::new(300.0, 200.0);

            let font = Arc::new(agg_gui::fonts::standard_ui_font());
            system_window.add_child(Box::new(left_button(font)));
            (system_window, ())
        },
        |test_runner, _| {
            // Test method that runs for 10+ seconds
            thread::sleep(Duration::from_secs(10));
            test_runner.mark_test_complete();
        },
    );

    // Should have returned a timeout
    assert_eq!(result.err(), Some(AutomationError::Timeout));
}

/// C# `new Button("left", 10, 40) { Name = "left" }`: a button at (10, 40)
/// at its own size.
fn left_button(font: Arc<Font>) -> Button {
    placed_button("left", 10.0, 40.0, font)
}

/// C# `new Button(text, x, y) { Name = text }`: a button named and labelled
/// `text` at (`x`, `y`), at its own size.
fn placed_button(text: &str, x: f64, y: f64, font: Arc<Font>) -> Button {
    let mut button = Button::new(text, font).with_name(text);
    let size = button.layout(Size::new(f64::MAX, f64::MAX));
    button.set_bounds(Rect::new(x, y, size.width, size.height));
    button
}
