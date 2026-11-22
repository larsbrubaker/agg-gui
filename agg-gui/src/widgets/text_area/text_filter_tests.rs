//! Tests for [`TextArea::with_text_filter`] (`text_filter.rs`): a length
//! limit refuses typing past it, cuts a paste or IME commit to fit at a
//! character boundary, counts the selection a paste replaces, and leaves undo
//! and redo exact. Driven through real events where one exists, and through
//! `insert_str`, the funnel the paste chord and the context menu's Paste call.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::event::Event;
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

/// A focused area holding `text`, its caret at the end, limited to `limit`
/// characters.
fn limited(text: &str, limit: usize) -> TextArea {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut ta = TextArea::new(font)
        .with_text(text)
        .with_text_filter(move |_, result| result.chars().count() <= limit);
    ta.layout(Size::new(400.0, 200.0));
    ta.on_event(&Event::FocusGained);
    ta.set_cursor_to_end();
    ta
}

fn press(ta: &mut TextArea, key: Key) -> EventResult {
    ta.on_event(&Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    })
}

#[test]
fn typing_past_the_limit_is_refused_and_the_key_still_consumed() {
    let mut ta = limited("", 3);
    for c in "abcd".chars() {
        assert!(press(&mut ta, Key::Char(c)).is_consumed());
    }
    assert_eq!(ta.text(), "abc");
    assert!(press(&mut ta, Key::Enter).is_consumed());
    assert!(press(&mut ta, Key::Tab).is_consumed());
    assert_eq!(ta.text(), "abc", "Enter and Tab are insertions too");
    // The refused keys left no undo steps: one undo removes the last letter.
    ta.undo();
    assert_eq!(ta.text(), "ab");
}

#[test]
fn a_paste_is_cut_to_fit_without_splitting_a_character() {
    // A byte limit, so a cut inside a multi-byte character would fit.
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&seen);
    let mut ta = TextArea::new(font)
        .with_text("ab")
        .with_text_filter(move |inserted, result| {
            log.borrow_mut().push(inserted.to_string());
            result.len() <= 6
        });
    ta.layout(Size::new(400.0, 200.0));
    ta.set_cursor_to_end();
    // "ab" + "é" is 4 bytes, + "漢" would be 7: cut after "é", not inside "漢".
    ta.insert_str("é漢字");
    assert_eq!(ta.text(), "abé");
    assert_eq!(ta.edit_state().borrow().cursor, "abé".len());
    for part in seen.borrow().iter() {
        assert!(
            "é漢字".starts_with(part.as_str()) && !part.is_empty(),
            "the filter only ever sees whole leading characters: {part:?}"
        );
    }
}

#[test]
fn an_ime_commit_and_a_multi_line_paste_are_cut_to_fit() {
    let mut ta = limited("", 8);
    // Line ends are normalised before the filter sees them, so "\r\n" is one
    // character of the limit.
    ta.insert_str("one\r\ntwo\r\nthree");
    assert_eq!(ta.text(), "one\ntwo\n");

    let mut ta = limited("abc", 5);
    ta.on_event(&Event::TextInput {
        text: "xyz".to_string(),
    });
    assert_eq!(ta.text(), "abcxy");
}

#[test]
fn replacing_a_selection_counts_the_text_it_removes() {
    let mut ta = limited("hello world", 11);
    ta.set_selection(6, 11);
    // "hello " + "there!" is 12: the removed "world" frees five characters,
    // so "there" fits and the "!" is cut.
    ta.insert_str("there!");
    assert_eq!(ta.text(), "hello there");
    assert_eq!(ta.selection(), None);
}

/// Caret and anchor of `ta`.
fn caret(ta: &TextArea) -> (usize, usize) {
    let st = ta.edit_state();
    let st = st.borrow();
    (st.cursor, st.anchor)
}

#[test]
fn undo_after_a_cut_paste_restores_exactly_and_redo_repeats_the_cut() {
    // The same paste into an area with no filter, whose undo is the
    // reference: a cut must not change what undo restores.
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut free = TextArea::new(font).with_text("abc");
    free.layout(Size::new(400.0, 200.0));
    free.on_event(&Event::FocusGained);
    free.set_cursor_to_end();
    free.set_selection(1, 2);
    free.insert_str("WXYZ");
    free.undo();

    let mut ta = limited("abc", 5);
    ta.set_selection(1, 2);
    ta.insert_str("WXYZ");
    // "a" + "c" leave three characters free.
    assert_eq!(ta.text(), "aWXYc");
    assert_eq!(caret(&ta), (4, 4), "the caret ends after what was inserted");
    ta.undo();
    assert_eq!(ta.text(), "abc");
    assert_eq!(
        caret(&ta),
        caret(&free),
        "undo restores what it would uncut"
    );
    assert!(!ta.can_undo(), "the cut paste was one step");
    ta.redo();
    assert_eq!(ta.text(), "aWXYc");
    assert_eq!(caret(&ta), (4, 4));
}

#[test]
fn a_paste_nothing_of_which_fits_changes_nothing() {
    let mut ta = limited("abc", 3);
    ta.set_selection(0, 0);
    ta.insert_str("xyz");
    assert_eq!(ta.text(), "abc");
    assert!(!ta.can_undo(), "a refused edit records no undo step");
}

#[test]
fn an_indent_over_several_lines_is_refused_whole() {
    let mut ta = limited("a\nb", 7);
    ta.set_selection(0, 3);
    // Indenting both lines adds eight characters: refused, not half done.
    assert!(press(&mut ta, Key::Tab).is_consumed());
    assert_eq!(ta.text(), "a\nb");
}

#[test]
fn without_a_filter_nothing_is_cut() {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let mut ta = TextArea::new(font).with_text("ab");
    ta.layout(Size::new(400.0, 200.0));
    ta.set_cursor_to_end();
    ta.insert_str("cdefgh");
    assert_eq!(ta.text(), "abcdefgh");
}
