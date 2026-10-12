//! Port of agg-sharp `Tests/Agg.Tests/Agg.UI/TextEditWordWrapTests.cs`: a
//! multi-line field wraps its lines at its width instead of scrolling
//! sideways, while its text stays exactly what was typed.
//!
//! C#'s `new TextEditWidget("", pixelWidth: 100, pixelHeight: 100, multiLine:
//! true) { WordWrap = true }` is a [`TextArea`] laid out 100 x 100 (device
//! scale 1, so C#'s pixels are logical units) at C#'s default 12 points (16
//! pixels to the em at 96 dpi), with no padding (C#'s edit widget draws its
//! text from its own left edge) and one em per line. C#'s visual lines (its
//! `Printer.Text` split at each newline) are the area's wrapped lines. C#'s
//! `OnKeyPress(c)` is a `Key::Char(c)` key down, which is how agg-gui delivers
//! a typed character; `field.Text = ...` is [`TextArea::set_text`]; `field.Width
//! = w` is a layout at the new width. C#'s `TopLeftOffset.X` is how far the
//! text has scrolled sideways; a `TextArea` has no sideways scroll, so it is
//! read as where the first visual line starts drawing.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::event::{Event, Key, Modifiers};
use crate::widget::Widget;

const CHAT_TEXT: &str = "abcdefghijk lmnopqrstuvwxyz123456789";

const LONG_WORD: &str = "abcdefghijklmnopqrstuvwxyz0123456789";

/// C#'s 12 points in pixels: `emSizeInPoints / PointsPerInch * PixelsPerInch`.
const EM_PIXELS: f64 = 12.0 / 72.0 * 96.0;

/// C# `NarrowField(wordWrap: true)`: a 100 x 100 wrapping field in a 300 x
/// 200 container (the container only gives the field a parent; nothing here
/// reads it).
fn narrow_field() -> TextArea {
    narrow_field_wrapping(true)
}

/// C# `NarrowField(wordWrap)`.
fn narrow_field_wrapping(word_wrap: bool) -> TextArea {
    let mut field = TextArea::new(Arc::new(crate::fonts::standard_ui_font()))
        .with_font_size(EM_PIXELS)
        .with_padding(0.0)
        .with_line_spacing(1.0)
        .with_word_wrap(word_wrap);
    set_width(&mut field, 100.0);
    field
}

/// C# `field.Width = width`: the field re-laid out at the new width.
fn set_width(field: &mut TextArea, width: f64) {
    field.set_bounds(Rect::new(0.0, 0.0, width, 100.0));
    field.layout(Size::new(width, 100.0));
}

/// Re-lay the field out at its current size, as the next frame would, so the
/// wrap reflects the latest edit.
fn relayout(field: &mut TextArea) {
    let size = Size::new(field.bounds.width, field.bounds.height);
    field.layout(size);
}

/// C# `VisualLines(field)`: what the field draws, one string per line.
fn visual_lines(field: &mut TextArea) -> Vec<String> {
    relayout(field);
    field.cached_lines.iter().map(|l| l.text.clone()).collect()
}

fn key_press(field: &mut TextArea, c: char) {
    field.on_event(&Event::KeyDown {
        key: Key::Char(c),
        modifiers: Modifiers::default(),
    });
}

fn key_down(field: &mut TextArea, key: Key) {
    relayout(field);
    field.on_event(&Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    });
    relayout(field);
}

/// C# `edit.SetCursorPosition(index)`: the caret at a character index with
/// nothing selected.
fn set_cursor_position(field: &mut TextArea, index: i32) {
    relayout(field);
    field.set_char_index_to_insert_before(index);
    field.set_selection_index_to_start_before(index);
}

/// C# `field.TopLeftOffset.X` at the next frame.
fn top_left_offset_x(field: &mut TextArea) -> f64 {
    relayout(field);
    field.top_left_offset_x()
}

