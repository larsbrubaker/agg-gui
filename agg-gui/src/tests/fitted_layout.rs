//! A fitted container (one that reports a smaller size than it was offered)
//! must end up with its children inside the box it actually occupies.
//!
//! A `FlexColumn` arranges its rows against the size it is laid out with: a
//! centred row is centred in the offered width and the rows start at the top
//! of the offered height.  When its parent then gives it its (smaller)
//! reported size, the parent must lay it out again at that size, or the rows
//! keep their offered-size positions and draw above/right of the column's
//! box — where its clip cuts them away.  These cover the two parents that
//! size a child differently from what they measured it with:
//! `AbsoluteLayout`, `Container`, and `FlexColumn`'s own cross axis.

use crate::widgets::AbsoluteLayout;
use crate::{Container, FlexColumn, HAnchor, Rect, Size, SizedBox, VAnchor, Widget};

fn sized(w: f64, h: f64) -> SizedBox {
    SizedBox::new().with_width(w).with_height(h)
}

/// A content-fitted column: a narrow centred row over a wide row, 40 tall.
fn fitted_column() -> FlexColumn {
    FlexColumn::new()
        .with_gap(0.0)
        .with_fit_width(true)
        .add(Box::new(sized(40.0, 20.0).with_h_anchor(HAnchor::CENTER)))
        .add(Box::new(sized(100.0, 20.0)))
}

/// Assert every child of `column` lies within the column's own box.
fn assert_children_inside(column: &dyn Widget) {
    let b = column.bounds();
    for (i, child) in column.children().iter().enumerate() {
        let c = child.bounds();
        assert!(
            c.x >= 0.0 && c.y >= 0.0 && c.x + c.width <= b.width && c.y + c.height <= b.height,
            "row {i} at {c:?} lies outside the column's {}x{} box",
            b.width,
            b.height
        );
    }
}

#[test]
fn a_fitted_column_in_an_absolute_layout_keeps_its_rows_inside() {
    let mut layout = AbsoluteLayout::new().add(Box::new(fitted_column().with_origin(10.0, 10.0)));
    layout.layout(Size::new(300.0, 400.0));
    let column = &layout.children()[0];
    assert_eq!(column.bounds(), Rect::new(10.0, 10.0, 100.0, 40.0));
    assert_children_inside(column.as_ref());
    // The centred row is centred in the column, the first row on top.
    assert_eq!(
        column.children()[0].bounds(),
        Rect::new(30.0, 20.0, 40.0, 20.0)
    );
    assert_eq!(
        column.children()[1].bounds(),
        Rect::new(0.0, 0.0, 100.0, 20.0)
    );
}

#[test]
fn a_centred_fitted_column_in_an_absolute_layout_keeps_its_rows_inside() {
    let mut layout = AbsoluteLayout::new().add(Box::new(
        fitted_column()
            .with_h_anchor(HAnchor::CENTER)
            .with_v_anchor(VAnchor::CENTER),
    ));
    layout.layout(Size::new(300.0, 400.0));
    let column = &layout.children()[0];
    assert_eq!(column.bounds(), Rect::new(100.0, 180.0, 100.0, 40.0));
    assert_children_inside(column.as_ref());
}

#[test]
fn a_fitted_column_in_a_stretching_column_keeps_its_rows_inside() {
    let mut outer = FlexColumn::new()
        .with_gap(0.0)
        .add(Box::new(fitted_column().with_h_anchor(HAnchor::CENTER)));
    outer.layout(Size::new(300.0, 400.0));
    let column = &outer.children()[0];
    assert_eq!(column.bounds(), Rect::new(100.0, 360.0, 100.0, 40.0));
    assert_children_inside(column.as_ref());
    assert_eq!(
        column.children()[0].bounds(),
        Rect::new(30.0, 20.0, 40.0, 20.0)
    );
}

#[test]
fn a_stretched_column_in_an_absolute_layout_places_rows_in_its_full_box() {
    // A stretched child gets the size it was measured with, so its rows
    // stay where its single layout pass put them.
    let mut layout = AbsoluteLayout::new().add(Box::new(
        fitted_column()
            .with_h_anchor(HAnchor::STRETCH)
            .with_v_anchor(VAnchor::STRETCH),
    ));
    layout.layout(Size::new(300.0, 400.0));
    let column = &layout.children()[0];
    assert_eq!(column.bounds(), Rect::new(0.0, 0.0, 300.0, 400.0));
    assert_children_inside(column.as_ref());
    // Rows start at the top of the stretched column; the centred row is
    // centred in its full width.
    assert_eq!(
        column.children()[0].bounds(),
        Rect::new(130.0, 380.0, 40.0, 20.0)
    );
}

#[test]
fn a_fitted_column_in_a_container_keeps_its_rows_inside() {
    let mut container = Container::new().add(Box::new(fitted_column()));
    container.layout(Size::new(300.0, 400.0));
    let column = &container.children()[0];
    // The column sits at the top of the container, at the size it reported.
    assert_eq!(column.bounds(), Rect::new(0.0, 360.0, 100.0, 40.0));
    assert_children_inside(column.as_ref());
    assert_eq!(
        column.children()[0].bounds(),
        Rect::new(30.0, 20.0, 40.0, 20.0)
    );
    assert_eq!(
        column.children()[1].bounds(),
        Rect::new(0.0, 0.0, 100.0, 20.0)
    );
}

#[test]
fn a_fit_height_container_keeps_a_fitted_column_inside() {
    let mut container = Container::new()
        .with_fit_height(true)
        .with_padding(5.0)
        .add(Box::new(fitted_column()));
    let size = container.layout(Size::new(300.0, 400.0));
    assert_eq!(size, Size::new(300.0, 50.0));
    let column = &container.children()[0];
    assert_eq!(column.bounds(), Rect::new(5.0, 5.0, 100.0, 40.0));
    assert_children_inside(column.as_ref());
}
