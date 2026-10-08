//! `MouseInteractionTests.ValidateEnterAndLeaveEventsWhenCoverd` and
//! `ValidateEnterAndLeaveInOverlapArea`, split out of
//! `mouse_interaction_tests.rs` (the class's shared helpers, including
//! [`super::watch`], live there).
//!
//! A widget covered by a sibling drawn above it is still under the mouse
//! (C#'s `UnderMouseNotFirst`): it and its children get the bounds events
//! (`Event::MouseEnter`/`MouseLeave`) but never `MouseOver`/`MouseOut`.

use super::*;

#[test]
fn validate_enter_and_leave_events_when_coverd() {
    // A widget contains two children the second completely covering the first.
    // When the mouse moves into the first it should not receive an enter event only a bounds enter event.
    // When the mouse move out of the first it should receive only a bounds exit, not an exit.
    let mut container = Box::new(
        ProbeWidget::new("container")
            .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
            .capturing_presses(),
    );

    let (mut covered_widget, covered) = watch(
        ProbeWidget::new("coveredWidget")
            .with_bounds(Rect::new(20.0, 20.0, 160.0, 160.0))
            .capturing_presses(),
    );

    // C# `coveredChildWidget.BoundsRelativeToParent = coveredWidget.LocalBounds`.
    let (covered_child_widget, covered_child) = watch(
        ProbeWidget::new("coveredChildWidget")
            .with_bounds(Rect::new(0.0, 0.0, 160.0, 160.0))
            .capturing_presses(),
    );
    covered_widget.add_child(covered_child_widget);
    container.add_child(covered_widget);

    let (cover_widget, cover) = watch(
        ProbeWidget::new("coverWidget")
            .with_bounds(Rect::new(10.0, 10.0, 180.0, 180.0))
            .capturing_presses(),
    );
    container.add_child(cover_widget);
    let (mut container, _) = container_window(container);

    let reset = || {
        for w in [&cover, &covered, &covered_child] {
            w.reset();
        }
    };

    assert!(cover.leave.get() == 0);
    assert!(cover.enter.get() == 0);
    assert!(covered.leave.get() == 0);
    assert!(covered.enter.get() == 0);
    assert!(covered_child.leave.get() == 0);
    assert!(covered_child.enter.get() == 0);

    // put the mouse into the widget but outside the children
    container.on_mouse_move(5.0, 5.0);
    cover.assert_counts(0, 0, 0, 0);
    covered.assert_counts(0, 0, 0, 0);
    covered_child.assert_counts(0, 0, 0, 0);

    // move it into the cover
    reset();
    container.on_mouse_move(15.0, 15.0);
    cover.assert_counts(0, 1, 0, 1);
    covered.assert_counts(0, 0, 0, 0);
    covered_child.assert_counts(0, 0, 0, 0);

    // now move it inside cover and make sure it does not re-trigger either event
    reset();
    container.on_mouse_move(16.0, 15.0);
    cover.assert_counts(0, 0, 0, 0);
    covered.assert_counts(0, 0, 0, 0);
    covered_child.assert_counts(0, 0, 0, 0);

    // now leave and make sure we see the leave
    reset();
    container.on_mouse_move(5.0, 5.0);
    cover.assert_counts(1, 0, 1, 0);
    covered.assert_counts(0, 0, 0, 0);
    covered_child.assert_counts(0, 0, 0, 0);

    // now enter the covered and make sure we only see bounds enter
    reset();
    container.on_mouse_move(25.0, 25.0);
    // now leave only the inside widget and make sure we see the leave
    cover.assert_counts(0, 1, 0, 1);
    covered.assert_counts(0, 0, 0, 1);
    covered_child.assert_counts(0, 0, 0, 1);

    // and a final leave and make sure we only see bounds leave
    reset();
    container.on_mouse_move(5.0, 5.0);
    cover.assert_counts(1, 0, 1, 0);
    covered.assert_counts(0, 0, 1, 0);
    covered_child.assert_counts(0, 0, 1, 0);
}

#[test]
fn validate_enter_and_leave_in_overlap_area() {
    let mut container = Box::new(
        ProbeWidget::new("container")
            .with_bounds(Rect::new(0.0, 0.0, 200.0, 200.0))
            .capturing_presses(),
    );

    let (bottom_widget, bottom) = watch(
        ProbeWidget::new("bottom")
            .with_bounds(Rect::new(10.0, 10.0, 180.0, 180.0))
            .capturing_presses(),
    );
    container.add_child(bottom_widget);

    let (top_widget, top) = watch(
        ProbeWidget::new("top")
            .with_bounds(Rect::new(5.0, 20.0, 185.0, 170.0))
            .capturing_presses(),
    );
    container.add_child(top_widget);
    let (mut container, _) = container_window(container);
    let bottom_handle = child(&container, &[0]);
    let top_handle = child(&container, &[1]);
    let screen = |w: &HeadlessWindow, h: &WidgetHandle| {
        agg_gui_automation::tree_query::screen_rect(w.root(), h).expect("on screen")
    };

    assert!(top.enter.get() == 0);
    assert!(top.leave.get() == 0);
    assert!(top.enter_bounds.get() == 0);
    assert!(top.leave_bounds.get() == 0);

    // move into the bottom widget only
    container.on_mouse_move(1.0, 15.0);
    bottom.assert_counts(0, 0, 0, 0);
    top.assert_counts(0, 0, 0, 0);

    container.on_mouse_move(15.0, 15.0);
    bottom.assert_counts(0, 1, 0, 1);
    top.assert_counts(0, 0, 0, 0);

    // clear our states
    bottom.reset();
    top.reset();
    // move out of the bottom widget only
    container.on_mouse_move(1.0, 15.0);
    bottom.assert_counts(1, 0, 1, 0);
    top.assert_counts(0, 0, 0, 0);

    // move to just outside both widgets
    container.on_mouse_move(1.0, 25.0);
    assert!(!screen(&container, &bottom_handle).contains(agg_gui::Point::new(1.0, 25.0)));
    assert!(!screen(&container, &top_handle).contains(agg_gui::Point::new(1.0, 25.0)));
    // clear our states
    bottom.reset();
    top.reset();
    // move over the top widget when it is over the bottom widget (only the top should see this)
    container.on_mouse_move(15.0, 25.0);
    bottom.assert_counts(0, 0, 0, 1);
    top.assert_counts(0, 1, 0, 1);

    // clear our states
    bottom.reset();
    top.reset();
    // move out of the top widget into the bottom
    container.on_mouse_move(15.0, 15.0);
    bottom.assert_counts(0, 1, 0, 0);
    top.assert_counts(1, 0, 1, 0);

    // clear our states
    bottom.reset();
    top.reset();
    // move back up into the top and make sure we see the leave in the bottom
    container.on_mouse_move(15.0, 25.0);
    bottom.assert_counts(1, 0, 0, 0);
    top.assert_counts(0, 1, 0, 1);
}
