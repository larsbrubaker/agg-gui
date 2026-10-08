//! Rust-only tests of the runner's `scroll_into_view` (agg-gui-automation
//! `src/runner/scroll.rs`) and the `Widget::scroll_rect_into_view` it calls
//! on a `ScrollView` and a `TextArea` (C#'s `ScrollableWidget.ScrollIntoView`
//! with `ScrollAmount.Minimum`).

use std::sync::Arc;

use agg_gui::widgets::{ScrollView, TextArea};
use agg_gui::{Rect, Size, Widget};
use agg_gui_automation::{
    show_window_and_execute_tests, AutomationWindow, ProbeWidget, RunOptions,
};

/// A 100 × 100 scroll view over a 100 × 400 column of four named 100-tall rows,
/// "row0" at the top.
fn scrolling_rows() -> Box<dyn Widget> {
    let mut content = ProbeWidget::new("content").with_bounds(Rect::new(0.0, 0.0, 100.0, 400.0));
    for i in 0..4 {
        let y = 300.0 - i as f64 * 100.0;
        content.add_child(Box::new(
            ProbeWidget::new(format!("row{i}")).with_bounds(Rect::new(0.0, y, 100.0, 100.0)),
        ));
    }
    let mut view = ScrollView::new(Box::new(content)).with_name("scroller");
    view.set_bounds(Rect::new(0.0, 0.0, 100.0, 100.0));
    Box::new(view)
}

fn offset(runner: &agg_gui_automation::AutomationRunner) -> f64 {
    let root = runner.driver().root();
    let handle = agg_gui_automation::tree_query::find_by_name(root, "scroller")
        .into_iter()
        .next()
        .expect("the scroll view is in the window");
    handle
        .downcast::<ScrollView>(root)
        .expect("a ScrollView")
        .scroll_offset()
}

#[test]
fn rust_only_scroll_into_view_does_the_minimum_scroll_in_the_nearest_scroll_view() {
    let offsets = show_window_and_execute_tests(
        RunOptions::default(),
        || {
            let mut window = AutomationWindow::new(300.0, 200.0);
            window.add_child(scrolling_rows());
            (window, ())
        },
        |runner, _| {
            runner.delay(0.1);
            let start = offset(runner);
            runner.scroll_into_view("row2");
            let to_row2 = offset(runner);
            runner.scroll_into_view("row2");
            let again = offset(runner);
            runner.scroll_into_view("row0");
            let back = offset(runner);
            runner.mark_test_complete();
            (start, to_row2, again, back)
        },
    )
    .expect("the run completes");
    // row2 spans 100..200 below the top: raising the content 200 puts its
    // bottom on the view's bottom; a second call finds it in view.
    assert_eq!(offsets, (0.0, 200.0, 200.0, 0.0));
}

#[test]
fn rust_only_text_area_scrolls_a_rect_into_view() {
    let mut area = TextArea::new(Arc::new(agg_gui::fonts::standard_ui_font()))
        .with_font_size(10.0)
        .with_padding(0.0)
        .with_line_spacing(1.0)
        .with_text("0\n1\n2\n3\n4\n5\n6\n7\n8\n9");
    area.layout(Size::new(100.0, 30.0));
    area.set_bounds(Rect::new(0.0, 0.0, 100.0, 30.0));
    // Line 5 sits 50..60 below the top: local y -30..-20.
    assert!(area.scroll_rect_into_view(Rect::new(0.0, -30.0, 10.0, 10.0)));
    assert_eq!(area.scroll_offset(), 30.0);
    // A plain widget does not scroll.
    let mut probe = ProbeWidget::new("p");
    assert!(!probe.scroll_rect_into_view(Rect::new(0.0, 0.0, 1.0, 1.0)));
}
