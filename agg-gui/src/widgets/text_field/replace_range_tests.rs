//! `TextField::replace_range` (agg-sharp `InternalTextEditWidget.ReplaceRange`):
//! one undo step for a real change, none for a no-op, read-only fields left
//! alone, line endings normalized and the field's char filter applied.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::event::Event;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn ctrl_key(field: &mut TextField, c: char) {
    field.on_event(&Event::FocusGained);
    field.on_event(&Event::KeyDown {
        key: Key::Char(c),
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    });
}

#[test]
fn replace_range_is_one_undo_step_with_the_caret_after_the_insert() {
    let mut field = TextField::new(font()).with_text("=sel");
    field.replace_range(1, 3, "self.");
    assert_eq!(field.text(), "=self.");
    assert_eq!(field.cursor_pos(), 6);
    ctrl_key(&mut field, 'z');
    assert_eq!(field.text(), "=sel");
}

#[test]
fn replace_range_that_changes_nothing_adds_no_undo_step_and_no_change() {
    let changes = Rc::new(Cell::new(0));
    let c = Rc::clone(&changes);
    let mut field = TextField::new(font())
        .with_text("cat")
        .on_change(move |_| c.set(c.get() + 1));
    field.replace_range(0, 3, "dog");
    assert_eq!(changes.get(), 1);
    field.set_cursor_position(0);
    field.replace_range(0, 3, "dog");
    assert_eq!(changes.get(), 1, "no change, no notification");
    assert_eq!(field.cursor_pos(), 3, "a no-op still moves the caret");
    ctrl_key(&mut field, 'z');
    assert_eq!(field.text(), "cat", "the only undo step is the real change");
}

#[test]
fn replace_range_leaves_a_read_only_field_alone() {
    let mut field = TextField::new(font()).with_text("cat").with_read_only(true);
    field.replace_range(0, 3, "dog");
    assert_eq!(field.text(), "cat");
}

#[test]
fn replace_range_normalizes_line_endings_and_applies_the_char_filter() {
    let mut field = TextField::new(font());
    field.replace_range(0, 0, "a\r\nb\rc");
    assert_eq!(field.text(), "a\nb\nc");

    let mut digits = TextField::new(font()).with_char_filter(|c| c.is_ascii_digit());
    digits.replace_range(0, 0, "1a2");
    assert_eq!(digits.text(), "12");
    assert_eq!(digits.cursor_pos(), 2);
}
