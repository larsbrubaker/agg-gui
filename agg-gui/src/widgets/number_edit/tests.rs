//! Unit tests for [`NumberEdit`]: Up / Down stepping, the commit's fallback
//! to the last good value, expression commits, range clamping, the
//! constructor's option checks and the whole-edit judge on paste.
//!
//! Events go through `send`, which mirrors routed dispatch for a focused
//! field: the box's `preview_event` first, then the field unless the preview
//! consumed it. TextEditTests' NumEdit tests (the C#-ported ones) are in
//! agg-gui-automation's `tests/text_edit/num_edit.rs`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::event::{Event, Key, Modifiers};

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn decimal_edit(start: f64) -> NumberEdit {
    let mut edit = NumberEdit::new(
        font(),
        NumberEditOptions {
            starting_value: start,
            allow_negatives: true,
            allow_decimals: true,
            ..NumberEditOptions::default()
        },
    )
    .expect("valid options");
    send(&mut edit, &Event::FocusGained);
    edit
}

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

fn type_text(edit: &mut NumberEdit, text: &str) {
    for c in text.chars() {
        key(edit, Key::Char(c));
    }
}

fn clear(edit: &mut NumberEdit) {
    edit.text_field_mut().expect("field").set_text("");
}

fn record_commits(edit: NumberEdit) -> (NumberEdit, Rc<RefCell<Vec<NumberCommit>>>) {
    let commits = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&commits);
    let edit = edit.on_edit_complete(move |commit| sink.borrow_mut().push(commit.clone()));
    (edit, commits)
}

/// C#'s `OnKeyDown` steps on Up / Down only when the base edit widget left
/// the key unhandled, and the base always handles both (caret to the line
/// above / below), so C#'s stepping never runs. The box steps as C# meant to.
#[test]
fn rust_only_up_and_down_step_by_the_increment_and_commit() {
    let (mut edit, commits) = record_commits(decimal_edit(2.0));
    key(&mut edit, Key::ArrowUp);
    assert_eq!(edit.value(), 3.0);
    assert_eq!(edit.text(), "3");
    key(&mut edit, Key::ArrowDown);
    key(&mut edit, Key::ArrowDown);
    assert_eq!(edit.text(), "1");
    let values: Vec<f64> = commits.borrow().iter().map(|c| c.value).collect();
    assert_eq!(values, vec![3.0, 2.0, 1.0]);
}

#[test]
fn rust_only_stepping_clamps_to_the_range_and_uses_the_increment() {
    let mut edit = NumberEdit::new(
        font(),
        NumberEditOptions {
            starting_value: 4.0,
            max_value: 5.0,
            increment: 2.0,
            ..NumberEditOptions::default()
        },
    )
    .expect("valid options");
    send(&mut edit, &Event::FocusGained);
    key(&mut edit, Key::ArrowUp);
    assert_eq!(edit.value(), 5.0);
    key(&mut edit, Key::ArrowDown);
    key(&mut edit, Key::ArrowDown);
    key(&mut edit, Key::ArrowDown);
    // no negatives: the minimum was raised to 0
    assert_eq!(edit.value(), 0.0);
    assert_eq!(edit.min_value(), 0.0);
}

#[test]
fn rust_only_arrow_stepping_can_be_turned_off() {
    let mut edit = decimal_edit(2.0).with_arrow_stepping(false);
    key(&mut edit, Key::ArrowUp);
    assert_eq!(edit.text(), "2");
}

#[test]
fn rust_only_unreadable_commit_falls_back_to_the_last_good_value() {
    let (mut edit, commits) = record_commits(decimal_edit(7.5));
    clear(&mut edit);
    type_text(&mut edit, "-.");
    assert_eq!(edit.text(), "-.");
    key(&mut edit, Key::Enter);
    assert_eq!(edit.text(), "7.5");
    assert_eq!(edit.value(), 7.5);
    assert_eq!(commits.borrow().last().map(|c| c.value), Some(7.5));

    // a good entry becomes the new fallback
    clear(&mut edit);
    type_text(&mut edit, "3");
    key(&mut edit, Key::Enter);
    clear(&mut edit);
    type_text(&mut edit, ".");
    send(&mut edit, &Event::FocusLost);
    assert_eq!(edit.text(), "3");
    assert_eq!(commits.borrow().len(), 3);
}

