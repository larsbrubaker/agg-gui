//! Rust-only tests of clicks whose count the window works out
//! (`ClickOpts::platform_counts_clicks`, `runner/pointer.rs`, and
//! `InputMethod::platform_counted_mouse_event`, `input.rs`): the press goes to
//! the input forwarder with `ClickCount::Auto`, so two quick clicks on one
//! spot are a double click and a press elsewhere, or a pause past the
//! double-click time, keeps them single.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::event::{Event, EventResult};
use agg_gui::Rect;
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationRunner, AutomationWindow, ClickOpts, ProbeWidget,
    RunOptions,
};

/// The click count each press reported to the widget it landed on, in order.
type Counts = Rc<RefCell<Vec<u32>>>;

/// A probe named `name` that takes every press and records the click count
/// the product reports for it (`agg_gui::event::current_click_count`).
fn counting_probe(name: &str, bounds: Rect, counts: &Counts) -> ProbeWidget {
    let counts = Rc::clone(counts);
    ProbeWidget::new(name)
        .with_bounds(bounds)
        .on_event_with(move |event| match event {
            Event::MouseDown { .. } => {
                counts
                    .borrow_mut()
                    .push(agg_gui::event::current_click_count());
                EventResult::Consumed
            }
            Event::MouseUp { .. } => EventResult::Consumed,
            _ => EventResult::Ignored,
        })
}

/// A 300 × 200 window with a counting `target` and, far from it, a counting
/// `elsewhere`; both record into the returned list.
fn two_targets() -> (AutomationWindow, Counts) {
    let counts = Counts::default();
    let mut window = AutomationWindow::new(300.0, 200.0);
    window.add_child(Box::new(counting_probe(
        "target",
        Rect::new(20.0, 40.0, 80.0, 20.0),
        &counts,
    )));
    window.add_child(Box::new(counting_probe(
        "elsewhere",
        Rect::new(200.0, 140.0, 80.0, 20.0),
        &counts,
    )));
    (window, counts)
}

fn platform_click(runner: &mut AutomationRunner, name: &str) {
    runner.click_by_name_with(
        name,
        &ClickOpts {
            platform_counts_clicks: true,
            ..ClickOpts::default()
        },
    );
}

#[test]
fn rust_only_two_quick_platform_counted_clicks_on_one_widget_are_a_double_click() {
    let counts =
        show_window_and_execute_tests(RunOptions::default(), two_targets, |runner, counts| {
            platform_click(runner, "target");
            platform_click(runner, "target");
            assert_eq!(runner.input_method().click_count(), 2);
            assert_eq!(runner.driver().forwarder().click_count(), 2);
            runner.mark_test_complete();
            counts.borrow().clone()
        })
        .expect("the run completes");
    assert_eq!(counts, vec![1, 2]);
}

#[test]
fn rust_only_a_platform_counted_click_elsewhere_in_between_keeps_them_single_clicks() {
    let counts =
        show_window_and_execute_tests(RunOptions::default(), two_targets, |runner, counts| {
            platform_click(runner, "target");
            platform_click(runner, "elsewhere");
            platform_click(runner, "target");
            assert_eq!(runner.input_method().click_count(), 1);
            // With nothing in between, the next quick click continues it.
            platform_click(runner, "target");
            runner.mark_test_complete();
            counts.borrow().clone()
        })
        .expect("the run completes");
    assert_eq!(counts, vec![1, 1, 1, 2]);
}

#[test]
fn rust_only_platform_counted_clicks_further_apart_than_the_double_click_time_stay_single() {
    let counts =
        show_window_and_execute_tests(RunOptions::default(), two_targets, |runner, counts| {
            platform_click(runner, "target");
            let double_click_time = runner.driver().forwarder().click_policy().double_click_time;
            runner.delay(double_click_time.as_secs_f64());
            platform_click(runner, "target");
            assert_eq!(runner.driver().forwarder().click_count(), 1);
            // Without the pause, the next quick click continues it.
            platform_click(runner, "target");
            runner.mark_test_complete();
            counts.borrow().clone()
        })
        .expect("the run completes");
    assert_eq!(counts, vec![1, 1, 2]);
}

/// The runner's own clicks still state their counts: two quick ones on one
/// spot are two single clicks, as C#'s automation sends them.
#[test]
fn rust_only_clicks_by_default_state_their_count_however_quick() {
    let counts =
        show_window_and_execute_tests(RunOptions::default(), two_targets, |runner, counts| {
            runner.click_by_name("target");
            runner.click_by_name("target");
            runner.mark_test_complete();
            counts.borrow().clone()
        })
        .expect("the run completes");
    assert_eq!(counts, vec![1, 1]);
}
