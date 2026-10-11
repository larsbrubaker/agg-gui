//! agg-sharp `FlowLayoutTests` (`Tests/Agg.Tests/Agg Automation
//! Tests/FlowLayoutTests.cs`): margins, padding, nesting and visibility
//! changes of `FlowLayoutWidget`, ported to `FlexColumn`/`FlexRow`.  The
//! anchor-driven half of the class (fit-or-stretch, anchored children,
//! `ChildHAnchorPriority`, `TestVAnchorCenter`) is in
//! `flow_layout_tests_anchors.rs`; the class is split only for the 800-line
//! limit.
//!
//! Conversions (C# → agg-gui):
//! - C#'s plain `GuiWidget` parent is an `AbsoluteLayout`; a top-level C#
//!   widget is laid out at its size with `support::lay`.  Layout is a pass
//!   here, not a side effect of a property setter, so each C# property change
//!   is followed by a fresh layout of the root before the C# assertion.
//! - A `FlowLayoutWidget` has no inter-child gap (`with_gap(0.0)`) and its C#
//!   default `HAnchor.Fit` is `with_fit_width(true)`.
//! - A C# `Button` is a fixed box of a button's size: the tests read only its
//!   size.  `Visible = false` is a `Conditional` whose cell the test clears.
//! - The C# image comparisons (control picture drawn by hand vs. the flow's
//!   picture) compare the children's boxes instead: the pictures are equal
//!   exactly when every box is.  agg-gui's flex layout snaps each child's
//!   origin and size to whole logical units (`flex.rs`), so a C# edge with a
//!   fractional padding/margin is matched rounded, within one unit where two
//!   nested flows each snap (`assert_rect_near`).
//! - The resize-and-back legs ("make sure it can resize without breaking")
//!   compare a snapshot of every box before and after.
//!
//! Where the C# expectation is a C# bug, agg-gui keeps the correct
//! behaviour, pinned by a `rust_only_*` test:
//! - **An unaligned child of a horizontal flow sits one padding and its margin
//!   above the flow's bottom.**  C# (`LayoutEngineFlow.ApplyVAnchorToChild` →
//!   `LayoutEngineSimpleAlign`) leaves a child without a vertical anchor at
//!   y = 0, inside the flow's padding and ignoring its own margin; C# fixed
//!   the same fault for vertical flows (`HoldUnalignedChildOnePaddingIn`) but
//!   not for horizontal ones.  `LeftToRightTests` and
//!   `NestedFlowWidgetsLeftToRightTests` therefore expect y = padding.Bottom +
//!   margin.Bottom where C#'s control says 0
//!   (`rust_only_an_unaligned_child_of_a_row_keeps_the_padding_below_it`).
//! - The vertical-flow controls that put a child at x = 0 inside the flow's
//!   padding (`NestedLayoutTopToBottomTest`, `NestedFlowWidgetsTopToBottomTest`)
//!   contradict C#'s own `HoldUnalignedChildOnePaddingIn`; their C# drivers
//!   call the async helper without `await`, so the C# assertions never ran.
//!   They expect x = padding.Left + margin.Left here.
//!
//! Driven by direct layout passes, not an automation runner:
//! `SpacingClearedAfterLoadPositionsCorrectly` used `AutomationRunner` only to
//! show its window, and C#'s `Load` handler is the code that runs before the
//! first layout here.
//!
//! Not ported: `RightToLeftTests` and `NestedFlowWidgetsRightToLeftTests`
//! (MatterCAD never builds a `RightToLeft` flow; agg-gui has no reversed
//! row).

use agg_gui::{FlexColumn, FlexRow, HAnchor, Insets, Rect, SizedBox, VAnchor, Widget};

use super::support::*;

const WIDE_W: f64 = 120.0;
const NARROW_W: f64 = 72.0;
const LEFT_W: f64 = 80.0;
const RIGHT_W: f64 = 88.0;

/// The edges of `actual` within `tol` of `expected`.
pub fn assert_rect_near(actual: Rect, expected: Rect, tol: f64, what: &str) {
    let ok = (actual.x - expected.x).abs() <= tol
        && (actual.y - expected.y).abs() <= tol
        && (actual.width - expected.width).abs() <= tol
        && (actual.height - expected.height).abs() <= tol;
    assert!(ok, "{what}: {actual:?} vs expected {expected:?}");
}

