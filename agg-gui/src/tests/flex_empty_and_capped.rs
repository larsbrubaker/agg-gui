//! Flex layouts must not claim space they have nothing to put in (found by
//! the downstream HDTreeMap app):
//!
//! * An **empty** `FlexRow` / `FlexColumn` reports only its padding as its
//!   natural height, not the whole slot.  Before the fix an empty `FlexRow`
//!   built by a vertically centred `Rebuilder` took the full height and
//!   pushed a sheet's Cancel button off screen.
//! * A `Spacer` (or any flex child) capped by `max_size` takes no more than
//!   its cap: the space it can't use goes to the other flex children, and a
//!   column whose flex children are all capped reports its content height.

use std::sync::Arc;

use crate::geometry::{Rect, Size};
use crate::layout_props::{Insets, VAnchor};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::{FlexColumn, FlexRow, Label, Rebuilder, Spacer};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

#[test]
fn empty_flex_row_has_padding_only_height() {
    let mut row = FlexRow::new();
    let s = row.layout(Size::new(300.0, 500.0));
    assert_eq!(s.height, 0.0);
    assert_eq!(s.width, 300.0, "width keeps the legacy full-width report");

    let mut padded = FlexRow::new().with_inner_padding(Insets::all(3.0));
    assert_eq!(padded.layout(Size::new(300.0, 500.0)).height, 6.0);

    let mut fit = FlexRow::new()
        .with_fit_width(true)
        .with_inner_padding(Insets::all(3.0));
    assert_eq!(fit.layout(Size::new(300.0, 500.0)).width, 6.0);
}

#[test]
fn empty_flex_column_has_padding_only_height() {
    let mut col = FlexColumn::new();
    let s = col.layout(Size::new(300.0, 500.0));
    assert_eq!(s.height, 0.0);
    assert_eq!(s.width, 300.0);

    let mut padded = FlexColumn::new().with_inner_padding(Insets::all(3.0));
    assert_eq!(padded.layout(Size::new(300.0, 500.0)).height, 6.0);
}

#[test]
fn empty_row_in_centred_rebuilder_leaves_room_for_the_button() {
    let font = font();
    let rebuilt = Rebuilder::new(|| 1, || Box::new(FlexRow::new()) as Box<dyn Widget>)
        .with_v_anchor(VAnchor::CENTER);
    let mut col = FlexColumn::new()
        .with_top_anchor(true)
        .add(Box::new(rebuilt))
        .add(Box::new(Label::new("Cancel", font)));
    col.layout(Size::new(300.0, 200.0));
    col.set_bounds(Rect::new(0.0, 0.0, 300.0, 200.0));

    assert_eq!(col.children()[0].bounds().height, 0.0);
    let cancel = col.children()[1].bounds();
    assert!(
        cancel.y >= 0.0 && cancel.y + cancel.height <= 200.0,
        "Cancel pushed out of the column: {cancel:?}"
    );
}

#[test]
fn spacer_layout_respects_its_max_size() {
    let mut s = Spacer::new().with_max_size(Size::new(f64::MAX, 0.0));
    assert_eq!(s.layout(Size::new(300.0, 500.0)), Size::new(300.0, 0.0));
}

#[test]
fn zero_height_spacer_in_column_takes_no_space() {
    let font = font();
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .add(Box::new(Label::new("Top", Arc::clone(&font))))
        .add_flex(
            Box::new(Spacer::new().with_max_size(Size::new(f64::MAX, 0.0))),
            1.0,
        )
        .add(Box::new(Label::new("Bottom", font)));
    let s = col.layout(Size::new(300.0, 500.0));
    let kids = col.children();
    let labels_h = kids[0].bounds().height + kids[2].bounds().height;
    assert_eq!(kids[1].bounds().height, 0.0);
    assert!(
        (s.height - labels_h).abs() < 0.5,
        "column should hug its labels ({labels_h}), got {}",
        s.height
    );
}

#[test]
fn capped_flex_child_gives_its_share_to_the_others() {
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .add_flex(
            Box::new(Spacer::new().with_max_size(Size::new(f64::MAX, 0.0))),
            1.0,
        )
        .add_flex(
            Box::new(Spacer::new().with_max_size(Size::new(f64::MAX, 50.0))),
            1.0,
        )
        .add_flex(Box::new(Spacer::new()), 1.0);
    let s = col.layout(Size::new(300.0, 400.0));
    let h: Vec<f64> = col.children().iter().map(|c| c.bounds().height).collect();
    assert_eq!(h, vec![0.0, 50.0, 350.0]);
    assert_eq!(s.height, 400.0);

    let mut row = FlexRow::new()
        .with_gap(0.0)
        .add_flex(
            Box::new(Spacer::new().with_max_size(Size::new(0.0, f64::MAX))),
            1.0,
        )
        .add_flex(Box::new(Spacer::new()), 1.0);
    row.layout(Size::new(300.0, 20.0));
    let w: Vec<f64> = row.children().iter().map(|c| c.bounds().width).collect();
    assert_eq!(w, vec![0.0, 300.0]);
}
