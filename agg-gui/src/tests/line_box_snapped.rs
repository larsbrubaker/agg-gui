//! `LineBox::EmSnapped` is agg-sharp's `TextWidget` line box with
//! `GuiWidget.DefaultEnforceIntegerBounds = true` (what MatterCAD runs with):
//! the one-em box, its bottom `LineBoxBottomInPixels` below the baseline
//! (-0.2 em for Liberation Sans 1.07), is rounded outward to whole device
//! pixels (`Floor` bottom, `Ceiling` top) and the width is `Ceiling`'d. The
//! expected numbers are that rule worked by hand for 7, 8, 9, 10, 11, 12 and
//! 14 pt (px = pt * 96 / 72) at device scale 1 and 1.25: `(device height,
//! device baseline above the box's bottom)`. The one-em `LineBox::Em` and the
//! 1.5 em `Standard` are untouched. Paints go through `PaintRecorder` with the
//! real font.

use std::sync::Arc;

use super::paint_recorder::PaintRecorder;
use crate::font_settings::LineBox;
use crate::text::{Font, VerticalMetrics};
use crate::widgets::{Label, TextField};
use crate::{Rect, Size, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

/// (pt, device height, device baseline) at scale 1.
pub(super) const AT_1X: [(f64, f64, f64); 7] = [
    (7.0, 10.0, 2.0),
    (8.0, 12.0, 3.0),
    (9.0, 13.0, 3.0),
    (10.0, 14.0, 3.0),
    (11.0, 15.0, 3.0),
    (12.0, 17.0, 4.0),
    (14.0, 19.0, 4.0),
];

/// The same at scale 1.25.
pub(super) const AT_1_25X: [(f64, f64, f64); 7] = [
    (7.0, 13.0, 3.0),
    (8.0, 14.0, 3.0),
    (9.0, 16.0, 4.0),
    (10.0, 18.0, 4.0),
    (11.0, 19.0, 4.0),
    (12.0, 21.0, 5.0),
    (14.0, 24.0, 5.0),
];

/// A face with agg-sharp's Liberation Sans 1.07 vertical metrics (ascent 1638,
/// descent -410 of 2048), scaled to the test font's em.
pub(super) fn liberation_metrics() -> Arc<Font> {
    let font = Font::from_slice(FONT_BYTES).expect("font");
    let upem = font.units_per_em() as i32;
    Arc::new(font.with_vertical_metrics(VerticalMetrics {
        ascent: (1638 * upem / 2048) as i16,
        descent: (-410 * upem / 2048) as i16,
        line_gap: 0,
        cap_height: (1409 * upem / 2048) as i16,
    }))
}

fn px(pt: f64) -> f64 {
    pt * 96.0 / 72.0
}

fn label(font: &Arc<Font>, pt: f64, line_box: LineBox) -> Label {
    Label::new("Hello", Arc::clone(font))
        .with_font_size(px(pt))
        .with_line_box(line_box)
}

/// The label's laid-out size and the baseline it paints at.
pub(super) fn label_box(font: &Arc<Font>, pt: f64, line_box: LineBox) -> (Size, f64) {
    let mut l = label(font, pt, line_box);
    let size = l.layout(Size::new(1000.0, 1000.0));
    l.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let mut rec = PaintRecorder::with_real_fonts();
    l.paint(&mut rec);
    let (_, _, y) = rec.texts.remove(0);
    (size, y)
}

fn check_labels(scale: f64, table: &[(f64, f64, f64)]) {
    crate::device_scale::set_device_scale(scale);
    let font = liberation_metrics();
    for &(pt, dev_h, dev_base) in table {
        let (size, y) = label_box(&font, pt, LineBox::EmSnapped);
        assert!(
            (size.height - dev_h / scale).abs() < 1e-9,
            "{pt} pt at {scale}x: height {}, expected {}",
            size.height,
            dev_h / scale
        );
        assert!(
            (y - dev_base / scale).abs() < 1e-9,
            "{pt} pt at {scale}x: baseline {y}, expected {}",
            dev_base / scale
        );
        // The width is the text's, rounded up to whole device pixels.
        let dev_w = size.width * scale;
        assert!(
            (dev_w - dev_w.round()).abs() < 1e-9,
            "{pt} pt at {scale}x: width {} is not whole device pixels",
            size.width
        );
    }
    crate::device_scale::set_device_scale(1.0);
}

#[test]
fn a_label_is_the_snapped_line_box_at_scale_1() {
    check_labels(1.0, &AT_1X);
}

#[test]
fn a_label_is_the_snapped_line_box_at_scale_1_25() {
    check_labels(1.25, &AT_1_25X);
}

#[test]
fn rust_only_the_8_and_10_point_labels_are_12_and_14_not_11_and_13() {
    // The case this variant exists for: one em rounded to the nearest pixel
    // (what `Em` gives a flex layout) comes out 1 px short.
    crate::device_scale::set_device_scale(1.0);
    let font = liberation_metrics();
    assert_eq!(label_box(&font, 8.0, LineBox::EmSnapped).0.height, 12.0);
    assert_eq!(label_box(&font, 10.0, LineBox::EmSnapped).0.height, 14.0);
    assert!((label_box(&font, 8.0, LineBox::Em).0.height - px(8.0)).abs() < 1e-9);
    assert!((label_box(&font, 10.0, LineBox::Em).0.height - px(10.0)).abs() < 1e-9);
    assert_eq!(
        label_box(&font, 10.0, LineBox::Standard).0.height,
        px(10.0) * 1.5
    );
}

#[test]
fn rust_only_a_taller_snapped_label_centres_the_box() {
    crate::device_scale::set_device_scale(1.0);
    let font = liberation_metrics();
    let mut l = label(&font, 10.0, LineBox::EmSnapped);
    l.layout(Size::new(1000.0, 1000.0));
    l.set_bounds(Rect::new(0.0, 0.0, 100.0, 24.0));
    let mut rec = PaintRecorder::with_real_fonts();
    l.paint(&mut rec);
    let (_, _, y) = rec.texts.remove(0);
    // The 14 px box (baseline 3 above its bottom) sits 5 px up in 24.
    assert!((y - 8.0).abs() < 1e-9, "baseline {y}");
}

/// The field's height and the baseline it paints "Ag" at, with padding 3.
pub(super) fn field_box(font: &Arc<Font>, pt: f64, scale: f64) -> (f64, f64) {
    crate::device_scale::set_device_scale(scale);
    let mut f = TextField::new(Arc::clone(font))
        .with_font_size(px(pt))
        .with_line_box(LineBox::EmSnapped)
        .with_padding(3.0)
        .with_text("Ag");
    let size = f.layout(Size::new(400.0, 400.0));
    f.set_bounds(Rect::new(0.0, 0.0, 400.0, size.height));
    let mut rec = PaintRecorder::with_real_fonts();
    f.paint(&mut rec);
    let y = rec
        .texts
        .iter()
        .find(|(t, _, _)| t == "Ag")
        .map(|(_, _, y)| *y)
        .expect("the field drew its text");
    crate::device_scale::set_device_scale(1.0);
    (size.height, y)
}

#[test]
fn a_text_field_is_the_snapped_line_box_plus_its_insets() {
    let font = liberation_metrics();
    for (scale, table) in [(1.0, &AT_1X), (1.25, &AT_1_25X)] {
        for &(pt, dev_h, dev_base) in table {
            let (h, y) = field_box(&font, pt, scale);
            assert!(
                (h - (dev_h / scale + 6.0)).abs() < 1e-9,
                "{pt} pt at {scale}x: field height {h}, expected {}",
                dev_h / scale + 6.0
            );
            assert!(
                (y - (3.0 + dev_base / scale)).abs() < 1e-9,
                "{pt} pt at {scale}x: field baseline {y}, expected {}",
                3.0 + dev_base / scale
            );
        }
    }
}
