//! agg-sharp `FlowLayoutTests`, the anchor-driven half: fit-or-stretch flows
//! in a plain parent, stretched children sharing a flow's free space, and
//! anchored children on the cross axis.  The conversions (C#'s `GuiWidget`
//! parent → `AbsoluteLayout`, a layout pass after each C# property change,
//! `GuiWidget(w, h)` keeping its construction size as its minimum) are listed
//! in `flow_layout_tests.rs`, the other half of the class.
//!
//! `LeftRightWithAnchorLeftRightChildTest`, `RightLeftWithAnchorLeftRightChildTest`,
//! `TopBottomWithAnchorBottomTopChildTest` and `BottomTopWithAnchorBottomTopChildTest` compare boxes exactly, as C#
//! does.  Their unaligned first and last children differ from C#'s control
//! as described in `flow_layout_tests.rs`: the rows' sit at y =
//! padding.Bottom + margin.Bottom (C#'s horizontal flows leave them at 0),
//! the columns' at x = padding.Left + margin.Left (what C#'s own
//! `HoldUnalignedChildOnePaddingIn` does; the control's x = 0 was never
//! checked, the C# driver doesn't await its helper).
//!
//! `NestedMinFitOrStretchToChildrenParentWidth`'s fourth block follows a
//! `return;` in C# ("this last test does not work yet") and never runs; it is
//! not ported.
//!
//! `TestVAnchorCenter` uses a 24-unit button beside an 18-unit text box (C#'s
//! real `Button`/`TextEditWidget` heights are font-dependent): agg-gui snaps a
//! centred child to whole units, so an odd height difference would leave the
//! extra unit below it.

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{FlexRow, HAnchor, Insets, Rect, Size, SizedBox, VAnchor, Widget};

use super::support::*;

fn row_mut<'a>(root: &'a mut dyn Widget, path: &[usize]) -> &'a mut FlexRow {
    at_mut(root, path)
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<FlexRow>())
        .expect("a FlexRow")
}

fn width(root: &dyn Widget, path: &[usize]) -> f64 {
    bounds(root, path).width
}

#[test]
fn nested_max_fit_or_stretch_to_children_parent_width() {
    for child_anchor in [HAnchor::STRETCH, HAnchor::MAX_FIT_OR_STRETCH] {
        let flow = left_to_right()
            .with_h_anchor(HAnchor::MAX_FIT_OR_STRETCH)
            .add(Box::new(gui_widget(20.0, 20.0).with_h_anchor(child_anchor)))
            .add(Box::new(gui_widget(10.0, 10.0)));
        let mut c = container().add(Box::new(flow));
        for w in [300.0, 350.0, 310.0] {
            lay(&mut c, w, 200.0);
            assert_eq!(width(&c, &[0]), w, "{child_anchor:?}");
            assert_eq!(
                width(&c, &[0, 0]) + width(&c, &[0, 1]),
                w,
                "{child_anchor:?}"
            );
        }
    }
}

#[test]
fn nested_min_fit_or_stretch_to_children_parent_width() {
    // The flow's single child is fixed; the main container starts out wider
    // (300) and then as the constraint (123).
    for (start, first) in [(300.0, 150.0), (123.0, 123.0)] {
        let flow = left_to_right().with_h_anchor(HAnchor::MIN_FIT_OR_STRETCH);
        let mut c = container().add(Box::new(flow));
        lay(&mut c, start, 200.0);
        assert_eq!(width(&c, &[0]), 0.0);

        row_mut(&mut c, &[0]).push(Box::new(SizedBox::fixed(150.0, 10.0)), 0.0);
        lay(&mut c, start, 200.0);
        assert_eq!(width(&c, &[0]), first);

        lay(&mut c, 100.0, 200.0);
        assert_eq!(width(&c, &[0]), 100.0);
        lay(&mut c, 200.0, 200.0);
        assert_eq!(width(&c, &[0]), 150.0);

        set_box_width(&mut c, &[0, 0], 21.0);
        lay(&mut c, 200.0, 200.0);
        assert_eq!(width(&c, &[0, 0]), 21.0);
        assert_eq!(width(&c, &[0]), 21.0);
    }

    // Child of the flow is Stretch: a fitting flow gives it no extra room.
    {
        let flow = left_to_right()
            .with_h_anchor(HAnchor::MIN_FIT_OR_STRETCH)
            .add(Box::new(
                gui_widget(20.0, 20.0).with_h_anchor(HAnchor::STRETCH),
            ))
            .add(Box::new(gui_widget(10.0, 10.0)));
        let mut c = container().add(Box::new(flow));
        for w in [300.0, 350.0, 310.0] {
            lay(&mut c, w, 200.0);
            assert_eq!(width(&c, &[0]), 30.0);
            assert_eq!(width(&c, &[0, 0]), 20.0);
        }
        set_box_width(&mut c, &[0, 1], 21.0);
        lay(&mut c, 310.0, 200.0);
        assert_eq!(width(&c, &[0]), 41.0);
        assert_eq!(width(&c, &[0, 0]), 20.0);
    }
}

