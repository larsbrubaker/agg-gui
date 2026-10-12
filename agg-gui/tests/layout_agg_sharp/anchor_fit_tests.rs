//! Port of agg-sharp `AnchorTests.cs`, its fit-to-children tests (the
//! anchor-placement tests are in `anchor_tests.rs`; the class is split to
//! keep each file under the line limit): `SimpleFitToChildren`, the
//! `VAnchor`/`HAnchor` `Center | Fit` tests and
//! `GroupBoxResizeThenLayoutBeforeMatchChildren`.
//!
//! C# fits a widget by moving its `LocalBounds`' low edge to its children's
//! (margins and padding included), and its `Position` / `LocalBounds` are
//! measured from the parent's origin. agg-gui bounds start at the widget's
//! bottom-left, and a fitted container reports C#'s `LocalBounds` through
//! `local_bounds()`, so a child's C# rectangle is its bounds moved by its
//! parent's `local_bounds()` corner (`c_rect`). C#'s
//! `RectangleDouble(left, bottom, right, top)` is
//! `Rect::new(left, bottom, right - left, top - bottom)`.

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{GroupBox, HAnchor, Insets, Rect, Size, SizedBox, VAnchor, Widget};

use super::support::{font, layout_at, sized};

/// C# `new RectangleDouble(left, bottom, right, top)`.
fn rd(left: f64, bottom: f64, right: f64, top: f64) -> Rect {
    Rect::new(left, bottom, right - left, top - bottom)
}

/// The C# rectangle (relative to `parent`'s origin) of `parent`'s child
/// `index`.
fn c_rect(parent: &AbsoluteLayout, index: usize) -> Rect {
    let low = parent.local_bounds();
    let b = parent.children()[index].bounds();
    Rect::new(b.x + low.x, b.y + low.y, b.width, b.height)
}

fn sized_box(root: &mut dyn Widget, index: usize) -> &mut SizedBox {
    root.children_mut()[index]
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<SizedBox>())
        .expect("a SizedBox")
}

/// C# `child.LocalBounds = new RectangleDouble(-10, -11, 10, 11)` on a child
/// whose origin is 0: agg-gui bounds have no local origin, so the child moves
/// to (-10, -11) and asks for 20 x 22 (its size limits still apply).
fn offset_child(parent: &mut AbsoluteLayout) {
    let child = sized_box(parent, 0);
    child.width = Some(20.0);
    child.height = Some(22.0);
    child.widget_base_mut().expect("WidgetBase").origin = agg_gui::Point::new(-10.0, -11.0);
}

/// One `SimpleFitToChildren` block: `limits` are the child's minimum and
/// maximum size (C# `SizeLimitsToSet`), `expected` the C# rectangles after
/// each step: child after the offset, parent, parent with VAnchor.Fit,
/// child after `Width = 50`.
fn simple_fit_block(min: Size, max: Size, expected: [Rect; 4], width_50_before_v_fit: bool) {
    // C# new GuiWidget(10, 10): its construction size is its minimum.
    let mut parent = AbsoluteLayout::new()
        .with_h_anchor(HAnchor::FIT)
        .with_v_anchor(VAnchor::ABSOLUTE)
        .with_min_size(Size::new(10.0, 10.0));
    let own = Size::new(10.0, 10.0);
    layout_at(&mut parent, own);
    assert_eq!(parent.local_bounds(), rd(0.0, 0.0, 10.0, 10.0));

    parent = parent.add(Box::new(
        sized(30.0, 30.0).with_min_size(min).with_max_size(max),
    ));
    layout_at(&mut parent, own);
    assert_eq!(parent.local_bounds(), rd(0.0, 0.0, 30.0, 10.0));

    offset_child(&mut parent);
    layout_at(&mut parent, own);
    assert_eq!(c_rect(&parent, 0), expected[0]);
    if width_50_before_v_fit {
        sized_box(&mut parent, 0).width = Some(50.0); // we set the max so this won't work
        layout_at(&mut parent, own);
        assert_eq!(c_rect(&parent, 0), expected[3]);
    }
    assert_eq!(parent.local_bounds(), expected[1]);

    parent.widget_base_mut().expect("WidgetBase").v_anchor = VAnchor::FIT;
    layout_at(&mut parent, own);
    assert_eq!(parent.local_bounds(), expected[2]);
    if !width_50_before_v_fit {
        sized_box(&mut parent, 0).width = Some(50.0); // we set the max so this won't work
        layout_at(&mut parent, own);
        assert_eq!(c_rect(&parent, 0), expected[3]);
    }
}

