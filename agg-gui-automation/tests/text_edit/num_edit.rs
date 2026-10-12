//! TextEditTests' NumEdit tests: agg-sharp's `NumberEdit` is
//! [`agg_gui::widgets::NumberEdit`], a wrapper around one `TextField`.
//!
//! C#'s `new NumberEdit(0, 0, 0, 12, 200, 16, true, true)` is a box starting
//! at 0 with negatives and decimals, its field at C#'s 12 point size with no
//! inset, 200 wide at its own height, at the container's origin. C#'s
//! `numberEdit.InternalNumberEdit.OnKeyPress(args)` called directly is the
//! box's own dispatch of one typed character (`HeadlessWindow::on_key_down`
//! reaches the same field); a refused key is consumed (C# `Handled`). C#'s
//! `TopLeftOffset.Y == 0` (no vertical scroll) has no counterpart in a
//! single-line agg-gui field, which never scrolls vertically.

use std::sync::Arc;

use agg_gui::event::{Event, Key, Modifiers};
use agg_gui::widgets::{NumberEdit, NumberEditOptions};
use agg_gui::{Rect, Size, Widget};
use agg_gui_automation::HeadlessWindow;

use super::{down, handle, send_char, send_key, up, POINT_SIZE};

const NAME: &str = "numberEdit";

/// C#'s container (200 x 200) holding `new NumberEdit(0, 0, 0, 12, 200, 16,
/// true, true)`, clicked at its top-left as every NumEdit test starts.
fn clicked_number_edit() -> HeadlessWindow {
    let mut edit = NumberEdit::new(
        Arc::new(agg_gui::fonts::standard_ui_font()),
        NumberEditOptions {
            starting_value: 0.0,
            allow_negatives: true,
            allow_decimals: true,
            ..NumberEditOptions::default()
        },
    )
    .expect("negatives and decimals allowed")
    .map_field(|field| field.with_font_size(POINT_SIZE).with_padding(0.0))
    .with_name(NAME);
    let height = edit.layout(Size::new(200.0, 1000.0)).height;
    edit.set_bounds(Rect::new(0.0, 0.0, 200.0, height));
    let mut container = HeadlessWindow::new(200.0, 200.0);
    container.add_child(Box::new(edit));
    down(&mut container, 1, 1.0, height - 1.0);
    up(&mut container, 1.0, height - 1.0);
    container
}

fn number_edit(container: &HeadlessWindow) -> &NumberEdit {
    handle(container, NAME)
        .downcast::<NumberEdit>(container.root())
        .expect("the number edit is a NumberEdit")
}

fn number_edit_mut(container: &mut HeadlessWindow) -> &mut NumberEdit {
    let handle = handle(container, NAME);
    handle
        .widget_mut(container.driver_mut().root_mut())
        .and_then(|widget| widget.as_any_mut())
        .and_then(|any| any.downcast_mut::<NumberEdit>())
        .expect("the number edit is a NumberEdit")
}

fn text(container: &HeadlessWindow) -> String {
    number_edit(container).text()
}

fn value(container: &HeadlessWindow) -> f64 {
    number_edit(container).value()
}

fn cursor(container: &HeadlessWindow) -> usize {
    number_edit(container)
        .text_field()
        .expect("the field")
        .cursor_pos()
}

/// C# `numberEdit.CharIndexToInsertBefore = index`.
fn set_cursor(container: &mut HeadlessWindow, index: usize) {
    number_edit_mut(container)
        .text_field_mut()
        .expect("the field")
        .set_cursor_position(index);
}

/// C# `SendKey(Keys.Back, ...)` then `SendKey(Keys.Delete, ...)`.
fn back_and_delete(container: &mut HeadlessWindow) {
    send_key(container, Key::Backspace, Modifiers::default());
    send_key(container, Key::Delete, Modifiers::default());
}

#[test]
fn num_edit_handles_non_number_chars() {
    let mut container = clicked_number_edit();
    let c = &mut container;

    assert!(cursor(c) == 0);

    // type a . (non numeric character)
    back_and_delete(c);
    send_char(c, '-');
    assert!(value(c) == 0.0);
    send_char(c, '.');
    assert!(value(c) == 0.0);
    // C# `SendKey(Keys.D0, '.')`: the character typed is '.'
    send_char(c, '.');
    assert!(value(c) == 0.0);
    send_char(c, 'A');
    assert!(value(c) == 0.0);
}