/// A C# rectangle from its left/bottom and size.
fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, w, h)
}

fn column_mut<'a>(root: &'a mut dyn Widget, path: &[usize]) -> &'a mut FlexColumn {
    at_mut(root, path)
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<FlexColumn>())
        .expect("a FlexColumn")
}

fn row_mut<'a>(root: &'a mut dyn Widget, path: &[usize]) -> &'a mut FlexRow {
    at_mut(root, path)
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<FlexRow>())
        .expect("a FlexRow")
}

#[test]
fn top_to_bottom_container_applies_expected_margin() {
    let margin_size = 40.0;
    let dimensions = 300.0;
    let child = SizedBox::new()
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH)
        .with_margin(Insets::all(margin_size));
    let column = anchor_all_column(top_to_bottom()).add(Box::new(child));
    let mut outer = container().add(Box::new(column));
    lay(&mut outer, dimensions, dimensions);

    let b = bounds(&outer, &[0, 0]);
    assert_eq!(b.x, margin_size, "Left margin is incorrect");
    assert_eq!(
        b.x + b.width,
        dimensions - margin_size,
        "Right margin is incorrect"
    );
    assert_eq!(
        b.y + b.height,
        dimensions - margin_size,
        "Top margin is incorrect"
    );
    assert_eq!(b.y, margin_size, "Bottom margin is incorrect");
}

#[test]
fn spacing_cleared_after_load_positions_correctly() {
    let initial_spacing = 8.0;
    let spaced_row = || {
        left_to_right()
            .with_fit_width(false)
            .with_margin(Insets::all(initial_spacing))
            .with_inner_padding(Insets::all(initial_spacing))
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::FIT)
            .add(Box::new(gui_widget(45.0, 45.0)))
    };
    let body = anchor_all_column(top_to_bottom())
        .with_margin(Insets::all(initial_spacing))
        .with_inner_padding(Insets::all(initial_spacing))
        .add(Box::new(gui_widget(45.0, 45.0)));
    let column = anchor_all_column(top_to_bottom())
        .add(Box::new(spaced_row()))
        .add(Box::new(body))
        .add(Box::new(spaced_row()));
    let mut window = container().add(Box::new(column));

    // C#'s `Load` handler: remove padding and margin at runtime.
    for i in 0..3 {
        let child = at_mut(&mut window, &[0, i]);
        if let Some(base) = child.widget_base_mut() {
            base.margin = Insets::ZERO;
        }
    }
    row_mut(&mut window, &[0, 0]).inner_padding = Insets::ZERO;
    column_mut(&mut window, &[0, 1]).inner_padding = Insets::ZERO;
    row_mut(&mut window, &[0, 2]).inner_padding = Insets::ZERO;
    lay(&mut window, 700.0, 200.0);

    // Footer should be positioned at Y0 when it is the last item in a
    // TopToBottom flow.
    assert_eq!(bounds(&window, &[0, 2]).y, 0.0);
}

/// C# `NestedLayoutTopToBottomTest`.
fn nested_layout_top_to_bottom_test(control_padding: Insets, button_margin: Insets) {
    let top_bar = left_to_right().add(Box::new(button(WIDE_W, 40.0).with_margin(button_margin)));
    let bottom_bar = left_to_right().add(Box::new(
        button(WIDE_W, BUTTON_H).with_margin(button_margin),
    ));
    let all_buttons = anchor_all_column(top_to_bottom())
        .with_inner_padding(control_padding)
        .add(Box::new(top_bar))
        .add(Box::new(bottom_bar));
    let mut test = container().add(Box::new(all_buttons));
    lay(&mut test, 300.0, 200.0);

    let x = control_padding.left + button_margin.left;
    let top_y = 200.0 - control_padding.top - 40.0 - button_margin.top;
    let bottom_y = top_y - button_margin.vertical() - BUTTON_H;
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_rect_near(
        rect_in_root(&test, &[0, 0, 0]),
        rect(x, top_y, WIDE_W, 40.0),
        1.0,
        &format!("top button, {what}"),
    );
    assert_rect_near(
        rect_in_root(&test, &[0, 1, 0]),
        rect(x, bottom_y, WIDE_W, BUTTON_H),
        1.0,
        &format!("bottom button, {what}"),
    );
}

