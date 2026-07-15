//! Gap/margin handling for invisible flex children.
//!
//! `Conditional`'s contract (see `widgets/conditional.rs`) is that a hidden
//! child consumes no space: containers skip the gap and margin allotted to
//! slots whose child reports `is_visible() == false`. Without this, every
//! collapsed `Conditional` (or self-hiding widget like an empty results
//! list) leaves a phantom `gap` px of dead space in the layout — the cause
//! of the "search field not vertically centred in its panel" bug.

use std::cell::Cell;
use std::rc::Rc;

use crate::widgets::{Conditional, DEFAULT_COLUMN_GAP, DEFAULT_ROW_GAP};
use crate::{FlexColumn, FlexRow, Insets, Size, SizedBox, Widget};

/// The framework default is non-zero spacing (see [`DEFAULT_ROW_GAP`] /
/// [`DEFAULT_COLUMN_GAP`]): adjacent controls breathe unless a caller opts
/// into a fused/segmented look with `.with_gap(0.0)`. Pin the values and the
/// fact that `::new()` applies them, so the default can't silently revert.
#[test]
fn flex_new_applies_non_zero_default_gaps() {
    assert_eq!(DEFAULT_ROW_GAP, 8.0);
    assert_eq!(DEFAULT_COLUMN_GAP, 4.0);

    // FlexRow: two 20px children sit one DEFAULT_ROW_GAP apart.
    let mut row = FlexRow::new()
        .add(Box::new(SizedBox::fixed(20.0, 20.0)))
        .add(Box::new(SizedBox::fixed(20.0, 20.0)));
    row.layout(Size::new(300.0, 50.0));
    assert_eq!(row.children()[0].bounds().x, 0.0);
    assert_eq!(
        row.children()[1].bounds().x,
        20.0 + DEFAULT_ROW_GAP,
        "second row child offset by the default 8px gap"
    );

    // FlexColumn: Y-up, first child spans [280, 300], second starts one
    // DEFAULT_COLUMN_GAP below.
    let mut col = FlexColumn::new()
        .add(Box::new(SizedBox::fixed(20.0, 20.0)))
        .add(Box::new(SizedBox::fixed(20.0, 20.0)));
    col.layout(Size::new(200.0, 300.0));
    assert_eq!(col.children()[0].bounds().y, 280.0);
    assert_eq!(
        col.children()[1].bounds().y,
        280.0 - 20.0 - DEFAULT_COLUMN_GAP,
        "second column child offset by the default 4px gap"
    );
}

fn hidden_box() -> (Rc<Cell<bool>>, Box<dyn Widget>) {
    let visible = Rc::new(Cell::new(false));
    let child = Conditional::new(Rc::clone(&visible), Box::new(SizedBox::fixed(50.0, 20.0)))
        .with_margin(Insets::all(4.0));
    (visible, Box::new(child))
}

/// A hidden middle child must contribute no height, no margins, and no
/// gap — siblings sit exactly one `gap` apart, as if it weren't there.
#[test]
fn flex_column_skips_gap_and_margin_for_invisible_children() {
    let (visible, hidden) = hidden_box();
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .add(Box::new(SizedBox::fixed(50.0, 20.0)))
        .add(hidden)
        .add(Box::new(SizedBox::fixed(50.0, 20.0)));

    let size = col.layout(Size::new(200.0, 300.0));
    assert_eq!(
        size.height, 50.0,
        "20 + gap 10 + 20 — the hidden slot adds nothing"
    );
    // Y-up, cursor starts at the top (300): first child spans [280, 300],
    // one gap, second visible child spans [250, 270].
    assert_eq!(col.children()[0].bounds().y, 280.0);
    assert_eq!(
        col.children()[2].bounds().y,
        250.0,
        "only one gap between the two visible children"
    );

    // Toggling visible restores the full slot (height + margins + gaps).
    visible.set(true);
    let size = col.layout(Size::new(200.0, 300.0));
    assert_eq!(size.height, 20.0 + 10.0 + 4.0 + 20.0 + 4.0 + 10.0 + 20.0);
}

/// Same contract on the horizontal axis.
#[test]
fn flex_row_skips_gap_and_margin_for_invisible_children() {
    let (_visible, hidden) = hidden_box();
    let mut row = FlexRow::new()
        .with_gap(10.0)
        .add(Box::new(SizedBox::fixed(20.0, 20.0)))
        .add(hidden)
        .add(Box::new(SizedBox::fixed(20.0, 20.0)));

    row.layout(Size::new(300.0, 50.0));
    assert_eq!(row.children()[0].bounds().x, 0.0);
    assert_eq!(
        row.children()[2].bounds().x,
        30.0,
        "only one gap between the two visible children"
    );
}

/// A trailing hidden child must not leave a phantom gap below the last
/// visible child — the column's natural height ends at the visible content.
#[test]
fn flex_column_trailing_invisible_child_adds_no_height() {
    let (_visible, hidden) = hidden_box();
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .add(Box::new(SizedBox::fixed(50.0, 36.0)))
        .add(hidden);
    let size = col.layout(Size::new(200.0, 300.0));
    assert_eq!(
        size.height, 36.0,
        "a single visible child means no gaps at all"
    );
}