/// C#'s expected rectangles, `(left, bottom, right, top)`.
type Edges = (f64, f64, f64, f64);

/// The six boxes of `LeftRightWithAnchorLeftRightChildTest`'s control.
fn left_right_control(padding: Insets, margin: Insets, button_size: f64) -> [Edges; 6] {
    let (w, h) = (button_size * 6.0, button_size * 3.0);
    let mut current_left = padding.left + margin.left;
    let button_height_with_margin = button_size + margin.vertical();
    let scaled_width =
        (w - padding.horizontal() - margin.horizontal() * 6.0 - button_size * 2.0) / 4.0;
    let bottom_anchor_y = padding.bottom + margin.bottom;
    // The left unsized rect (y: see the module header).
    let r0 = (
        current_left,
        bottom_anchor_y,
        current_left + button_size,
        bottom_anchor_y + button_size,
    );
    // A bottom anchor.
    current_left += button_size + margin.horizontal();
    let r1 = (
        current_left,
        bottom_anchor_y,
        current_left + scaled_width,
        bottom_anchor_y + button_size,
    );
    // Center anchor.
    let center_y = padding.bottom + (h - padding.vertical()) / 2.0;
    current_left += scaled_width + margin.horizontal();
    let r2 = (
        current_left,
        center_y - button_height_with_margin / 2.0 + margin.bottom,
        current_left + scaled_width,
        center_y + button_height_with_margin / 2.0 - margin.top,
    );
    // Top anchor.
    let top_anchor_y = h - padding.top - margin.top;
    current_left += scaled_width + margin.horizontal();
    let r3 = (
        current_left,
        top_anchor_y - button_size,
        current_left + scaled_width,
        top_anchor_y,
    );
    // Bottom top anchor.
    current_left += scaled_width + margin.horizontal();
    let r4 = (
        current_left,
        bottom_anchor_y,
        current_left + scaled_width,
        top_anchor_y,
    );
    // Right anchor.
    current_left += scaled_width + margin.horizontal();
    let r5 = (
        current_left,
        bottom_anchor_y,
        current_left + button_size,
        bottom_anchor_y + button_size,
    );
    [r0, r1, r2, r3, r4, r5]
}

fn assert_children_match(flow: &dyn Widget, expected: &[Edges; 6], what: &str) {
    for (i, (child, &(l, b, r, t))) in flow.children().iter().zip(expected).enumerate() {
        assert_eq!(
            child.bounds(),
            Rect::new(l, b, r - l, t - b),
            "child {i}, {what}"
        );
    }
}

/// C# `LeftRightWithAnchorLeftRightChildTest`.
fn left_right_with_anchor_left_right_child_test(control_padding: Insets, button_margin: Insets) {
    let button_size = 40.0;
    let expected = left_right_control(control_padding, button_margin, button_size);
    let middle = |v: VAnchor| {
        gui_widget(button_size / 2.0, button_size)
            .with_margin(button_margin)
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(v)
    };
    let flow = anchor_all_row(left_to_right())
        .with_inner_padding(control_padding)
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ))
        .add(Box::new(middle(VAnchor::BOTTOM)))
        .add(Box::new(middle(VAnchor::CENTER)))
        .add(Box::new(middle(VAnchor::TOP)))
        .add(Box::new(middle(VAnchor::STRETCH)))
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ));
    let mut test = container().add(Box::new(flow));
    lay(&mut test, button_size * 6.0, button_size * 3.0);
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_children_match(at(&test, &[0]), &expected, &what);
}

#[test]
fn left_right_with_anchor_left_right_child_tests() {
    for (padding, margin) in standard_pairs(uneven_pair()) {
        left_right_with_anchor_left_right_child_test(padding, margin);
    }
}

