//! Unit tests for [`TextArea::on_edit_complete`] (`text_area/callbacks.rs`):
//! it mirrors `TextField::on_edit_complete` — fires once when focus leaves
//! after the text changed — except that no key commits, because Enter
//! (with or without Ctrl/Cmd) inserts a newline in a multi-line editor.
//! Sibling of `tests.rs`, which covers `on_change`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::event::{Event, Key, Modifiers};
use crate::widget::Widget;

const FONT_BYTES: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

/// A laid-out `TextArea` with `text` whose edit-complete callback records
/// every text it is given into the returned log.
fn area_with_log(text: &str) -> (TextArea, Rc<RefCell<Vec<String>>>) {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let log = Rc::new(RefCell::new(Vec::new()));
    let log2 = Rc::clone(&log);
    let mut ta = TextArea::new(font)
        .with_text(text)
        .on_edit_complete(move |s| log2.borrow_mut().push(s.to_string()));
    ta.layout(Size::new(200.0, 80.0));
    (ta, log)
}

fn key_with(ta: &mut TextArea, k: Key, modifiers: Modifiers) {
    ta.on_event(&Event::KeyDown { key: k, modifiers });
}

fn key(ta: &mut TextArea, k: Key) {
    key_with(ta, k, Modifiers::default());
}

#[test]
fn edit_complete_fires_once_on_focus_loss_after_edit() {
    let (mut ta, log) = area_with_log("");
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('a'));
    key(&mut ta, Key::Char('b'));
    assert!(log.borrow().is_empty(), "typing alone does not commit");

    ta.on_event(&Event::FocusLost);
    assert_eq!(*log.borrow(), vec!["ab".to_string()]);

    // A second FocusLost without a new edit reports nothing new.
    ta.on_event(&Event::FocusLost);
    assert_eq!(log.borrow().len(), 1, "fires once per edit");
}

#[test]
fn edit_complete_silent_without_change() {
    let (mut ta, log) = area_with_log("hello");
    ta.on_event(&Event::FocusGained);
    ta.on_event(&Event::FocusLost);
    assert!(log.borrow().is_empty(), "focus in/out without an edit");

    // An edit that is undone by hand before leaving is no change either.
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('x'));
    key(&mut ta, Key::Backspace);
    ta.on_event(&Event::FocusLost);
    assert!(log.borrow().is_empty(), "text back to its focus-time value");
}

#[test]
fn edit_complete_not_fired_by_enter() {
    let (mut ta, log) = area_with_log("");
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('a'));
    key(&mut ta, Key::Enter);
    key_with(
        &mut ta,
        Key::Enter,
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
    key_with(
        &mut ta,
        Key::Enter,
        Modifiers {
            meta: true,
            ..Modifiers::default()
        },
    );
    key(&mut ta, Key::Char('b'));
    assert!(
        log.borrow().is_empty(),
        "Enter inserts a newline, no commit"
    );
    assert_eq!(ta.text(), "a\n\n\nb");

    ta.on_event(&Event::FocusLost);
    assert_eq!(*log.borrow(), vec!["a\n\n\nb".to_string()]);
}

#[test]
fn edit_complete_compares_against_latest_focus_snapshot() {
    let (mut ta, log) = area_with_log("");
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('a'));
    ta.on_event(&Event::FocusLost);
    // Refocus and leave unchanged: the committed "a" is the new baseline.
    ta.on_event(&Event::FocusGained);
    ta.on_event(&Event::FocusLost);
    // Refocus, edit, leave: fires with the new text.
    ta.on_event(&Event::FocusGained);
    key(&mut ta, Key::Char('b'));
    ta.on_event(&Event::FocusLost);
    assert_eq!(*log.borrow(), vec!["a".to_string(), "ab".to_string()]);
}
