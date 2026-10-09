//! Unit tests for [`DragValue`](super::DragValue): value formatting / suffix
//! handling, the intrinsic minimum width that keeps numbers from clipping,
//! the shared value-cell binding used by the Widget Gallery, and how an inline
//! edit commits (external changes survive it; `on_change` only on a real
//! change). Split out of
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

/// Cursor convention (egui): horizontal-resize arrow over a DragValue
/// and throughout a scrub drag that leaves it; I-beam while editing
/// inline; the arrow once the pointer is off it.
#[test]
fn drag_value_cursor_hover_drag_and_edit() {
    use crate::cursor::{current_cursor_icon, reset_cursor_icon, CursorIcon};
    use crate::event::{Modifiers, MouseButton};
    use crate::geometry::Point;

    fn moved(dv: &mut DragValue, x: f64, y: f64) -> CursorIcon {
        reset_cursor_icon();
        dv.on_event(&Event::MouseMove {
            pos: Point::new(x, y),
        });
        current_cursor_icon()
    }

    let mut dv = DragValue::new(5.0, 0.0, 100.0, test_font());
    let size = dv.layout(Size::new(120.0, 30.0));
    dv.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let (cx, cy) = (size.width * 0.5, size.height * 0.5);

    assert_eq!(moved(&mut dv, cx, cy), CursorIcon::ResizeHorizontal);
    assert_eq!(moved(&mut dv, -1.0, -1.0), CursorIcon::Default);

    dv.on_event(&Event::MouseDown {
        pos: Point::new(cx, cy),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        moved(&mut dv, cx + 400.0, cy),
        CursorIcon::ResizeHorizontal,
        "scrub drag past the widget keeps the resize arrow"
    );
    dv.on_event(&Event::MouseUp {
        pos: Point::new(cx + 400.0, cy),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(moved(&mut dv, cx + 400.0, cy), CursorIcon::Default);

    dv.enter_edit_mode();
    assert_eq!(moved(&mut dv, cx, cy), CursorIcon::Text, "editing inline");
}

/// Type `text` into a DragValue that is in inline edit mode, one key at a time.
fn type_keys(dv: &mut DragValue, text: &str) {
    use crate::event::Modifiers;
    for c in text.chars() {
        dv.on_event(&Event::KeyDown {
            key: Key::Char(c),
            modifiers: Modifiers::default(),
        });
    }
}

/// Press Enter in a DragValue that is in inline edit mode.
fn press_enter(dv: &mut DragValue) {
    dv.on_event(&Event::KeyDown {
        key: Key::Enter,
        modifiers: crate::event::Modifiers::default(),
    });
}

/// egui #8403: while a DragValue is being edited, something else (a sibling
/// widget, app code) changes the bound value. The next frame discards the
/// now-stale edit text and shows the new value, and losing focus must not
/// write the old text back over it.
#[test]
fn edit_does_not_revert_external_change_after_layout() {
    use std::cell::Cell;
    use std::rc::Rc;

    let cell = Rc::new(Cell::new(10.0_f64));
    let mut dv = DragValue::new(cell.get(), 0.0, 360.0, test_font())
        .with_decimals(0)
        .with_value_cell(Rc::clone(&cell));

    dv.enter_edit_mode();
    type_keys(&mut dv, "5");
    assert_eq!(dv.edit_text, "105");

    // Something else changes the value while the field is being edited.
    cell.set(42.0);
    let _ = dv.layout(Size::new(120.0, 24.0));
    assert_eq!(dv.edit_text, "42", "stale edit text must be refreshed");

    // Losing focus commits — and must keep the external value.
    dv.on_event(&Event::FocusLost);
    assert_eq!(cell.get(), 42.0, "external change must survive the commit");
    assert_eq!(dv.value(), 42.0);
}

/// egui #8403 guard: with no external change, layout passes mid-edit must
/// keep the half-typed text (`"7."` would format back as `"7"`).
#[test]
fn layout_while_editing_keeps_typed_text() {
    use std::cell::Cell;
    use std::rc::Rc;

    let cell = Rc::new(Cell::new(7.0_f64));
    let mut dv = DragValue::new(cell.get(), 0.0, 100.0, test_font())
        .with_decimals(0)
        .with_value_cell(Rc::clone(&cell));

    dv.enter_edit_mode();
    type_keys(&mut dv, ".");
    let _ = dv.layout(Size::new(120.0, 24.0));
    type_keys(&mut dv, "5");
    let _ = dv.layout(Size::new(120.0, 24.0));
    assert_eq!(dv.edit_text, "7.5");

    press_enter(&mut dv);
    assert_eq!(cell.get(), 7.5);
}

/// egui #8403, commit-time check: the external change lands after the last
/// layout pass but before the commit (same frame). The edit text belongs to
/// the old value, so committing it must not overwrite the new one.
#[test]
fn commit_does_not_revert_external_change_made_since_layout() {
    use std::cell::Cell;
    use std::rc::Rc;

    let cell = Rc::new(Cell::new(10.0_f64));
    let mut dv = DragValue::new(cell.get(), 0.0, 360.0, test_font())
        .with_decimals(0)
        .with_value_cell(Rc::clone(&cell));

    dv.enter_edit_mode();
    type_keys(&mut dv, "5");
    cell.set(42.0);
    press_enter(&mut dv);

    assert_eq!(cell.get(), 42.0, "external change must survive the commit");
    assert_eq!(dv.value(), 42.0);
    assert_eq!(dv.value_label.text_str(), "42");
}

/// `on_change` means "the value changed" (agg-sharp `DragValueTests.
/// TypedTextIsParsedClampedAndCommitted` and `ConstructionClampsAnd
/// ProgrammaticSetsRaiseOnlyOnChange`; egui #8627). Committing an edit whose
/// text parses back to the current value, or doesn't parse, must not report
/// a change; committing a different value reports exactly one.
#[test]
fn commit_with_unchanged_text_does_not_fire_on_change() {
    use std::cell::Cell;
    use std::rc::Rc;

    let changes = Rc::new(Cell::new(0_u32));
    let changes_cb = Rc::clone(&changes);
    let mut dv = DragValue::new(7.0, 0.0, 100.0, test_font())
        .with_decimals(0)
        .on_change(move |_| changes_cb.set(changes_cb.get() + 1));

    dv.enter_edit_mode();
    press_enter(&mut dv);
    assert_eq!(
        changes.get(),
        0,
        "Enter with unchanged text is not a change"
    );

    dv.enter_edit_mode();
    dv.on_event(&Event::FocusLost);
    assert_eq!(changes.get(), 0, "blur with unchanged text is not a change");

    dv.enter_edit_mode();
    type_keys(&mut dv, "1");
    press_enter(&mut dv);
    assert_eq!(dv.value(), 71.0);
    assert_eq!(changes.get(), 1, "a real change is reported once");

    // Unparsable text (here, everything deleted) leaves the value alone.
    dv.enter_edit_mode();
    for _ in 0..2 {
        dv.on_event(&Event::KeyDown {
            key: Key::Backspace,
            modifiers: crate::event::Modifiers::default(),
        });
    }
    assert_eq!(dv.edit_text, "");
    press_enter(&mut dv);
    assert_eq!(dv.value(), 71.0);
    assert_eq!(changes.get(), 1, "unparsable text is not a change");
}

/// A drag step that step-snapping rounds back to the current value is not a
/// change, so `on_change` stays quiet until the snapped value actually moves
/// (agg-sharp's drag sets `DragValue.Value`, which raises only on a change:
/// `DragMovesTheValueBySpeedPerDesignUnit`; egui #8627).
#[test]
fn drag_within_one_snap_step_does_not_fire_on_change() {
    use crate::event::Modifiers;
    use crate::geometry::Point;
    use std::cell::RefCell;
    use std::rc::Rc;

    let changes = Rc::new(RefCell::new(Vec::new()));
    let changes_cb = Rc::clone(&changes);
    let mut dv = DragValue::new(5.0, 0.0, 100.0, test_font())
        .with_decimals(0)
        .with_step(1.0)
        .with_speed(0.1)
        .on_change(move |v| changes_cb.borrow_mut().push(v));
    let size = dv.layout(Size::new(120.0, 30.0));
    dv.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    let (cx, cy) = (size.width * 0.5, size.height * 0.5);

    dv.on_event(&Event::MouseDown {
        pos: Point::new(cx, cy),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    // Past the drag threshold, but 4 px * 0.1 = 0.4 snaps back to 5.
    dv.on_event(&Event::MouseMove {
        pos: Point::new(cx + 4.0, cy),
    });
    assert!(dv.dragging, "the drag is under way");
    assert_eq!(dv.value(), 5.0);
    assert!(
        changes.borrow().is_empty(),
        "a drag step that snaps back to the same value is not a change"
    );

    // 10 px * 0.1 = 1.0 snaps to 6: one real change.
    dv.on_event(&Event::MouseMove {
        pos: Point::new(cx + 10.0, cy),
    });
    assert_eq!(changes.borrow().as_slice(), [6.0]);
}
