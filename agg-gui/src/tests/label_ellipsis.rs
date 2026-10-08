//! `Label::with_ellipsis_if_clipped` (agg-sharp `TextWidget.EllipsisIfClipped`)
//! and the row guarantee it backs: a long name in a `FlexRow` next to fixed
//! buttons takes only the space left over, never overlaps the buttons, and
//! paints a shortened "..." line that stays inside its own bounds. Paints go
//! through `PaintRecorder`, whose `measure_text` is seven pixels a character.

use std::sync::Arc;

use super::paint_recorder::PaintRecorder;
use crate::text::Font;
use crate::widgets::Label;
use crate::{FlexRow, Rect, Size, SizedBox, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

const LONG: &str = "Simple Proof of Bevel.mcx";

#[test]
fn ellipsis_label_paints_a_shortened_line_inside_its_bounds() {
    let mut label = Label::new(LONG, test_font()).with_ellipsis_if_clipped(true);
    label.set_bounds(Rect::new(0.0, 0.0, 70.0, 16.0));
    let mut rec = PaintRecorder::new();
    label.paint(&mut rec);
    assert_eq!(rec.texts.len(), 1);
    let (text, x, _) = &rec.texts[0];
    assert_eq!(text, "Simple...");
    assert!(x + text.chars().count() as f64 * 7.0 <= 70.0);
}

#[test]
fn label_without_ellipsis_paints_the_full_text() {
    let mut label = Label::new(LONG, test_font());
    label.set_bounds(Rect::new(0.0, 0.0, 70.0, 16.0));
    let mut rec = PaintRecorder::new();
    label.paint(&mut rec);
    assert_eq!(rec.texts[0].0, LONG);
}

#[test]
fn ellipsis_label_that_fits_paints_the_full_text() {
    let mut label = Label::new("Cube", test_font()).with_ellipsis_if_clipped(true);
    label.set_bounds(Rect::new(0.0, 0.0, 70.0, 16.0));
    let mut rec = PaintRecorder::new();
    label.paint(&mut rec);
    assert_eq!(rec.texts[0].0, "Cube");
}

#[test]
fn ellipsis_active_follows_the_laid_out_width() {
    let mut label = Label::new(LONG, test_font()).with_ellipsis_if_clipped(true);
    let size = label.layout(Size::new(60.0, 20.0));
    label.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    assert!(label.ellipsis_active());
    assert!(label.shown_text().ends_with("..."));
    let size = label.layout(Size::new(2000.0, 20.0));
    label.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    assert!(!label.ellipsis_active());
    assert_eq!(label.shown_text(), LONG);
}

#[test]
fn a_long_flex_name_never_overlaps_the_fixed_buttons_beside_it() {
    let mut row = FlexRow::new()
        .add_flex(
            Box::new(Label::new(LONG, test_font()).with_ellipsis_if_clipped(true)),
            1.0,
        )
        .add(Box::new(SizedBox::fixed(24.0, 24.0)))
        .add(Box::new(SizedBox::fixed(24.0, 24.0)));
    row.layout(Size::new(150.0, 24.0));
    let name = row.children()[0].bounds();
    let first = row.children()[1].bounds();
    let second = row.children()[2].bounds();
    assert!(
        name.x + name.width <= first.x,
        "{name:?} overlaps {first:?}"
    );
    assert!(first.x + first.width <= second.x);
    assert!(second.x + second.width <= 150.0);

    let mut rec = PaintRecorder::new();
    row.children_mut()[0].paint(&mut rec);
    let (text, x, _) = &rec.texts[0];
    assert!(text.ends_with("..."), "{text}");
    assert!(x + text.chars().count() as f64 * 7.0 <= name.width);
}