#[test]
fn nested_layout_top_to_bottom_tests() {
    for (padding, margin) in standard_pairs(fractional_pair()) {
        nested_layout_top_to_bottom_test(padding, margin);
    }
}

#[test]
fn changing_child_visiblity_updates_flow() {
    let (size1_visible, size1) = hideable(Box::new(gui_widget(200.0, 20.0)));
    let flow2 = left_to_right()
        .add(size1)
        .add(Box::new(gui_widget(50.0, 20.0)));
    let flow1 = left_to_right()
        .with_fit_width(false)
        .with_h_anchor(HAnchor::STRETCH)
        .with_inner_padding(Insets::symmetric(3.0, 3.0))
        .add(Box::new(flow2));
    let mut container_control = container().add(Box::new(flow1));
    lay(&mut container_control, 300.0, 200.0);

    assert_eq!(bounds(&container_control, &[0]).width, 300.0);
    assert_eq!(bounds(&container_control, &[0, 0]).width, 50.0 + 200.0);

    size1_visible.set(false);
    lay(&mut container_control, 300.0, 200.0);
    assert_eq!(bounds(&container_control, &[0]).width, 300.0);
    assert_eq!(bounds(&container_control, &[0, 0]).width, 50.0);
}

#[test]
fn changing_child_flow_widget_visiblity_updates_parent_flow() {
    let (size1_visible, size1) = hideable(Box::new(gui_widget(200.0, 20.0)));
    let flow3 = left_to_right().add(size1);
    let flow2 = left_to_right()
        .add(Box::new(flow3))
        .add(Box::new(gui_widget(50.0, 20.0)));
    let flow1 = left_to_right()
        .with_fit_width(false)
        .with_h_anchor(HAnchor::STRETCH)
        .with_inner_padding(Insets::symmetric(3.0, 3.0))
        .add(Box::new(flow2));
    let mut container_control = container().add(Box::new(flow1));
    lay(&mut container_control, 300.0, 200.0);

    assert_eq!(bounds(&container_control, &[0]).width, 300.0);
    assert_eq!(bounds(&container_control, &[0, 0, 0]).width, 200.0);
    assert_eq!(bounds(&container_control, &[0, 0]).width, 50.0 + 200.0);

    size1_visible.set(false);
    lay(&mut container_control, 300.0, 200.0);
    assert_eq!(bounds(&container_control, &[0]).width, 300.0);
    assert_eq!(bounds(&container_control, &[0, 0]).width, 50.0);
}

/// C# `NestedLayoutTopToBottomWithResizeTest`.
fn nested_layout_top_to_bottom_with_resize_test(control_padding: Insets, button_margin: Insets) {
    let all_buttons = top_to_bottom()
        .add(Box::new(left_to_right().add(Box::new(
            button(NARROW_W, BUTTON_H).with_margin(button_margin),
        ))))
        .add(Box::new(left_to_right().add(Box::new(
            button(WIDE_W, BUTTON_H).with_margin(button_margin),
        ))));
    let mut test = container()
        .with_padding(control_padding)
        .add(Box::new(all_buttons));
    lay(&mut test, 300.0, 200.0);
    let control = snapshot(&test);

    lay(&mut test, 310.0, 200.0);
    lay(&mut test, 300.0, 200.0);
    assert_eq!(
        snapshot(&test),
        control,
        "The control should contain the same image after being scaled away and back to the same size."
    );
}

#[test]
fn nested_layout_top_to_bottom_with_resize_tests() {
    for (padding, margin) in standard_pairs(fractional_pair()) {
        nested_layout_top_to_bottom_with_resize_test(padding, margin);
    }
}

/// The two-button row of `LeftToRightTest` / `LeftToRightAnchorLeftBottomTest`.
fn two_button_row(control_padding: Insets, button_margin: Insets, v: VAnchor) -> impl Widget {
    let flow = anchor_all_row(left_to_right())
        .with_inner_padding(control_padding)
        .add(Box::new(
            button(LEFT_W, BUTTON_H)
                .with_margin(button_margin)
                .with_v_anchor(v),
        ))
        .add(Box::new(
            button(RIGHT_W, BUTTON_H)
                .with_margin(button_margin)
                .with_v_anchor(v),
        ));
    container().add(Box::new(flow))
}