#[test]
fn typing_into_a_narrow_wrapping_field_wraps_instead_of_scrolling_sideways() {
    let mut field = narrow_field();
    for c in CHAT_TEXT.chars() {
        key_press(&mut field, c);
    }

    // a line wider than the field has to continue on the next line
    assert!(visual_lines(&mut field).len() > 1);
    // a wrapping field never scrolls sideways, so the start of the line stays in view
    assert_eq!(top_left_offset_x(&mut field), 0.0);
    // wrapping is display only; no newline is put into the text
    assert_eq!(field.text(), CHAT_TEXT);
    // the line breaks at the space, which is drawn as the line end
    assert_eq!(visual_lines(&mut field)[0], "abcdefghijk");
}

#[test]
fn a_word_too_long_for_a_line_breaks_where_it_overflows() {
    let mut field = narrow_field();
    field.set_text(LONG_WORD);

    let lines = visual_lines(&mut field);
    assert!(lines.len() > 1);
    // an unbroken word breaks between characters and loses none of them
    assert_eq!(lines.concat(), LONG_WORD);
    assert_eq!(field.text(), LONG_WORD);
}

#[test]
fn end_goes_to_the_end_of_the_visual_line() {
    let mut field = narrow_field();
    field.set_text(CHAT_TEXT);
    set_cursor_position(&mut field, 0);
    let first_line_y = field.insert_bar_position().y;

    key_down(&mut field, Key::End);

    // End stops at the end of the first visual line, before the space it wrapped at
    assert_eq!(field.char_index_to_insert_before(), "abcdefghijk".len());
    assert_eq!(field.insert_bar_position().y, first_line_y);
}

#[test]
fn end_on_a_line_that_breaks_inside_a_word_stays_on_that_line() {
    let mut field = narrow_field();
    field.set_text(LONG_WORD);
    set_cursor_position(&mut field, 0);
    let first_line_y = field.insert_bar_position().y;

    key_down(&mut field, Key::End);

    let first_line_len = visual_lines(&mut field)[0].chars().count();
    assert_eq!(field.char_index_to_insert_before(), first_line_len);
    // the caret is drawn at the end of the line End was pressed on, not the start of the next
    assert_eq!(field.insert_bar_position().y, first_line_y);
    assert!(field.insert_bar_position().x > 0.0);
}

#[test]
fn up_and_down_move_between_visual_lines() {
    let mut field = narrow_field();
    field.set_text(CHAT_TEXT);
    set_cursor_position(&mut field, 2);
    let first_line_y = field.insert_bar_position().y;

    key_down(&mut field, Key::ArrowDown);

    // Down moves onto the second visual line, though the text has no second line
    assert!(field.char_index_to_insert_before() > "abcdefghijk".len());
    assert!(field.insert_bar_position().y < first_line_y);

    key_down(&mut field, Key::ArrowUp);

    // Up comes back to the same place on the first line
    assert_eq!(field.char_index_to_insert_before(), 2);
}

#[test]
fn resizing_the_field_rewraps_it_without_changing_the_text() {
    let text_changes = Rc::new(Cell::new(0));
    let counter = Rc::clone(&text_changes);
    let mut field = narrow_field().on_change(move |_| counter.set(counter.get() + 1));
    field.set_text(CHAT_TEXT);
    text_changes.set(0);

    set_width(&mut field, 1000.0);
    // a field wide enough for the whole line shows it on one line
    assert_eq!(visual_lines(&mut field).len(), 1);

    set_width(&mut field, 100.0);
    assert!(visual_lines(&mut field).len() > 1);
    // re-wrapping changes what is drawn, not the text
    assert_eq!(text_changes.get(), 0);
}

#[test]
fn without_word_wrap_the_field_scrolls_sideways_as_before() {
    let mut field = narrow_field_wrapping(false);
    for c in CHAT_TEXT.chars() {
        key_press(&mut field, c);
    }

    assert_eq!(visual_lines(&mut field).concat(), CHAT_TEXT);
    assert_eq!(visual_lines(&mut field).len(), 1);
    // an unwrapped line wider than the field scrolls to keep the caret in view
    assert!(top_left_offset_x(&mut field) < 0.0);

    set_cursor_position(&mut field, 0);
    key_down(&mut field, Key::End);
    assert_eq!(field.char_index_to_insert_before(), CHAT_TEXT.len());
}