/// A number edit given a text value parser is letting its owner spell
/// values as something other than a plain number - a unit written on the
/// number, say - so the letters that spelling needs have to be typeable,
/// and the parser's reading is the value the field commits.
#[test]
fn num_edit_with_text_parser_accepts_letters_and_commits_the_parsed_value() {
    let mut container = clicked_number_edit();
    let c = &mut container;

    back_and_delete(c);

    // with no parser set there is nothing that can read a letter, so the field refuses to take one
    send_char(c, '2');
    send_char(c, 'i');
    assert_eq!(text(c), "2");
    assert_eq!(value(c), 2.0);
    assert_eq!(number_edit(c).last_parsed_text(), None);

    // agg knows nothing about units - to this field "in" simply means "times ten"
    number_edit_mut(c).set_text_value_parser(Some(Box::new(|text: &str| {
        text.strip_suffix("in")
            .and_then(|number| number.parse::<f64>().ok())
            .map(|number| number * 10.0)
    })));

    send_char(c, 'i');
    send_char(c, 'n');
    assert_eq!(text(c), "2in");
    assert_eq!(value(c), 20.0);
    assert_eq!(number_edit(c).last_parsed_text().as_deref(), Some("2in"));

    // a digit typed beside the letters is kept too - with a parser in play it is the judge of
    // what reads, and "12in" is only unreadable to a plain numeric parse
    set_cursor(c, 0);
    send_char(c, '1');
    assert_eq!(text(c), "12in");
    assert_eq!(value(c), 120.0);

    // Committing writes the parsed value back as a plain number, and the entry it came from stays
    // readable - that text is the only exact record of what the user asked for once the box has
    // rounded the number it shows.
    number_edit_mut(c).commit();
    assert_eq!(text(c), "120");
    assert_eq!(number_edit(c).last_parsed_text().as_deref(), Some("12in"));
}

/// A keystroke that would leave the field holding something it cannot read
/// is refused outright, and the text is left exactly as it was.
#[test]
fn num_edit_refuses_keys_it_cannot_read() {
    let mut container = clicked_number_edit();
    let c = &mut container;

    back_and_delete(c);

    // a minus part way through a number is not a number, so it never lands
    send_char(c, '1');
    send_char(c, '-');
    send_char(c, '2');
    assert_eq!(text(c), "12");
    assert_eq!(value(c), 12.0);

    // nor is a second decimal point
    send_char(c, '.');
    send_char(c, '2');
    send_char(c, '.');
    send_char(c, '3');
    assert_eq!(text(c), "12.23");
    assert_eq!(value(c), 12.23);

    // and with no parser to read them, letters are not typeable at all
    send_char(c, 'a');
    assert_eq!(text(c), "12.23");
    assert_eq!(value(c), 12.23);
}

/// A number edit whose owner also takes expressions lets text that begins
/// with '=' be typed, so the owner's change handler gets a chance to take
/// the entry over. A plain number field still refuses '=' outright - it is
/// not a number and nothing there could ever read it.
#[test]
fn num_edit_takes_leading_equals_only_when_expression_entry_is_allowed() {
    let mut container = clicked_number_edit();
    let c = &mut container;

    back_and_delete(c);
    assert_eq!(text(c), "");

    // by default the field considers '=' and refuses it, and says so, so nothing above acts on it
    let refused = number_edit_mut(c).children_mut()[0].on_event(&Event::KeyDown {
        key: Key::Char('='),
        modifiers: Modifiers::default(),
    });
    assert_eq!(text(c), "");
    assert!(refused.is_consumed());

    number_edit_mut(c).set_allow_expression_entry(true);

    send_char(c, '=');
    assert_eq!(text(c), "=");

    // and the rest of the expression follows, unreadable as a number though it is
    send_char(c, '1');
    send_char(c, '0');
    send_char(c, '*');
    send_char(c, '2');
    assert_eq!(text(c), "=10*2");

    // only a leading '=' hands the entry to the owner - part way through a number it is still refused
    number_edit_mut(c)
        .text_field_mut()
        .expect("the field")
        .set_text("12");
    set_cursor(c, 2);
    send_char(c, '=');
    assert_eq!(text(c), "12");
}