/// The two buttons of a left-to-right flow at y = padding.Bottom +
/// margin.Bottom, then the resize-and-back leg.
fn check_two_button_row(mut test: impl Widget, control_padding: Insets, button_margin: Insets) {
    lay(&mut test, 300.0, 200.0);
    let y = control_padding.bottom + button_margin.bottom;
    let x1 = control_padding.left + button_margin.left;
    let x2 = control_padding.left + button_margin.horizontal() + button_margin.left + LEFT_W;
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_rect_near(
        rect_in_root(&test, &[0, 0]),
        rect(x1, y, LEFT_W, BUTTON_H),
        0.5,
        &format!("buttonLeft, {what}"),
    );
    assert_rect_near(
        rect_in_root(&test, &[0, 1]),
        rect(x2, y, RIGHT_W, BUTTON_H),
        0.5,
        &format!("buttonRight, {what}"),
    );

    // Make sure it can resize without breaking.
    let control = snapshot(&test);
    lay(&mut test, 310.0, 200.0);
    assert_ne!(
        snapshot(&test),
        control,
        "The Anchored widget should not be the same size."
    );
    lay(&mut test, 300.0, 200.0);
    assert_eq!(
        snapshot(&test),
        control,
        "The Anchored widget should be in the correct place."
    );
}

#[test]
fn left_to_right_tests() {
    for (padding, margin) in standard_pairs(fractional_pair()) {
        // C#'s buttons have no vertical anchor (`VAnchor.Absolute`); agg-gui's
        // default is FIT, which a row places like BOTTOM (see the header).
        let test = two_button_row(padding, margin, VAnchor::FIT);
        check_two_button_row(test, padding, margin);
    }
}

#[test]
fn left_to_right_anchor_left_bottom_tests() {
    for (padding, margin) in standard_pairs(fractional_pair()) {
        let test = two_button_row(padding, margin, VAnchor::BOTTOM);
        check_two_button_row(test, padding, margin);
    }
}

#[test]
fn rust_only_an_unaligned_child_of_a_row_keeps_the_padding_below_it() {
    for v in [VAnchor::FIT, VAnchor::ABSOLUTE] {
        let mut test = two_button_row(Insets::all(2.0), Insets::all(3.0), v);
        lay(&mut test, 300.0, 200.0);
        assert_eq!(bounds(&test, &[0, 0]).y, 5.0, "{v:?}");
        assert_eq!(bounds(&test, &[0, 1]).y, 5.0, "{v:?}");
    }
}

/// C#'s `IsEqualTo` on two sums of the same fractional terms: the flow adds
/// them in a different order than the test, so equal means equal to the last
/// bit of rounding.
fn assert_same_height(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-9, "{actual} vs {expected}");
}

/// C# `FlowTopBottomAnchorChildrenLeftRightTest`.
fn flow_top_bottom_anchor_children_left_right_test(control_padding: Insets, button_margin: Insets) {
    let mut flow = top_to_bottom()
        .with_inner_padding(control_padding)
        .add(Box::new(
            button(WIDE_W, BUTTON_H)
                .with_h_anchor(HAnchor::LEFT)
                .with_margin(button_margin),
        ));
    let mut correct_height = BUTTON_H + control_padding.vertical() + button_margin.vertical();
    assert_same_height(lay_unparented(&mut flow).height, correct_height);

    flow.push(
        Box::new(
            button(NARROW_W, BUTTON_H)
                .with_margin(button_margin)
                .with_h_anchor(HAnchor::LEFT | HAnchor::RIGHT),
        ),
        0.0,
    );
    correct_height += BUTTON_H + button_margin.vertical();
    assert_same_height(lay_unparented(&mut flow).height, correct_height);

    let flow = flow
        .with_h_anchor(HAnchor::LEFT)
        .with_v_anchor(VAnchor::BOTTOM);
    let mut test = container().add(Box::new(flow));
    lay(&mut test, 300.0, 500.0);

    let x = control_padding.left + button_margin.left;
    let button1_y = control_padding.bottom + button_margin.bottom;
    let wide_y = button1_y + BUTTON_H + button_margin.vertical();
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_rect_near(
        rect_in_root(&test, &[0, 0]),
        rect(x, wide_y, WIDE_W, BUTTON_H),
        1.0,
        &format!("wide button, {what}"),
    );
    assert_rect_near(
        rect_in_root(&test, &[0, 1]),
        rect(x, button1_y, WIDE_W, BUTTON_H),
        1.0,
        &format!("button1, {what}"),
    );

    // Make sure it can resize without breaking.
    let control = snapshot(&test);
    lay(&mut test, 310.0, 500.0);
    lay(&mut test, 300.0, 500.0);
    assert_eq!(
        snapshot(&test),
        control,
        "The Anchored widget should be in the correct place."
    );
}

