//! Unit tests for [`DragValue`](super::DragValue): value formatting / suffix
//! handling, the intrinsic minimum width that keeps numbers from clipping, and
//! the shared value-cell binding used by the Widget Gallery. Split out of
//! `drag_value.rs` to keep that file under the project's 800-line cap.

use super::*;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn test_font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

#[test]
fn display_text_appends_suffix() {
    let dv = DragValue::new(30.0, 0.0, 360.0, test_font())
        .with_decimals(0)
        .with_suffix("°");
    assert_eq!(dv.display_text(), "30°");
    // Edit buffer stays numeric so parsing works.
    assert_eq!(dv.format_value(), "30");
}

#[test]
fn suffix_with_space_reads_like_years() {
    let dv = DragValue::new(2.0, 0.0, 99.0, test_font())
        .with_decimals(0)
        .with_suffix(" years");
    assert_eq!(dv.display_text(), "2 years");
}

#[test]
fn edit_mode_buffer_excludes_suffix() {
    let mut dv = DragValue::new(5.0, 0.0, 10.0, test_font())
        .with_decimals(0)
        .with_suffix(" m");
    dv.enter_edit_mode();
    assert_eq!(dv.edit_text, "5", "suffix must not enter the edit buffer");
}

#[test]
fn no_suffix_matches_plain_value() {
    let dv = DragValue::new(1.5, 0.0, 10.0, test_font()).with_decimals(2);
    assert_eq!(dv.display_text(), "1.50");
}

/// A DragValue showing `1.00` reports a min width wide enough for the value
/// plus the arrow zones, and the label's available area is not clipped when
/// rendered at exactly that width.
#[test]
fn intrinsic_min_width_fits_value_and_arrows() {
    let font = test_font();
    let dv = DragValue::new(1.0, 0.0, 10.0, Arc::clone(&font)).with_decimals(2);
    assert_eq!(dv.display_text(), "1.00");
    let text_w = measure_advance(&font, "1.00", dv.font_size);
    let min_w = dv.min_size().width;
    assert!(
        min_w >= text_w + LABEL_SIDE_INSET * 2.0,
        "min width {min_w} must cover text {text_w} plus both arrow zones"
    );
    // At the min width, the label's available inner area still fits "1.00".
    let avail_w = min_w - LABEL_SIDE_INSET * 2.0;
    assert!(
        avail_w >= text_w,
        "label clipped at min width: inner {avail_w} < text {text_w}"
    );
}

/// An explicit, larger `min_size` set by the host wins over the intrinsic.
#[test]
fn explicit_min_width_wins_when_larger() {
    let dv = DragValue::new(1.0, 0.0, 10.0, test_font())
        .with_decimals(2)
        .with_min_size(Size::new(500.0, 0.0));
    assert_eq!(dv.min_size().width, 500.0);
}

/// `layout` never returns a width narrower than the intrinsic, even when the
/// host offers far less.
#[test]
fn layout_never_narrower_than_intrinsic() {
    let mut dv = DragValue::new(1.0, 0.0, 10.0, test_font()).with_decimals(2);
    let intrinsic = dv.intrinsic_min_width();
    let sz = dv.layout(Size::new(4.0, 24.0));
    assert!(
        sz.width >= intrinsic,
        "layout width {} must not fall below intrinsic {intrinsic}",
        sz.width
    );
}

/// Regression (Widget Gallery): a DragValue bound to a shared cell must
/// re-read that cell every `layout()` so a *sibling* widget writing the
/// same cell (e.g. the gallery Slider) drives this DragValue's displayed
/// value live.  Before the fix the DragValue captured its value at build
/// time and only ever wrote via `on_change`, so it read stale (the reported
/// "slider at 140, DragValue reads 205").
#[test]
fn value_cell_tracks_external_writes_after_layout() {
    use std::cell::Cell;
    use std::rc::Rc;

    let cell = Rc::new(Cell::new(42.0_f64));
    let mut dv = DragValue::new(cell.get(), 0.0, 360.0, test_font())
        .with_decimals(0)
        .with_value_cell(Rc::clone(&cell));

    assert_eq!(dv.value_label.text_str(), "42");

    // A sibling (the Slider) writes a new value into the shared cell.
    cell.set(140.0);
    // The next layout pass must pick it up and refresh the label text.
    let _ = dv.layout(Size::new(120.0, 24.0));

    assert_eq!(dv.value(), 140.0, "DragValue value must follow the cell");
    assert_eq!(
        dv.value_label.text_str(),
        "140",
        "DragValue label text must repaint to the cell's value"
    );
}

/// The value cell is bidirectional: a drag on the DragValue writes back to
/// the cell so the Slider (and progress bar) follow it too.
#[test]
fn drag_writes_back_to_value_cell() {
    use std::cell::Cell;
    use std::rc::Rc;

    let cell = Rc::new(Cell::new(10.0_f64));
    let mut dv = DragValue::new(cell.get(), 0.0, 360.0, test_font())
        .with_decimals(0)
        .with_value_cell(Rc::clone(&cell));

    // Simulate a confirmed drag that moves the value.
    dv.drag_start_x = 0.0;
    dv.drag_start_value = 10.0;
    dv.dragging = true;
    dv.update_from_drag(5.0); // speed 1.0 → +5 units

    assert_eq!(dv.value(), 15.0);
    assert_eq!(cell.get(), 15.0, "drag must write back to the shared cell");
}
