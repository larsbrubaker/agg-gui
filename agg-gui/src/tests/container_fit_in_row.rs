//! An empty fit-height `Container` inside a `FlexRow` (a colour swatch, a
//! legend chip) must take only its content width — its padding and
//! `min_size` — not the whole row.  Before the fix it reported the full
//! available width as its natural width, and the `FlexRow` pushed the
//! row's other labels out of view (found by HDTreeMap's legend rows).

use std::sync::Arc;

use crate::geometry::{Rect, Size};
use crate::layout_props::Insets;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::{Container, FlexRow, Label};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(super::TEST_FONT).expect("font"))
}

#[test]
fn empty_fit_height_container_does_not_push_row_labels_out() {
    let font = font();
    let swatch = Container::new()
        .with_fit_height(true)
        .with_inner_padding(Insets::all(2.0))
        .with_min_size(Size::new(12.0, 12.0));
    let mut row = FlexRow::new()
        .add(Box::new(swatch))
        .add(Box::new(Label::new("Folders", Arc::clone(&font))))
        .add(Box::new(Label::new("12.3 GB", font)));
    row.layout(Size::new(300.0, 20.0));
    row.set_bounds(Rect::new(0.0, 0.0, 300.0, 20.0));

    let kids = row.children();
    let swatch_w = kids[0].bounds().width;
    assert!(
        swatch_w <= 12.5,
        "empty fit-height container should be its min width, got {swatch_w}"
    );
    for (i, label) in kids.iter().enumerate().skip(1) {
        let b = label.bounds();
        assert!(
            b.width > 10.0 && b.x + b.width <= 300.0 + 0.5,
            "label {i} pushed out of the row: {b:?}"
        );
    }
}

#[test]
fn fit_height_container_with_content_still_fills_the_width() {
    // Backward compatibility: a fit-height container holding content keeps
    // filling the width it is given (status bars, cards).
    let mut c = Container::new()
        .with_fit_height(true)
        .add(Box::new(Label::new("Status", font())));
    let size = c.layout(Size::new(300.0, 100.0));
    assert_eq!(size.width, 300.0);
    assert!(size.height < 100.0);
}