#[test]
fn anchor_left_right_tests() {
    for (padding, margin) in standard_pairs(fractional_pair()) {
        flow_top_bottom_anchor_children_left_right_test(padding, margin);
    }
}

/// C# `NestedFlowWidgetsTopToBottomTest`.
fn nested_flow_widgets_top_to_bottom_test(control_padding: Insets, button_margin: Insets) {
    let inner = || {
        top_to_bottom().add(Box::new(
            button(WIDE_W, BUTTON_H).with_margin(button_margin),
        ))
    };
    let all = anchor_all_column(top_to_bottom())
        .with_inner_padding(control_padding)
        .add(Box::new(inner()))
        .add(Box::new(inner()));
    let mut test = container().add(Box::new(all));
    lay(&mut test, 300.0, 500.0);

    let x = control_padding.left + button_margin.left;
    let top_y = 500.0 - button_margin.top - control_padding.top - BUTTON_H;
    let bottom_y = top_y - BUTTON_H - button_margin.vertical();
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_rect_near(
        rect_in_root(&test, &[0, 0, 0]),
        rect(x, top_y, WIDE_W, BUTTON_H),
        1.0,
        &format!("buttonTop, {what}"),
    );
    assert_rect_near(
        rect_in_root(&test, &[0, 1, 0]),
        rect(x, bottom_y, WIDE_W, BUTTON_H),
        1.0,
        &format!("buttonBottom, {what}"),
    );
}

#[test]
fn nested_flow_widgets_top_to_bottom_tests() {
    for (padding, margin) in standard_pairs(fractional_pair()) {
        nested_flow_widgets_top_to_bottom_test(padding, margin);
    }
}

/// C# `NestedFlowWidgetsLeftToRightTest`.
fn nested_flow_widgets_left_to_right_test(control_padding: Insets, button_margin: Insets) {
    let inner =
        |w: f64| left_to_right().add(Box::new(button(w, BUTTON_H).with_margin(button_margin)));
    let all = anchor_all_row(left_to_right())
        .with_inner_padding(control_padding)
        .add(Box::new(inner(LEFT_W)))
        .add(Box::new(inner(RIGHT_W)));
    let mut test = container().add(Box::new(all));
    lay(&mut test, 500.0, 300.0);

    let y = control_padding.bottom + button_margin.bottom;
    let left_x = control_padding.left + button_margin.left;
    let right_x = left_x + LEFT_W + button_margin.horizontal();
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_rect_near(
        rect_in_root(&test, &[0, 0, 0]),
        rect(left_x, y, LEFT_W, BUTTON_H),
        1.0,
        &format!("buttonLeft, {what}"),
    );
    assert_rect_near(
        rect_in_root(&test, &[0, 1, 0]),
        rect(right_x, y, RIGHT_W, BUTTON_H),
        1.0,
        &format!("buttonRight, {what}"),
    );
}

#[test]
fn nested_flow_widgets_left_to_right_tests() {
    for (padding, margin) in standard_pairs(fractional_pair()) {
        nested_flow_widgets_left_to_right_test(padding, margin);
    }
}

#[test]
fn flow_with_max_size_child_allocates_to_others() {
    let item = || SizedBox::new().with_h_anchor(HAnchor::STRETCH);
    let flow = anchor_all_row(left_to_right())
        .add(Box::new(item()))
        .add(Box::new(item()));
    let mut c = container().add(Box::new(flow));
    lay(&mut c, 200.0, 300.0);
    assert_eq!(bounds(&c, &[0, 0]).width, 100.0);
    assert_eq!(bounds(&c, &[0, 1]).width, 100.0);

    if let Some(base) = at_mut(&mut c, &[0, 0]).widget_base_mut() {
        base.max_size = agg_gui::Size::new(50.0, 500.0);
    }
    lay(&mut c, 200.0, 300.0);
    assert_eq!(bounds(&c, &[0, 0]).width, 50.0);
    assert_eq!(bounds(&c, &[0, 1]).width, 150.0);
}