#[test]
fn simple_fit_to_children() {
    // this is what will happen when the default of minimum size gets set on guiwidget construction
    simple_fit_block(
        Size::new(30.0, 30.0),
        Size::MAX,
        [
            rd(-10.0, -11.0, 20.0, 19.0),
            rd(-10.0, 0.0, 20.0, 10.0),
            rd(-10.0, -11.0, 20.0, 19.0),
            rd(-10.0, -11.0, 40.0, 19.0),
        ],
        false,
    );

    // this is how it should be resized when we set it change to get smaller than the initial size
    simple_fit_block(
        Size::ZERO,
        Size::MAX,
        [
            rd(-10.0, -11.0, 10.0, 11.0),
            rd(-10.0, 0.0, 10.0, 10.0),
            rd(-10.0, -11.0, 10.0, 11.0),
            rd(-10.0, -11.0, 40.0, 11.0),
        ],
        false,
    );

    // if we set min an max size it should no change size at all
    simple_fit_block(
        Size::new(30.0, 30.0),
        Size::new(30.0, 30.0),
        [
            rd(-10.0, -11.0, 20.0, 19.0),
            rd(-10.0, 0.0, 20.0, 10.0),
            rd(-10.0, -11.0, 20.0, 19.0),
            rd(-10.0, -11.0, 20.0, 19.0),
        ],
        true,
    );
}

fn center_fit_cases() -> [(Insets, Insets); 4] {
    [
        (Insets::ZERO, Insets::ZERO),
        (Insets::ZERO, Insets::all(3.0)),
        (Insets::all(2.0), Insets::all(0.0)),
        (Insets::all(2.0), Insets::all(3.0)),
        // C# leaves the uneven (1.1, 1.2, 1.3, 1.4) / (2.1, 2.2, 2.3, 2.4) case commented out.
    ]
}

/// childA (`Center | Fit` on one axis, padded, with a margin) holding
/// childB (50 x 50 with a margin, no size limits) in a padded 200 x 300
/// container. Returns the container.
fn center_fit_tree(padding: Insets, child_margin: Insets, vertical: bool) -> AbsoluteLayout {
    let mut child_a = AbsoluteLayout::new()
        .with_padding(padding)
        .with_margin(child_margin);
    child_a = if vertical {
        child_a.with_v_anchor(VAnchor::CENTER | VAnchor::FIT)
    } else {
        child_a.with_h_anchor(HAnchor::CENTER | HAnchor::FIT)
    };
    let child_b = sized(50.0, 50.0).with_margin(child_margin);
    child_a = child_a.add(Box::new(child_b));
    AbsoluteLayout::new()
        .with_padding(padding)
        .add(Box::new(child_a))
}

fn child_a(container: &AbsoluteLayout) -> &AbsoluteLayout {
    container.children()[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<AbsoluteLayout>())
        .expect("childA")
}

#[test]
fn v_anchor_center_and_v_anchor_fit_work_correctly_together_test() {
    for (p, m) in center_fit_cases() {
        v_anchor_center_and_v_anchor_fit_work_correctly_together(p, m);
    }
}

