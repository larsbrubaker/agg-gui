//! `Label::with_ellipsis_mode`: a single-line label cut back in the middle
//! (`EllipsisMode::Middle`) or at path separators (`EllipsisMode::PathMiddle`)
//! instead of at the end.  Paints go through `PaintRecorder`, whose
//! `measure_text` is seven pixels a character unless built `with_real_fonts`.

use std::sync::Arc;

use super::paint_recorder::PaintRecorder;
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widgets::Label;
use crate::{EllipsisMode, Rect, Size, Widget};

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

const PATH: &str = "/Users/alex/projects/node_modules/esm";

fn painted(label: &mut Label, rec: &mut PaintRecorder) -> String {
    label.paint(rec);
    assert_eq!(rec.texts.len(), 1);
    rec.texts[0].0.clone()
}

#[test]
fn path_middle_label_paints_a_separator_cut_inside_its_bounds() {
    let mut label = Label::new(PATH, test_font()).with_ellipsis_mode(EllipsisMode::PathMiddle);
    label.set_bounds(Rect::new(0.0, 0.0, 210.0, 16.0));
    let text = painted(&mut label, &mut PaintRecorder::new());
    assert_eq!(text, "/Users/alex/\u{2026}/node_modules/esm");
}

#[test]
fn middle_label_keeps_head_and_tail() {
    let mut label = Label::new(PATH, test_font()).with_ellipsis_mode(EllipsisMode::Middle);
    label.set_bounds(Rect::new(0.0, 0.0, 70.0, 16.0));
    let text = painted(&mut label, &mut PaintRecorder::new());
    assert_eq!(text, "/User\u{2026}/esm");
}

#[test]
fn mode_label_that_fits_paints_the_full_text() {
    let mut label = Label::new(PATH, test_font()).with_ellipsis_mode(EllipsisMode::PathMiddle);
    label.set_bounds(Rect::new(0.0, 0.0, 400.0, 16.0));
    assert_eq!(painted(&mut label, &mut PaintRecorder::new()), PATH);
}

#[test]
fn middle_label_measures_with_its_real_font() {
    let font = test_font();
    let mut label =
        Label::new(PATH, Arc::clone(&font)).with_ellipsis_mode(EllipsisMode::PathMiddle);
    let size = label.layout(Size::new(150.0, 24.0));
    label.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let mut rec = PaintRecorder::with_real_fonts();
    let text = painted(&mut label, &mut rec);
    assert!(
        text.starts_with("/Users/") && text.ends_with("/esm"),
        "{text}"
    );
    assert!(text.contains('\u{2026}'), "{text}");
    let width = rec.measure_text(&text).expect("measured").width;
    assert!(
        width <= size.width,
        "{text:?} is {width} wide in {}",
        size.width
    );
    // `shown_text` (the tooltip / inspector view) agrees with the paint.
    assert!(label.ellipsis_active());
    assert_eq!(label.shown_text(), text);
}

#[test]
fn ellipsis_mode_round_trips_and_if_clipped_means_end() {
    let label = Label::new(PATH, test_font());
    assert_eq!(label.ellipsis_mode(), None);
    let label = label.with_ellipsis_mode(EllipsisMode::Middle);
    assert_eq!(label.ellipsis_mode(), Some(EllipsisMode::Middle));
    let label = label.with_ellipsis_if_clipped(false);
    assert_eq!(label.ellipsis_mode(), None);
    let mut label = label.with_ellipsis_if_clipped(true);
    assert_eq!(label.ellipsis_mode(), Some(EllipsisMode::End));
    label.set_ellipsis_mode(Some(EllipsisMode::PathMiddle));
    assert_eq!(label.ellipsis_mode(), Some(EllipsisMode::PathMiddle));
    // A middle-ellipsizing label gives way in a crowded row like an end one.
    assert!(label.shrink_min_width().is_some());
}