/// The six boxes of `TopBottomWithAnchorBottomTopChildTest`'s control.
fn top_bottom_control(padding: Insets, margin: Insets, button_size: f64) -> [Edges; 6] {
    let (w, h) = (button_size * 3.0, button_size * 6.0);
    let mut current_bottom = h - padding.top - margin.top - button_size;
    let button_width_with_margin = button_size + margin.horizontal();
    let scaled_height =
        (h - padding.vertical() - margin.vertical() * 6.0 - button_size * 2.0) / 4.0;
    let left_anchor_x = padding.left + margin.left;
    // The top unsized rect (x: see the module header).
    let r0 = (
        left_anchor_x,
        current_bottom,
        left_anchor_x + button_size,
        current_bottom + button_size,
    );
    // Left anchor.
    current_bottom -= scaled_height + margin.vertical();
    let r1 = (
        left_anchor_x,
        current_bottom,
        left_anchor_x + button_size,
        current_bottom + scaled_height,
    );
    // Center anchor.
    let center_x = padding.left + (w - padding.horizontal()) / 2.0;
    current_bottom -= scaled_height + margin.vertical();
    let r2 = (
        center_x - button_width_with_margin / 2.0 + margin.left,
        current_bottom,
        center_x + button_width_with_margin / 2.0 - margin.right,
        current_bottom + scaled_height,
    );
    // Right anchor.
    let right_anchor_x = w - padding.right - margin.right;
    current_bottom -= scaled_height + margin.vertical();
    let r3 = (
        right_anchor_x - button_size,
        current_bottom,
        right_anchor_x,
        current_bottom + scaled_height,
    );
    // Left right anchor.
    current_bottom -= scaled_height + margin.vertical();
    let r4 = (
        left_anchor_x,
        current_bottom,
        right_anchor_x,
        current_bottom + scaled_height,
    );
    // Bottom anchor.
    current_bottom -= button_size + margin.vertical();
    let r5 = (
        left_anchor_x,
        current_bottom,
        left_anchor_x + button_size,
        current_bottom + button_size,
    );
    [r0, r1, r2, r3, r4, r5]
}

/// C# `TopBottomWithAnchorBottomTopChildTest`.
fn top_bottom_with_anchor_bottom_top_child_test(control_padding: Insets, button_margin: Insets) {
    let button_size = 40.0;
    let expected = top_bottom_control(control_padding, button_margin, button_size);
    let middle = |h: HAnchor| {
        gui_widget(button_size, button_size / 2.0)
            .with_margin(button_margin)
            .with_v_anchor(VAnchor::STRETCH)
            .with_h_anchor(h)
    };
    let flow = anchor_all_column(top_to_bottom())
        .with_inner_padding(control_padding)
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ))
        .add(Box::new(middle(HAnchor::LEFT)))
        .add(Box::new(middle(HAnchor::CENTER)))
        .add(Box::new(middle(HAnchor::RIGHT)))
        .add(Box::new(middle(HAnchor::STRETCH)))
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ));
    let mut test = container().add(Box::new(flow));
    lay(&mut test, button_size * 3.0, button_size * 6.0);
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_children_match(at(&test, &[0]), &expected, &what);
}

#[test]
fn top_bottom_with_anchor_bottom_top_child_tests() {
    for (padding, margin) in standard_pairs(uneven_pair()) {
        top_bottom_with_anchor_bottom_top_child_test(padding, margin);
    }
}

