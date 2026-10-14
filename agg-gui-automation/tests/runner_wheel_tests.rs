//! Rust-only tests of the runner's mouse wheel (`runner/wheel.rs`): a
//! `ScrollView` turned N notches by `scroll_wheel_by_name` scrolls exactly as
//! far as one real wheel event of N notches (what the shells forward) moves
//! it, the turn lands where the pointer is, and Shift held through the
//! runner turns it sideways.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui::widgets::ScrollView;
use agg_gui::{Rect, Widget};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationRunner, AutomationWindow, ModifierKeys, Point2D,
    ProbeWidget, RunOptions,
};

/// A 100 × 100 scroll view at the window's lower-left over a 300 × 1000
/// content, scrolling both ways; its horizontal offset is published to
/// `h_offset`.
fn scroller(h_offset: &Rc<Cell<f64>>) -> Box<dyn Widget> {
    let content = ProbeWidget::new("content").with_bounds(Rect::new(0.0, 0.0, 300.0, 1000.0));
    let mut view = ScrollView::new(Box::new(content))
        .with_name("scroller")
        .horizontal(true)
        .with_h_offset_cell(Rc::clone(h_offset));
    view.set_bounds(Rect::new(0.0, 0.0, 100.0, 100.0));
    Box::new(view)
}

fn window() -> (AutomationWindow, Rc<Cell<f64>>) {
    let h_offset = Rc::new(Cell::new(0.0));
    let mut window = AutomationWindow::new(300.0, 200.0);
    window.add_child(scroller(&h_offset));
    (window, h_offset)
}

/// The scroll view's (vertical, horizontal) offsets.
fn offsets(runner: &AutomationRunner, h_offset: &Rc<Cell<f64>>) -> (f64, f64) {
    let root = runner.driver().root();
    let handle = agg_gui_automation::tree_query::find_by_name(root, "scroller")
        .into_iter()
        .next()
        .expect("the scroll view is in the window");
    let view = handle.downcast::<ScrollView>(root).expect("a ScrollView");
    (view.scroll_offset(), h_offset.get())
}

/// The offsets one real wheel event of `notches` gives the scroll view, sent
/// straight to the `App` (as the shells' input forwarder ends up calling it)
/// at the view's center.
fn real_wheel_offsets(notches: f64) -> (f64, f64) {
    show_window_and_execute_tests(RunOptions::default(), window, move |runner, h| {
        runner.wait_for_draw();
        // The view's center (50, 50) Y-up is pointer (50, 150).
        runner.app_mut().on_mouse_wheel(50.0, 150.0, notches);
        let got = offsets(runner, h);
        runner.mark_test_complete();
        got
    })
    .expect("the real wheel run completes")
}

#[test]
fn rust_only_scroll_wheel_by_name_scrolls_as_far_as_a_real_wheel_of_as_many_notches() {
    let (by_runner, start) =
        show_window_and_execute_tests(RunOptions::default(), window, |runner, h| {
            runner.wait_for_draw();
            let start = offsets(runner, h);
            runner.scroll_wheel_by_name("scroller", -3.0);
            assert_eq!(runner.current_mouse_position(), Point2D::new(50, 150));
            let got = offsets(runner, h);
            runner.mark_test_complete();
            (got, start)
        })
        .expect("the runner's wheel run completes");
    assert_eq!(start, (0.0, 0.0));
    // Three notches down: three times the pixels of one notch.
    assert_eq!(by_runner, (3.0 * agg_gui::wheel::PIXELS_PER_NOTCH, 0.0));
    assert_eq!(by_runner, real_wheel_offsets(-3.0));
}

#[test]
fn rust_only_scroll_wheel_turns_at_the_pointer_and_shift_turns_it_sideways() {
    show_window_and_execute_tests(RunOptions::default(), window, |runner, h| {
        runner.wait_for_draw();
        // Outside the scroll view: nothing under the pointer scrolls.
        runner.set_mouse_cursor_position(250, 20);
        runner.scroll_wheel(-2.0);
        assert_eq!(offsets(runner, h), (0.0, 0.0));

        // Over it: down two notches, then back up one.
        runner.set_mouse_cursor_position(50, 150);
        runner.scroll_wheel(-2.0);
        runner.scroll_wheel(1.0);
        let notch = agg_gui::wheel::PIXELS_PER_NOTCH;
        assert_eq!(offsets(runner, h), (notch, 0.0));

        // Shift held: the wheel moves the view sideways instead.
        runner.press_modifier_keys(ModifierKeys::SHIFT);
        runner.scroll_wheel(-1.0);
        runner.release_modifier_keys(ModifierKeys::SHIFT);
        assert_eq!(offsets(runner, h), (notch, notch));
        runner.mark_test_complete();
    })
    .expect("the wheel turns where the pointer is");
}

#[test]
#[should_panic(expected = "ScrollWheel Failed: the pointer")]
fn rust_only_scroll_wheel_outside_the_window_fails() {
    let h_offset = Rc::new(Cell::new(0.0));
    let driver = agg_gui_automation::HeadlessDriver::new(scroller(&h_offset), 300, 200);
    let mut runner = AutomationRunner::attach(driver, Default::default());
    runner.set_mouse_cursor_position(-10, 20);
    runner.scroll_wheel(1.0);
}
