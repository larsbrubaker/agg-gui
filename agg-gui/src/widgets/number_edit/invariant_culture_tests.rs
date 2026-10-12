//! agg-sharp `Tests/Agg.Tests/Other/InvariantCultureTests.cs`, the rows
//! that cover `InternalNumberEdit` and `DisplayNumber`: the box shows the
//! user's decimal separator, reads both it and `.`, refuses a group
//! separator as it is typed, and refuses a thousands-grouped whole number
//! rather than guessing. C# sets `CultureInfo.CurrentCulture`; here the
//! box's [`NumberFormat`] carries the culture's separators (de-DE: `,`
//! decimal, `.` groups; en-US: `.` decimal, `,` groups). The class's other
//! rows (type converters, vectors, vertex files, G-code text, packed
//! polygons, STL, game data) are not agg-gui code.
//!
//! Plus the `rust_only_*` tests of the format hook itself: typing and
//! stepping under a comma format, and a format set after construction.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::display_number::{format_in, is_ambiguous_grouping_in};
use super::*;
use crate::event::{Event, Key, Modifiers};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const GERMAN: NumberFormat = NumberFormat::new(',', Some('.'));
const US: NumberFormat = NumberFormat::new('.', Some(','));

fn culture(name: &str) -> NumberFormat {
    match name {
        "de-DE" => GERMAN,
        "en-US" => US,
        other => panic!("no test culture {other}"),
    }
}

/// C#'s `new InternalNumberEdit(start, 12, true, true, min, max, 1, 0)`
/// under `format`.
fn edit_in(format: NumberFormat, start: f64, min: f64, max: f64) -> NumberEdit {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut edit = NumberEdit::new(
        font,
        NumberEditOptions {
            starting_value: start,
            allow_negatives: true,
            allow_decimals: true,
            min_value: min,
            max_value: max,
            increment: 1.0,
        },
    )
    .expect("valid options")
    .with_number_format(format);
    send(&mut edit, &Event::FocusGained);
    edit
}

/// Routed dispatch for a focused field: the box's preview, then the field.
fn send(edit: &mut NumberEdit, event: &Event) {
    if edit.preview_event(event).is_consumed() {
        return;
    }
    edit.children_mut()[0].on_event(event);
}

fn key(edit: &mut NumberEdit, key: Key) {
    send(
        edit,
        &Event::KeyDown {
            key,
            modifiers: Modifiers::default(),
        },
    );
}

/// C# `edit.Text = text`.
fn set_text(edit: &mut NumberEdit, text: &str) {
    edit.text_field_mut().expect("field").set_text(text);
}

/// C# `edit.CharIndexToInsertBefore = index`.
fn set_caret(edit: &mut NumberEdit, index: usize) {
    edit.text_field_mut()
        .expect("field")
        .set_cursor_position(index);
}

#[test]
fn number_edit_shows_the_users_decimal_and_reads_both() {
    // The edit is what the user reads, so it shows their decimal comma; it still reads a typed '.'.
    let mut edit = edit_in(GERMAN, 1.5, -100.0, 100.0);
    assert_eq!(edit.text(), "1,5");
    assert_eq!(edit.value(), 1.5);

    set_text(&mut edit, "2.25");
    assert_eq!(edit.value(), 2.25);

    edit.set_value(3.5);
    assert_eq!(edit.text(), "3,5");
    assert_eq!(format_in(1234.5, -1, &GERMAN), "1234,5");
}

/// A thousands separator cannot be typed: "1.000" is one under de-DE, and
/// the ',' that would make it "1.000,5" is refused, as the ',' after "1" is
/// under en-US.
#[test]
fn number_edit_refuses_a_grouping_separator_as_it_is_typed() {
    for (name, start, grouping) in [("de-DE", "1.000", ','), ("en-US", "1", ',')] {
        let mut edit = edit_in(culture(name), 0.0, -10000.0, 10000.0);
        set_text(&mut edit, start);
        set_caret(&mut edit, start.chars().count());
        key(&mut edit, Key::Char(grouping));
        assert_eq!(edit.text(), start, "{name}");
    }
}

#[test]
fn a_thousands_grouped_whole_number_is_ambiguous() {
    for (name, text, ambiguous) in [
        ("de-DE", "1.000", true),
        ("de-DE", "12.500", true),
        ("de-DE", "1.000.000", true),
        ("de-DE", "-1.000", true),
        ("de-DE", "0.500", false),
        ("de-DE", "-0.250", false),
        ("de-DE", "1.5", false),
        ("de-DE", "2.25", false),
        ("de-DE", "1.0000", false),
        ("de-DE", "1234.500", false),
        ("en-US", "1,000", true),
        ("en-US", "1,000,000", true),
        ("en-US", "1.000", false),
        ("en-US", "12,5", false),
    ] {
        assert_eq!(
            is_ambiguous_grouping_in(text, &culture(name)),
            ambiguous,
            "{name} {text:?}"
        );
    }
}