/// The six boxes of `RightLeftWithAnchorLeftRightChildTest`'s control.
fn right_left_control(padding: Insets, margin: Insets, button_size: f64) -> [Edges; 6] {
    let (w, h) = (button_size * 6.0, button_size * 3.0);
    let mut current_left = w - padding.right - margin.right - button_size;
    let button_height_with_margin = button_size + margin.vertical();
    let scaled_width =
        (w - padding.horizontal() - margin.horizontal() * 6.0 - button_size * 2.0) / 4.0;
    let bottom_anchor_y = padding.bottom + margin.bottom;
    // The right unsized rect (y: see the module header).
    let r0 = (
        current_left,
        bottom_anchor_y,
        current_left + button_size,
        bottom_anchor_y + button_size,
    );
    // A bottom anchor.
    current_left -= scaled_width + margin.horizontal();
    let r1 = (
        current_left,
        bottom_anchor_y,
        current_left + scaled_width,
        bottom_anchor_y + button_size,
    );
    // Center anchor.
    let center_y = padding.bottom + (h - padding.vertical()) / 2.0;
    current_left -= scaled_width + margin.horizontal();
    let r2 = (
        current_left,
        center_y - button_height_with_margin / 2.0 + margin.bottom,
        current_left + scaled_width,
        center_y + button_height_with_margin / 2.0 - margin.top,
    );
    // Top anchor.
    let top_anchor_y = h - padding.top - margin.top;
    current_left -= scaled_width + margin.horizontal();
    let r3 = (
        current_left,
        top_anchor_y - button_size,
        current_left + scaled_width,
        top_anchor_y,
    );
    // Bottom top anchor.
    current_left -= scaled_width + margin.horizontal();
    let r4 = (
        current_left,
        bottom_anchor_y,
        current_left + scaled_width,
        top_anchor_y,
    );
    // The left unsized rect.
    current_left -= button_size + margin.horizontal();
    let r5 = (
        current_left,
        bottom_anchor_y,
        current_left + button_size,
        bottom_anchor_y + button_size,
    );
    [r0, r1, r2, r3, r4, r5]
}

/// C# `RightLeftWithAnchorLeftRightChildTest`: the left-to-right test's
/// children in a right-to-left flow, so the first added sits at the right.
fn right_left_with_anchor_left_right_child_test(control_padding: Insets, button_margin: Insets) {
    let button_size = 40.0;
    let expected = right_left_control(control_padding, button_margin, button_size);
    let middle = |v: VAnchor| {
        gui_widget(button_size / 2.0, button_size)
            .with_margin(button_margin)
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(v)
    };
    let flow = anchor_all_row(right_to_left())
        .with_inner_padding(control_padding)
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ))
        .add(Box::new(middle(VAnchor::BOTTOM)))
        .add(Box::new(middle(VAnchor::CENTER)))
        .add(Box::new(middle(VAnchor::TOP)))
        .add(Box::new(middle(VAnchor::STRETCH)))
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ));
    let mut test = container().add(Box::new(flow));
    lay(&mut test, button_size * 6.0, button_size * 3.0);
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_children_match(at(&test, &[0]), &expected, &what);
}

#[test]
fn right_left_with_anchor_left_right_child_tests() {
    for (padding, margin) in standard_pairs(uneven_pair()) {
        right_left_with_anchor_left_right_child_test(padding, margin);
    }
}

/// The six boxes of `BottomTopWithAnchorBottomTopChildTest`'s control.
fn bottom_top_control(padding: Insets, margin: Insets, button_size: f64) -> [Edges; 6] {
    let (w, h) = (button_size * 3.0, button_size * 6.0);
    let mut current_bottom = padding.bottom + margin.bottom;
    let button_width_with_margin = button_size + margin.horizontal();
    let scaled_height =
        (h - padding.vertical() - margin.vertical() * 6.0 - button_size * 2.0) / 4.0;
    let left_anchor_x = padding.left + margin.left;
    // The bottom unsized rect (x: see the module header).
    let r0 = (
        left_anchor_x,
        current_bottom,
        left_anchor_x + button_size,
        current_bottom + button_size,
    );
    // Left anchor.
    current_bottom += button_size + margin.vertical();
    let r1 = (
        left_anchor_x,
        current_bottom,
        left_anchor_x + button_size,
        current_bottom + scaled_height,
    );
    // Center anchor.
    let center_x = padding.left + (w - padding.horizontal()) / 2.0;
    current_bottom += scaled_height + margin.vertical();
    let r2 = (
        center_x - button_width_with_margin / 2.0 + margin.left,
        current_bottom,
        center_x + button_width_with_margin / 2.0 - margin.right,
        current_bottom + scaled_height,
    );
    // Right anchor.
    let right_anchor_x = w - padding.right - margin.right;
    current_bottom += scaled_height + margin.vertical();
    let r3 = (
        right_anchor_x - button_size,
        current_bottom,
        right_anchor_x,
        current_bottom + scaled_height,
    );
    // Left right anchor.
    current_bottom += scaled_height + margin.vertical();
    let r4 = (
        left_anchor_x,
        current_bottom,
        right_anchor_x,
        current_bottom + scaled_height,
    );
    // The top unsized rect.
    current_bottom += scaled_height + margin.vertical();
    let r5 = (
        left_anchor_x,
        current_bottom,
        left_anchor_x + button_size,
        current_bottom + button_size,
    );
    [r0, r1, r2, r3, r4, r5]
}