#[test]
fn rust_only_commit_clamps_the_typed_value_into_range() {
    let mut edit = decimal_edit(0.0);
    edit.set_max_value(10.0).expect("decimal box");
    clear(&mut edit);
    type_text(&mut edit, "25");
    key(&mut edit, Key::Enter);
    assert_eq!(edit.text(), "10");
    // Enter committed the rewritten text: focus loss commits nothing more
    let (mut edit, commits) = record_commits(edit);
    send(&mut edit, &Event::FocusLost);
    assert!(commits.borrow().is_empty());
}

/// C#'s `OnEditComplete` says an expression "is left alone", but then reads
/// `Value` (the error value, 0) and writes it back, replacing "=10*2" with
/// "0". The box leaves the expression for its owner, as C# documents.
#[test]
fn rust_only_expression_commit_leaves_the_text_for_the_owner() {
    let (mut edit, commits) = record_commits(decimal_edit(4.0).with_allow_expression_entry(true));
    clear(&mut edit);
    type_text(&mut edit, "=10*2");
    key(&mut edit, Key::Enter);
    assert_eq!(edit.text(), "=10*2");
    let commit = commits.borrow().last().cloned().expect("committed");
    assert_eq!(commit.text, "=10*2");
    assert_eq!(commit.value, 4.0);
}

#[test]
fn rust_only_options_the_box_cannot_hold_are_refused() {
    let negative = NumberEditOptions {
        starting_value: -1.0,
        ..NumberEditOptions::default()
    };
    assert_eq!(
        NumberEdit::new(font(), negative).err(),
        Some(NumberEditError::NegativeNotAllowed)
    );
    let fraction = NumberEditOptions {
        increment: 0.5,
        ..NumberEditOptions::default()
    };
    assert_eq!(
        NumberEdit::new(font(), fraction).err(),
        Some(NumberEditError::FractionNotAllowed)
    );
    let mut whole = NumberEdit::new(font(), NumberEditOptions::default()).expect("defaults");
    assert_eq!(
        whole.set_max_value(2.5),
        Err(NumberEditError::FractionNotAllowed)
    );
    assert_eq!(
        whole.set_min_value(-1.0),
        Err(NumberEditError::NegativeNotAllowed)
    );
}

#[test]
fn rust_only_paste_is_judged_whole() {
    let mut edit = decimal_edit(0.0);
    clear(&mut edit);
    let field = edit.text_field_mut().expect("field");
    field.replace_range(0, 0, "1.5");
    assert_eq!(field.text(), "1.5");
    field.replace_range(3, 0, ".2");
    assert_eq!(field.text(), "1.5");
    field.replace_range(0, 3, "abc");
    assert_eq!(field.text(), "1.5");
}

#[test]
fn rust_only_max_decimal_places_rounds_what_is_shown() {
    let mut edit = decimal_edit(1.23456).with_max_decimal_places(2);
    assert_eq!(edit.text(), "1.23");
    edit.set_value(2.0);
    assert_eq!(edit.text(), "2");
}

#[test]
fn rust_only_commit_hands_the_parsed_entry_to_the_callback() {
    let edit = decimal_edit(0.0).with_text_value_parser(|text| {
        text.strip_suffix("in")
            .and_then(|n| n.parse::<f64>().ok())
            .map(|n| n * 25.4)
    });
    let (mut edit, commits) = record_commits(edit);
    clear(&mut edit);
    type_text(&mut edit, "2in");
    key(&mut edit, Key::Enter);
    let commit = commits.borrow().last().cloned().expect("committed");
    assert_eq!(commit.value, 50.8);
    assert_eq!(commit.text, "50.8");
    assert_eq!(commit.last_parsed_text.as_deref(), Some("2in"));
}
