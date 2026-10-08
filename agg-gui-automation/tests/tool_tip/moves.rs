//! `ToolTipTests`: moving between tipped widgets, onto a covering widget with
//! no tip, and `Clear` dropping an armed tip.

use super::{
    controller, create_two_child_window, tipped_free, AUTO_POP_DELAY_MS, INITIAL_DELAY_MS,
    MIN_MS_TIME_TO_RESPOND, MIN_MS_TO_BIAS, RESHOW_DELAY_MS, TOOL_TIP1_TEXT, TOOL_TIP2_TEXT,
};

#[test]
fn move_from_tool_tip_to_tool_tip() {
    let mut w = create_two_child_window();

    // move into the first widget
    w.mouse_move(11.0, 11.0);
    w.invoke_pending_actions();

    // sleep long enough to show the tool tip
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS + MIN_MS_TO_BIAS);

    // make sure the tool tip came up
    assert!(w.tip_children() == 1);
    assert!(w.show_count() == 1);
    assert!(controller::current_text() == TOOL_TIP1_TEXT);

    // move off the first widget
    w.mouse_move(29.0, 29.0);
    w.sleep_then_invoke_pending_actions(MIN_MS_TIME_TO_RESPOND); // sleep enough for the tool tip to want to respond

    // make sure the first tool tip went away
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());

    // sleep long enough to clear the fast move time
    w.sleep_then_invoke_pending_actions(RESHOW_DELAY_MS * 2);

    // make sure the first tool still gone
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());

    // move onto the other widget
    w.mouse_move(31.0, 31.0);
    w.sleep_then_invoke_pending_actions(MIN_MS_TIME_TO_RESPOND); // sleep enough for the tool tip to want to respond

    // make sure the first tool tip still gone
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());

    // wait 1/2 long enough for the second tool tip to come up
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS / 2 + MIN_MS_TO_BIAS);

    // make sure the second tool tip not showing
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());

    // wait 1/2 long enough for the second tool tip to come up
    w.sleep_then_invoke_pending_actions(AUTO_POP_DELAY_MS / 2 + MIN_MS_TO_BIAS);

    // make sure the tool tip 2 came up
    assert!(w.tip_children() == 1);
    assert!(w.show_count() == 2);
    assert!(controller::current_text() == TOOL_TIP2_TEXT);
}

#[test]
fn move_fast_from_tool_tip_to_tool_tip() {
    let mut w = create_two_child_window();

    // move into the first widget
    w.mouse_move(11.0, 11.0);
    w.invoke_pending_actions();

    // sleep long enough to show the tool tip
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS + MIN_MS_TO_BIAS);

    // make sure the tool tip came up
    assert!(w.tip_children() == 1);
    assert!(w.show_count() == 1);
    assert!(controller::current_text() == TOOL_TIP1_TEXT);

    // wait 1/2 long enough for the tool tip to go away
    w.sleep_then_invoke_pending_actions(AUTO_POP_DELAY_MS / 2 + MIN_MS_TO_BIAS);

    // move onto the other widget
    w.mouse_move(31.0, 31.0);
    w.sleep_then_invoke_pending_actions(MIN_MS_TIME_TO_RESPOND); // sleep enough for the tool tip to want to respond

    // make sure the first tool tip went away
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());

    // wait long enough for the second tool tip to come up
    w.sleep_then_invoke_pending_actions(RESHOW_DELAY_MS + MIN_MS_TO_BIAS);

    // make sure the tool tip 2 came up
    assert!(w.tip_children() == 1);
    assert!(w.show_count() == 2);
    assert!(controller::current_text() == TOOL_TIP2_TEXT);
}

#[test]
fn move_from_tool_tip_to_overlapping_widget_with_no_tool_tip() {
    let mut w = create_two_child_window();

    // A widget with no tooltip that covers the right half of the first widget (added last so it is
    // on top). This is the 'Open Recent' popup over a design tab case: the popup has no tooltip of
    // its own, and it hides the widget that does.
    w.window
        .add_child(tipped_free(agg_gui::Rect::new(14.0, 5.0, 11.0, 20.0)));

    // move into the uncovered part of the first widget
    w.mouse_move(11.0, 11.0);
    w.invoke_pending_actions();

    // sleep long enough to show the tool tip
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS + MIN_MS_TO_BIAS);

    // make sure the tool tip came up
    assert!(w.tip_children() == 1);
    assert!(controller::current_text() == TOOL_TIP1_TEXT);

    // move onto the covering widget (still inside the first widget's bounds)
    w.mouse_move(16.0, 16.0);
    w.sleep_then_invoke_pending_actions(MIN_MS_TIME_TO_RESPOND); // sleep enough for the tool tip to want to respond

    // the first widget's tool tip must go away even though the mouse is still over its bounds
    assert!(w.tip_children() == 0);
    assert!(w.pop_count() == 1);
    assert!(controller::current_text().is_empty());

    // and it must not come back while we hover the widget that has no tool tip
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS + MIN_MS_TO_BIAS);

    assert!(w.tip_children() == 0);
    assert!(controller::current_text().is_empty());
}

/// Opening a menu calls Clear() so no tooltip floats over it. The tooltip that is dangerous is often
/// the one the mouse armed on its way to the menu item and that has not appeared yet - it would pop
/// on top of the menu a fraction of a second after the menu opened.
#[test]
fn clear_also_drops_a_tool_tip_that_is_armed_but_not_yet_shown() {
    let mut w = create_two_child_window();

    // hover the first widget, but not long enough for its tool tip to come up
    w.mouse_move(11.0, 11.0);
    w.invoke_pending_actions();

    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS / 2);

    assert!(w.tip_children() == 0);
    assert!(w.show_count() == 0);

    // this is what showing a menu does
    controller::clear();

    // wait well past the point the armed tool tip would have shown
    w.sleep_then_invoke_pending_actions(INITIAL_DELAY_MS + MIN_MS_TO_BIAS);

    assert!(w.tip_children() == 0);
    assert!(w.show_count() == 0);
    assert!(controller::current_text().is_empty());
}