/// C# `BottomTopWithAnchorBottomTopChildTest`: the top-to-bottom test's
/// children in a bottom-to-top flow, so the first added sits at the bottom.
fn bottom_top_with_anchor_bottom_top_child_test(control_padding: Insets, button_margin: Insets) {
    let button_size = 40.0;
    let expected = bottom_top_control(control_padding, button_margin, button_size);
    let middle = |h: HAnchor| {
        gui_widget(button_size, button_size / 2.0)
            .with_margin(button_margin)
            .with_v_anchor(VAnchor::STRETCH)
            .with_h_anchor(h)
    };
    let flow = anchor_all_column(bottom_to_top())
        .with_inner_padding(control_padding)
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ))
        .add(Box::new(middle(HAnchor::LEFT)))
        .add(Box::new(middle(HAnchor::CENTER)))
        .add(Box::new(middle(HAnchor::RIGHT)))
        .add(Box::new(middle(HAnchor::STRETCH)))
        .add(Box::new(
            gui_widget(button_size, button_size).with_margin(button_margin),
        ));
    let mut test = container().add(Box::new(flow));
    lay(&mut test, button_size * 3.0, button_size * 6.0);
    let what = format!("padding {control_padding:?} margin {button_margin:?}");
    assert_children_match(at(&test, &[0]), &expected, &what);
}

#[test]
fn bottom_top_with_anchor_bottom_top_child_tests() {
    for (padding, margin) in standard_pairs(uneven_pair()) {
        bottom_top_with_anchor_bottom_top_child_test(padding, margin);
    }
}

/// A `[<->(10)(<->)(10)<->]` flow: `FIT | STRETCH`, built child by child
/// with C#'s checks on the parentless flow along the way.
fn fit_or_stretch_flow() -> FlexRow {
    let flow = left_to_right();
    assert_eq!(
        flow.h_anchor(),
        HAnchor::FIT,
        "flow layout starts with FitToChildren"
    );
    flow.with_h_anchor(HAnchor::FIT | HAnchor::STRETCH)
}

fn stretch_spacer() -> SizedBox {
    SizedBox::fixed(0.0, 10.0)
        .with_min_size(Size::new(0.0, 10.0))
        .with_h_anchor(HAnchor::STRETCH)
}

/// `(10)(spacer)(10)` in a fit-or-stretch flow, with C#'s parentless checks.
fn ten_spacer_ten() -> FlexRow {
    let mut flow = fit_or_stretch_flow();
    assert_eq!(lay_unparented(&mut flow).width, 0.0);
    flow.push(Box::new(gui_widget(10.0, 10.0)), 0.0);
    assert_eq!(lay_unparented(&mut flow).width, 10.0);
    flow.push(Box::new(stretch_spacer()), 0.0);
    assert_eq!(lay_unparented(&mut flow).width, 10.0);
    assert_eq!(width(&flow, &[1]), 0.0);
    flow.push(Box::new(gui_widget(10.0, 10.0)), 0.0);
    assert_eq!(lay_unparented(&mut flow).width, 20.0);
    flow
}

