//! A content-fitted `Container` (`with_fit_height(true)`) measured inside a
//! `ScrollView` must report its real content height.
//!
//! `ScrollView` measures its content with an effectively unbounded height
//! (`f64::MAX / 2`). `Container` stacks its children downward from the top of
//! that height, and at that magnitude subtracting a child's few-pixel height
//! from the cursor is absorbed by floating-point rounding — so a container
//! that derived its content height from `start - cursor` measured 0 and
//! collapsed to its padding. These pin the stacking containers' measured
//! height under the unbounded measure.

use std::sync::Arc;

use crate::text::Font;
use crate::widgets::Label;
use crate::{Color, Container, FlexColumn, ScrollView, Size, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// A rounded, padded, fit-height card holding three labels.
fn card(font: &Arc<Font>) -> Container {
    Container::new()
        .with_fit_height(true)
        .with_corner_radius(8.0)
        .with_padding(10.0)
        .with_background(Color::rgb(0.2, 0.2, 0.2))
        .add(Box::new(Label::new("Title", Arc::clone(font))))
        .add(Box::new(Label::new("Subtitle", Arc::clone(font))))
        .add(Box::new(Label::new("Detail", Arc::clone(font))))
}

/// The card's height when laid out in an ordinary finite slot.
fn finite_card_height(font: &Arc<Font>) -> f64 {
    card(font).layout(Size::new(300.0, 1000.0)).height
}

#[test]
fn fit_height_container_measures_its_content_under_an_unbounded_height() {
    let font = test_font();
    let expected = finite_card_height(&font);
    assert!(
        expected > 20.0,
        "card should be taller than its padding: {expected}"
    );
    let measured = card(&font).layout(Size::new(300.0, f64::MAX / 2.0));
    assert_eq!(measured.height, expected);
}

#[test]
fn fit_height_container_in_a_scroll_view_reports_its_content_height() {
    let font = test_font();
    let expected = finite_card_height(&font);
    let mut sv = ScrollView::new(Box::new(card(&font)));
    sv.layout(Size::new(300.0, 30.0));
    let content = &sv.children()[0];
    assert_eq!(content.bounds().height, expected);
    // Its labels sit inside the card, not at the measuring height.
    for (i, label) in content.children().iter().enumerate() {
        let b = label.bounds();
        assert!(
            b.y >= 0.0 && b.y + b.height <= expected,
            "label {i} at {b:?} lies outside the {expected}-tall card"
        );
    }
    assert!(
        sv.max_scroll_value() > 0.0,
        "a card taller than the view scrolls"
    );
}

#[test]
fn fit_height_cards_in_a_column_in_a_scroll_view_keep_their_height() {
    let font = test_font();
    let expected = finite_card_height(&font);
    let column = FlexColumn::new()
        .with_gap(4.0)
        .add(Box::new(card(&font)))
        .add(Box::new(card(&font)));
    let mut sv = ScrollView::new(Box::new(column));
    sv.layout(Size::new(300.0, 30.0));
    let column = &sv.children()[0];
    assert_eq!(column.bounds().height, expected * 2.0 + 4.0);
    for card in column.children() {
        assert_eq!(card.bounds().height, expected);
    }
}
