//! A flex layout measured with an unbounded cross axis anchors its children
//! within the extent it actually takes, not the extent it was offered.
//!
//! A column inside a scroll view lays each row out with a huge height
//! (`f64::MAX / 2`) to read its natural height. The row reports its content
//! height, so a `VAnchor::CENTER` child centred in the offered height landed
//! around y = 4.5e307, off screen, while its unanchored siblings sat at the
//! bottom (MatterCAD's editor list lost its row titles this way). The same
//! holds for a fit-width `FlexColumn` measured with an unbounded width and a
//! `HAnchor::CENTER` / `HAnchor::RIGHT` child. A finite offer keeps anchoring
//! within the offered extent, as before.

use std::sync::Arc;

use crate::geometry::Size;
use crate::layout_props::{HAnchor, VAnchor};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::{Button, FlexColumn, FlexRow, Label};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

/// A row holding a short centred label, a taller unanchored button and a
/// top-anchored label.
fn row() -> FlexRow {
    let font = font();
    FlexRow::new()
        .add(Box::new(
            Label::new("Title", Arc::clone(&font)).with_v_anchor(VAnchor::CENTER),
        ))
        .add(Box::new(Button::new("Edit", Arc::clone(&font))))
        .add(Box::new(
            Label::new("Top", font).with_v_anchor(VAnchor::TOP),
        ))
}

#[test]
fn centred_child_of_unbounded_row_is_centred_in_the_rows_own_height() {
    let mut row = row();
    let size = row.layout(Size::new(300.0, f64::MAX / 2.0));
    let label = row.children()[0].bounds();
    let button = row.children()[1].bounds();
    let top = row.children()[2].bounds();

    assert!(
        button.height > label.height,
        "the button ({}) must be taller than the label ({})",
        button.height,
        label.height
    );
    assert_eq!(
        size.height, button.height,
        "the row takes the button's height"
    );
    assert_eq!(button.y, 0.0, "the unanchored button sits at the bottom");
    let centred_y = ((size.height - label.height) * 0.5).round();
    assert_eq!(
        label.y, centred_y,
        "the centred label must sit in the middle of the row's {} px, not at {}",
        size.height, label.y
    );
    assert_eq!(
        top.y,
        size.height - top.height,
        "the top-anchored label must sit at the row's own top"
    );
}

#[test]
fn unbounded_row_places_children_as_a_row_offered_its_own_height() {
    let mut unbounded = row();
    let size = unbounded.layout(Size::new(300.0, f64::MAX / 2.0));
    let mut exact = row();
    exact.layout(Size::new(300.0, size.height));
    for i in 0..3 {
        assert_eq!(
            unbounded.children()[i].bounds(),
            exact.children()[i].bounds(),
            "child {i}"
        );
    }
}

#[test]
fn finite_row_still_centres_in_the_offered_height() {
    let mut row = row();
    row.layout(Size::new(300.0, 100.0));
    let label = row.children()[0].bounds();
    assert_eq!(label.y, ((100.0 - label.height) * 0.5).round());
    let top = row.children()[2].bounds();
    assert_eq!(top.y, 100.0 - top.height);
}

/// A fit-width column holding a short centred label, a wider button and a
/// right-anchored label.
fn column() -> FlexColumn {
    let font = font();
    FlexColumn::new()
        .with_fit_width(true)
        .add(Box::new(
            Label::new("A", Arc::clone(&font)).with_h_anchor(HAnchor::CENTER),
        ))
        .add(Box::new(Button::new(
            "A much wider button",
            Arc::clone(&font),
        )))
        .add(Box::new(
            Label::new("B", font).with_h_anchor(HAnchor::RIGHT),
        ))
}

#[test]
fn centred_child_of_unbounded_fit_width_column_is_centred_in_its_own_width() {
    let mut col = column();
    let size = col.layout(Size::new(f64::MAX / 2.0, 400.0));
    let label = col.children()[0].bounds();
    let button = col.children()[1].bounds();
    let right = col.children()[2].bounds();

    assert!(button.width > label.width);
    assert_eq!(
        size.width, button.width,
        "the column takes the button's width"
    );
    assert_eq!(button.x, 0.0);
    // Within a pixel: the column centres the label's unrounded width, then
    // snaps the result.
    let label_centre = label.x + label.width * 0.5;
    assert!(
        (label_centre - size.width * 0.5).abs() <= 1.0,
        "the centred label (x {}) must sit in the middle of the column's {} px",
        label.x,
        size.width
    );
    assert!(
        (right.x + right.width - size.width).abs() <= 1.0,
        "the right-anchored label (x {}) must end at the column's own right edge",
        right.x
    );
}

#[test]
fn unbounded_fit_width_column_places_children_as_one_offered_its_own_width() {
    let mut unbounded = column();
    let size = unbounded.layout(Size::new(f64::MAX / 2.0, 400.0));
    let mut exact = column();
    exact.layout(Size::new(size.width, 400.0));
    for i in 0..3 {
        assert_eq!(
            unbounded.children()[i].bounds(),
            exact.children()[i].bounds(),
            "child {i}"
        );
    }
}

#[test]
fn finite_fit_width_column_still_centres_in_the_offered_width() {
    let mut col = column();
    col.layout(Size::new(500.0, 400.0));
    let label = col.children()[0].bounds();
    assert_eq!(label.x, ((500.0 - label.width) * 0.5).round());
}