#[test]
fn child_h_anchor_priority() {
    // Make sure a middle spacer grows and shrinks correctly.
    {
        let flow = ten_spacer_ten();
        let mut c = container()
            .with_min_size(Size::new(40.0, 20.0))
            .add(Box::new(flow));
        for (w, spacer) in [(40.0, 20.0), (50.0, 30.0), (40.0, 20.0)] {
            lay(&mut c, w, 20.0);
            assert_eq!(c.bounds().width, w);
            assert_eq!(width(&c, &[0]), w);
            assert_eq!(width(&c, &[0, 1]), spacer);
        }
        // Minimum size is set to the construction size for normal GuiWidgets.
        assert_eq!(c.min_size().width, 40.0);
        c.widget_base_mut().expect("a base").min_size = Size::new(0.0, 0.0);
        lay(&mut c, 10.0, 20.0);
        assert_eq!(c.bounds().width, 10.0, "nothing should be keeping this big");
        assert_eq!(
            width(&c, &[0]),
            20.0,
            "it can't get smaller than its contents"
        );
        assert_eq!(width(&c, &[0, 1]), 0.0);
    }

    // Make sure the middle spacer works the same when in a flow layout.
    {
        let mut flow = fit_or_stretch_flow();
        assert_eq!(lay_unparented(&mut flow).width, 0.0);
        flow.push(Box::new(gui_widget(10.0, 10.0)), 0.0);
        assert_eq!(lay_unparented(&mut flow).width, 10.0);
        let wrapper = left_to_right()
            .with_h_anchor(HAnchor::FIT | HAnchor::STRETCH)
            .add(Box::new(stretch_spacer()));
        flow.push(Box::new(wrapper), 0.0);
        assert_eq!(lay_unparented(&mut flow).width, 10.0);
        assert_eq!(width(&flow, &[1]), 0.0);
        assert_eq!(width(&flow, &[1, 0]), 0.0);
        flow.push(Box::new(gui_widget(10.0, 10.0)), 0.0);
        assert_eq!(lay_unparented(&mut flow).width, 20.0);

        let mut c = container()
            .with_min_size(Size::new(40.0, 20.0))
            .add(Box::new(flow));
        lay(&mut c, 40.0, 20.0);
        assert_eq!(width(&c, &[0]), 40.0);
        assert_eq!(width(&c, &[0, 1]), 20.0);
        assert_eq!(width(&c, &[0, 1, 0]), 20.0);
        for (w, spacer) in [(50.0, 30.0), (40.0, 20.0)] {
            lay(&mut c, w, 20.0);
            assert_eq!(width(&c, &[0]), w);
            assert_eq!(width(&c, &[0, 1, 0]), spacer);
        }
    }

    // A middle spacer grows and shrinks correctly when the flow sits in a
    // plain widget (not a flow) that is itself LeftRight: first with the
    // spacer alone, then between two fixed widgets.
    for with_sides in [false, true] {
        let flow = if with_sides {
            ten_spacer_ten()
        } else {
            let mut flow = fit_or_stretch_flow();
            assert_eq!(lay_unparented(&mut flow).width, 0.0);
            flow.push(Box::new(stretch_spacer()), 0.0);
            assert_eq!(lay_unparented(&mut flow).width, 0.0);
            assert_eq!(width(&flow, &[0]), 0.0);
            flow
        };
        let spacer_path: &[usize] = if with_sides { &[0, 0, 1] } else { &[0, 0, 0] };
        let inner = || {
            AbsoluteLayout::new()
                .with_h_anchor(HAnchor::STRETCH | HAnchor::FIT)
                .with_v_anchor(VAnchor::ABSOLUTE)
                .with_min_size(Size::new(0.0, 20.0))
                .with_max_size(Size::new(f64::INFINITY, 20.0))
        };
        let mut outer = container()
            .with_min_size(Size::new(40.0, 20.0))
            .add(Box::new(inner()));
        lay(&mut outer, 40.0, 20.0);
        assert_eq!(width(&outer, &[0]), 40.0);

        let mut outer = container()
            .with_min_size(Size::new(40.0, 20.0))
            .add(Box::new(inner().add(Box::new(flow))));
        let side = if with_sides { 20.0 } else { 0.0 };
        for w in [40.0, 50.0, 40.0] {
            lay(&mut outer, w, 20.0);
            assert_eq!(width(&outer, &[0]), w);
            assert_eq!(width(&outer, &[0, 0]), w);
            assert_eq!(width(&outer, spacer_path), w - side);
        }
    }
}

const SEARCH_INPUT_H: f64 = 18.0;
const SEARCH_BUTTON_H: f64 = 24.0;

#[test]
fn test_v_anchor_center() {
    let mut search_panel = left_to_right()
        .with_fit_width(false)
        .with_h_anchor(HAnchor::STRETCH)
        .with_inner_padding(Insets::symmetric(3.0, 3.0));
    let search_input = SizedBox::fixed(60.0, SEARCH_INPUT_H)
        .with_margin(Insets::symmetric(6.0, 0.0))
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::CENTER);
    let centred = |panel: &mut FlexRow| {
        let p = lay_unparented(panel);
        let input = bounds(panel, &[0]);
        input.y == p.height - (input.y + input.height)
    };
    search_panel.push(Box::new(search_input), 0.0);
    assert!(centred(&mut search_panel));
    search_panel.push(
        Box::new(button(70.0, SEARCH_BUTTON_H).with_margin(Insets::from_sides(0.0, 9.0, 0.0, 0.0))),
        0.0,
    );
    assert!(centred(&mut search_panel));
}