fn v_anchor_center_and_v_anchor_fit_work_correctly_together(padding: Insets, child_margin: Insets) {
    let size = Size::new(200.0, 300.0);
    let mut container = center_fit_tree(padding, child_margin, true);
    layout_at(&mut container, size);

    // assert sizes and positions
    let a = child_a(&container);
    assert_eq!(c_rect(a, 0).height, 50.0);
    let a_height = c_rect(&container, 0).height;
    assert_eq!(
        a_height,
        50.0 + child_margin.vertical() + padding.vertical()
    );
    assert_eq!(c_rect(&container, 0).y, (size.height - a_height) / 2.0);
    assert_eq!(c_rect(a, 0).y, 0.0);
    for height in [60.0, 40.0] {
        // expand B, then compact B
        sized_box(container.children_mut()[0].as_mut(), 0).height = Some(height);
        layout_at(&mut container, size);
        // assert sizes and positions
        let a = child_a(&container);
        assert_eq!(c_rect(a, 0).height, height);
        let a_height = c_rect(&container, 0).height;
        assert_eq!(
            a_height,
            height + child_margin.vertical() + padding.vertical()
        );
        assert_eq!(c_rect(&container, 0).y, (size.height - a_height) / 2.0);
    }
}

#[test]
fn h_anchor_center_and_h_anchor_fit_work_correctly_together_test() {
    for (p, m) in center_fit_cases() {
        h_anchor_center_and_h_anchor_fit_work_correctly_together(p, m);
    }
}

fn h_anchor_center_and_h_anchor_fit_work_correctly_together(padding: Insets, child_margin: Insets) {
    let size = Size::new(200.0, 300.0);
    let mut container = center_fit_tree(padding, child_margin, false);
    layout_at(&mut container, size);

    // assert sizes and positions
    let a = child_a(&container);
    assert_eq!(c_rect(a, 0).width, 50.0);
    let a_width = c_rect(&container, 0).width;
    assert_eq!(
        a_width,
        50.0 + child_margin.horizontal() + padding.horizontal()
    );
    assert_eq!(c_rect(&container, 0).x, (size.width - a_width) / 2.0);
    assert_eq!(c_rect(a, 0).x, 0.0);
    for width in [60.0, 40.0] {
        // expand B, then compact B
        sized_box(container.children_mut()[0].as_mut(), 0).width = Some(width);
        layout_at(&mut container, size);
        // assert sizes and positions
        let a = child_a(&container);
        assert_eq!(c_rect(a, 0).width, width);
        let a_width = c_rect(&container, 0).width;
        assert_eq!(
            a_width,
            width + child_margin.horizontal() + padding.horizontal()
        );
        assert_eq!(c_rect(&container, 0).x, (size.width - a_width) / 2.0);
    }
}

#[test]
fn group_box_resize_then_layout_before_match_children() {
    // agg-gui widgets enforce integer bounds by default (C# sets
    // DefaultEnforceIntegerBounds for this test).
    let contents = sized(30.0, 20.0).with_min_size(Size::new(0.0, 0.0));
    let mut group_box = GroupBox::new("group box", font());
    // make sure the client area will get smaller when the contents get smaller
    group_box
        .client_area_mut()
        .widget_base_mut()
        .expect("WidgetBase")
        .v_anchor = VAnchor::FIT;
    let mut group_box = group_box.add(Box::new(contents));

    // C#'s group box has no parent: nothing stretches it.
    layout_at(&mut group_box, Size::ZERO);
    let client = |g: &GroupBox| g.client_area().bounds();
    let contents_h = |g: &GroupBox| g.client_area().children()[0].bounds().height;
    let label_top = |g: &GroupBox| {
        let b = g.children()[0].bounds();
        b.y + b.height
    };
    assert_eq!(contents_h(&group_box), 20.0);
    assert_eq!(client(&group_box).height, 20.0);
    assert_eq!(group_box.bounds().height, 50.0);
    assert_eq!(label_top(&group_box), group_box.bounds().height);

    sized_box(group_box.client_area_mut(), 0).height = Some(10.0);
    layout_at(&mut group_box, Size::ZERO);
    assert_eq!(label_top(&group_box), group_box.bounds().height);
    assert_eq!(contents_h(&group_box), 10.0);
    assert_eq!(client(&group_box).height, 10.0);
    assert_eq!(group_box.bounds().height, 40.0);
}
