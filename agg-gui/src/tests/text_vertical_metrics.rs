//! `Font::with_vertical_metrics` moves every baseline a widget computes: a
//! `Label` (plain and ellipsized) on the one-em line box puts its baseline
//! where agg-sharp's `TypeFacePrinter` would for the overriding metrics, and a
//! `TextField` moves by the same amount. A font with no override places text
//! exactly as the face's own `hhea` metrics say. Paints go through
//! `PaintRecorder` measuring with the real font.

use std::sync::Arc;

use super::paint_recorder::PaintRecorder;
use crate::font_settings::LineBox;
use crate::text::{Font, VerticalMetrics};
use crate::widgets::{Label, TextField};
use crate::{Rect, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const SIZE: f64 = 48.0;

/// agg-sharp's Liberation Sans 1.07 SVG face (`LiberationSansFont.cs`).
const OVERRIDE: VerticalMetrics = VerticalMetrics {
    ascent: 1638,
    descent: -410,
    line_gap: 0,
    cap_height: 1409,
};

fn plain() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn overridden() -> Arc<Font> {
    Arc::new(
        Font::from_slice(FONT_BYTES)
            .expect("font")
            .with_vertical_metrics(OVERRIDE),
    )
}

/// The baseline agg-sharp's `TextWidget` puts its text on, measured up from
/// the bottom of its one-em box: `-LineBoxBottomInPixels`, where
/// `LineBoxBottomInPixels = (Ascent + Descent - EmSize) / 2` (descent
/// negative) for a face whose span exceeds zero (`TypeFacePrinter.cs`).
fn agg_sharp_baseline(ascent: i16, descent: i16, units_per_em: u16, em: f64) -> f64 {
    let scale = em / units_per_em as f64;
    let line_box_bottom =
        (ascent as f64 * scale + descent as f64 * scale - units_per_em as f64 * scale) / 2.0;
    -line_box_bottom
}

fn label_baseline(font: Arc<Font>, text: &str, width: f64, ellipsis: bool) -> (String, f64) {
    let mut label = Label::new(text, font)
        .with_font_size(SIZE)
        .with_line_box(LineBox::Em)
        .with_ellipsis_if_clipped(ellipsis);
    label.set_bounds(Rect::new(0.0, 0.0, width, SIZE));
    let mut rec = PaintRecorder::with_real_fonts();
    label.paint(&mut rec);
    assert_eq!(rec.texts.len(), 1, "one line drawn");
    let (shown, _, y) = rec.texts.remove(0);
    (shown, y)
}

fn text_field_baseline(font: Arc<Font>) -> f64 {
    let mut field = TextField::new(font).with_font_size(SIZE).with_text("Ag");
    field.set_bounds(Rect::new(0.0, 0.0, 400.0, SIZE + 8.0));
    let mut rec = PaintRecorder::with_real_fonts();
    field.paint(&mut rec);
    rec.texts
        .iter()
        .find(|(t, _, _)| t == "Ag")
        .map(|(_, _, y)| *y)
        .expect("the field drew its text")
}

#[test]
fn an_override_puts_a_labels_baseline_where_agg_sharp_does() {
    let font = overridden();
    let expected = agg_sharp_baseline(1638, -410, font.units_per_em(), SIZE);
    // 410 / 2048 of the em above the box's bottom for Liberation Sans 1.07.
    if font.units_per_em() == 2048 {
        assert!((expected - SIZE * 410.0 / 2048.0).abs() < 1e-9);
    }
    let (shown, y) = label_baseline(font.clone(), "New Design", 1000.0, false);
    assert_eq!(shown, "New Design");
    assert!(
        (y - expected).abs() < 1e-9,
        "baseline {y}, expected {expected}"
    );

    // The ellipsized path centres the shortened line the same way.
    let (shown, y) = label_baseline(font, "Simple Proof of Bevel.mcx", 150.0, true);
    assert!(shown.ends_with("..."), "{shown:?} is ellipsized");
    assert!(
        (y - expected).abs() < 1e-9,
        "ellipsized baseline {y}, expected {expected}"
    );
}

#[test]
fn without_an_override_a_labels_baseline_follows_the_face() {
    let face = ttf_parser::Face::parse(FONT_BYTES, 0).unwrap();
    let expected = agg_sharp_baseline(face.ascender(), face.descender(), face.units_per_em(), SIZE);
    let (_, y) = label_baseline(plain(), "New Design", 1000.0, false);
    assert!(
        (y - expected).abs() < 1e-9,
        "baseline {y}, expected {expected}"
    );
    // The override really moves it (the case is only meaningful if it does).
    let (_, moved) = label_baseline(overridden(), "New Design", 1000.0, false);
    assert!(
        (moved - y).abs() > 0.5,
        "override baseline {moved} vs face {y}"
    );
}

#[test]
fn an_override_moves_a_text_fields_baseline_by_the_same_amount() {
    let face = ttf_parser::Face::parse(FONT_BYTES, 0).unwrap();
    let upem = face.units_per_em();
    let shift = agg_sharp_baseline(1638, -410, upem, SIZE)
        - agg_sharp_baseline(face.ascender(), face.descender(), upem, SIZE);
    let moved = text_field_baseline(overridden()) - text_field_baseline(plain());
    assert!(
        (moved - shift).abs() < 1e-9,
        "moved {moved}, expected {shift}"
    );
}