/// Committing text the edit cannot read - a German "1.000" is a thousand or
/// one - keeps the last value it held rather than reading as 0, so every
/// caller that takes the value at commit gets a real number.
#[test]
fn committing_unreadable_text_keeps_the_last_value() {
    for (name, typed, shown) in [
        ("de-DE", "1.000", "2,5"),
        ("en-US", "1,000", "2.5"),
        ("en-US", ".", "2.5"),
    ] {
        let committed = Rc::new(RefCell::new(f64::NAN));
        let sink = Rc::clone(&committed);
        let mut edit = edit_in(culture(name), 2.5, -10000.0, 10000.0)
            .on_edit_complete(move |commit| *sink.borrow_mut() = commit.value);

        set_text(&mut edit, typed);
        edit.commit();
        assert_eq!(*committed.borrow(), 2.5, "{name} {typed:?}");
        assert_eq!(edit.text(), shown, "{name} {typed:?}");

        // a readable commit becomes the value kept next time
        set_text(&mut edit, "4");
        edit.commit();
        set_text(&mut edit, typed);
        edit.commit();
        assert_eq!(*committed.borrow(), 4.0, "{name} {typed:?}");
    }
}

#[test]
fn number_edit_reads_the_users_comma_decimal() {
    // A German keyboard writes "2,5"; an invariant read with group separators made that 25.
    let mut edit = edit_in(GERMAN, 0.0, -100.0, 100.0);
    set_text(&mut edit, "2,5");
    assert_eq!(edit.value(), 2.5);

    // and the comma can be typed at all
    set_text(&mut edit, "3");
    set_caret(&mut edit, 1);
    key(&mut edit, Key::Char(','));
    key(&mut edit, Key::Char('5'));
    assert_eq!(edit.text(), "3,5");
    assert_eq!(edit.value(), 3.5);
}

#[test]
fn number_edit_does_not_read_a_comma_as_a_group_separator() {
    // ',' is not a decimal here and cannot be typed; text that holds one is not a number
    let mut edit = edit_in(US, 0.0, -100.0, 100.0);
    set_text(&mut edit, "2,5");
    assert_eq!(edit.value(), 0.0);
    set_text(&mut edit, "2.5");
    assert_eq!(edit.value(), 2.5);
}

#[test]
fn rust_only_typing_one_comma_five_commits_one_and_a_half_and_steps_to_two_comma_five() {
    let commits = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&commits);
    let mut edit = edit_in(GERMAN, 0.0, -100.0, 100.0)
        .on_edit_complete(move |commit| sink.borrow_mut().push(commit.clone()));
    set_text(&mut edit, "");
    for c in "1,5".chars() {
        key(&mut edit, Key::Char(c));
    }
    assert_eq!(edit.text(), "1,5");
    key(&mut edit, Key::Enter);
    assert_eq!(commits.borrow().last().map(|c| c.value), Some(1.5));
    assert_eq!(edit.text(), "1,5");

    key(&mut edit, Key::ArrowUp);
    assert_eq!(edit.text(), "2,5");
    assert_eq!(commits.borrow().last().map(|c| c.value), Some(2.5));
    key(&mut edit, Key::ArrowDown);
    key(&mut edit, Key::ArrowDown);
    assert_eq!(edit.text(), "0,5");
}

/// C# admits '.' under every culture (it is read invariant first), so a
/// comma-decimal user may type "1.5"; the commit shows it back as "1,5".
#[test]
fn rust_only_a_point_under_a_comma_format_reads_invariant_and_shows_a_comma() {
    let mut edit = edit_in(GERMAN, 0.0, -100.0, 100.0);
    set_text(&mut edit, "");
    for c in "1.5".chars() {
        key(&mut edit, Key::Char(c));
    }
    assert_eq!(edit.text(), "1.5");
    assert_eq!(edit.value(), 1.5);
    key(&mut edit, Key::Enter);
    assert_eq!(edit.text(), "1,5");

    // the two separators never mix: "1.5" then ',' reads as neither
    set_text(&mut edit, "1.5");
    set_caret(&mut edit, 3);
    key(&mut edit, Key::Char(','));
    assert_eq!(edit.text(), "1.5");
}

/// The starts of a number in the format's own separator are typeable: ","
/// and "-," as well as "." and "-.".
#[test]
fn rust_only_a_lone_comma_decimal_is_the_start_of_a_number() {
    let mut edit = edit_in(GERMAN, 0.0, -100.0, 100.0);
    set_text(&mut edit, "");
    key(&mut edit, Key::Char(','));
    assert_eq!(edit.text(), ",");
    set_text(&mut edit, "-");
    set_caret(&mut edit, 1);
    key(&mut edit, Key::Char(','));
    assert_eq!(edit.text(), "-,");
    key(&mut edit, Key::Char('5'));
    assert_eq!(edit.value(), -0.5);
}

/// The default is the invariant format, and a format set later re-shows a
/// readable value in it, with decimal places kept.
#[test]
fn rust_only_the_default_is_invariant_and_a_later_format_reshows_the_value() {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let options = NumberEditOptions {
        starting_value: 1.23456,
        allow_decimals: true,
        ..NumberEditOptions::default()
    };
    let mut edit = NumberEdit::new(font, options)
        .expect("valid options")
        .with_max_decimal_places(3);
    assert_eq!(edit.number_format(), NumberFormat::invariant());
    assert_eq!(NumberFormat::default(), NumberFormat::invariant());
    assert_eq!(edit.text(), "1.235");
    edit.set_number_format(GERMAN);
    assert_eq!(edit.text(), "1,235");

    // unreadable text is left as typed
    set_text(&mut edit, "1.000");
    edit.set_number_format(GERMAN);
    assert_eq!(edit.text(), "1.000");
}