#[test]
fn child_visibility_change_cause_resize() {
    let mut c = container().add(Box::new(top_to_bottom()));
    let (item2_visible, item2) = hideable(Box::new(gui_widget(1.0, 30.0)));

    column_mut(&mut c, &[0]).push(Box::new(gui_widget(1.0, 20.0)), 0.0);
    lay(&mut c, 640.0, 480.0);
    assert_eq!(bounds(&c, &[0]).height, 20.0);
    column_mut(&mut c, &[0]).push(item2, 0.0);
    lay(&mut c, 640.0, 480.0);
    assert_eq!(bounds(&c, &[0]).height, 50.0);
    column_mut(&mut c, &[0]).push(Box::new(gui_widget(1.0, 40.0)), 0.0);
    lay(&mut c, 640.0, 480.0);
    assert_eq!(bounds(&c, &[0]).height, 90.0);

    item2_visible.set(false);
    lay(&mut c, 640.0, 480.0);
    assert_eq!(bounds(&c, &[0]).height, 60.0);
}

/// C#'s text and button heights in `EnsureCorrectSizeOnChildrenVisibleChange`
/// (the nested block's expected values: 13 + 33 + 21 = 67, 13 + 21 = 34).
const TOP_TEXT_H: f64 = 13.0;
const HIDDEN_BUTTON_H: f64 = 33.0;
const BOTTOM_TEXT_H: f64 = 21.0;
const CHECK_BOX_H: f64 = 16.0;

#[test]
fn ensure_correct_size_on_children_visible_change() {
    // Just one column changes correctly.
    {
        let mut test_column = top_to_bottom().add(Box::new(gui_widget(10.0, 10.0)));
        assert_eq!(lay_unparented(&mut test_column).height, 10.0);
        let (item2_visible, item2) = hideable(Box::new(gui_widget(11.0, 11.0)));
        test_column.push(item2, 0.0);
        assert_eq!(lay_unparented(&mut test_column).height, 21.0);
        test_column.push(Box::new(gui_widget(12.0, 12.0)), 0.0);
        assert_eq!(lay_unparented(&mut test_column).height, 33.0);
        item2_visible.set(false);
        assert_eq!(lay_unparented(&mut test_column).height, 22.0);
        item2_visible.set(true);
        assert_eq!(lay_unparented(&mut test_column).height, 33.0);
    }

    // Nested columns change correctly.
    {
        let (hide_visible, thing_to_hide) = hideable(Box::new(button(90.0, HIDDEN_BUTTON_H)));
        let top_left_stuff = top_to_bottom()
            .add(Box::new(SizedBox::fixed(100.0, TOP_TEXT_H)))
            .add(thing_to_hide)
            .add(Box::new(SizedBox::fixed(120.0, BOTTOM_TEXT_H)));
        let left_column = top_to_bottom().add(Box::new(top_left_stuff));
        let right_column = top_to_bottom().add(Box::new(SizedBox::fixed(80.0, CHECK_BOX_H)));
        let two_columns = left_to_right()
            .with_v_anchor(VAnchor::TOP)
            .add(Box::new(left_column))
            .add(Box::new(right_column));
        let mut everything = container().add(Box::new(two_columns));
        lay(&mut everything, 500.0, 500.0);

        let top = |r: Rect| r.y + r.height;
        assert_eq!(bounds(&everything, &[0, 0, 0, 0]).y, 54.0);
        assert_eq!(top(bounds(&everything, &[0])), 500.0);
        assert_eq!(top(bounds(&everything, &[0, 0])), 67.0);
        assert_eq!(bounds(&everything, &[0, 0]).y, 0.0);
        assert_eq!(top(bounds(&everything, &[0, 0, 0])), 67.0);
        assert_eq!(bounds(&everything, &[0, 0, 0]).height, 67.0);
        assert_eq!(bounds(&everything, &[0, 0]).height, 67.0);

        // C# `hideCheckBox.Checked = true` hides the button.
        hide_visible.set(false);
        lay(&mut everything, 500.0, 500.0);
        assert_eq!(bounds(&everything, &[0, 0, 0, 0]).y, 21.0);
        assert_eq!(bounds(&everything, &[0, 0]).y, 0.0);
        assert_eq!(bounds(&everything, &[0, 0, 0]).height, 34.0);
        assert_eq!(bounds(&everything, &[0, 0]).height, 34.0);
    }
}
