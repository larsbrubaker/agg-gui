//! Committed text (`Event::TextInput`) and IME composition
//! (`Event::ImePreedit`) reaching the focused text widget through the real
//! [`App`] routing, the `InputForwarder` entry, and function keys reaching the
//! app as `Key::F(n)`.

use super::*;
use crate::shell_input::{ForwarderEvent, InputForwarder};
use crate::text::Font;
use crate::widgets::text_field_core::TextEditState;
use crate::widgets::TextArea;
use crate::Event;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const VIEW_H: f64 = 40.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn focus_click(app: &mut App) {
    app.on_mouse_down(20.0, VIEW_H * 0.5, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(20.0, VIEW_H * 0.5, MouseButton::Left, Modifiers::default());
}

/// An App whose root is one focused `TextField` bound to the returned cell.
fn field_app(text: &str) -> (App, Rc<RefCell<String>>) {
    let cell = Rc::new(RefCell::new(text.to_string()));
    let field = TextField::new(font()).with_text_cell(Rc::clone(&cell));
    let mut app = App::new(Box::new(field));
    app.layout(Size::new(400.0, VIEW_H));
    focus_click(&mut app);
    (app, cell)
}

fn field(app: &App) -> &TextField {
    app.root()
        .as_any()
        .and_then(|a| a.downcast_ref::<TextField>())
        .expect("root is a TextField")
}

/// An App whose root is one focused `TextArea` sharing the returned state.
fn area_app(text: &str) -> (App, Rc<RefCell<TextEditState>>) {
    let area = TextArea::new(font()).with_text(text);
    let state = area.edit_state();
    let mut app = App::new(Box::new(area));
    app.layout(Size::new(400.0, 120.0));
    app.on_mouse_down(20.0, 100.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(20.0, 100.0, MouseButton::Left, Modifiers::default());
    (app, state)
}

#[test]
fn committed_text_reaches_a_focused_text_field() {
    let (mut app, cell) = field_app("ab");
    app.on_key_down(Key::End, Modifiers::default());
    assert!(app.on_text_input("日本語"));
    assert_eq!(*cell.borrow(), "ab日本語");
    // Plain key typing still works beside it.
    app.on_key_down(Key::Char('c'), Modifiers::default());
    assert_eq!(*cell.borrow(), "ab日本語c");
}

#[test]
fn committed_text_replaces_the_selection() {
    let (mut app, cell) = field_app("hello");
    app.on_key_down(
        Key::Char('a'),
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
    app.on_text_input("你好");
    assert_eq!(*cell.borrow(), "你好");
}

#[test]
fn committed_text_is_one_undo_step() {
    let (mut app, cell) = field_app("ab");
    app.on_key_down(Key::End, Modifiers::default());
    app.on_text_input("かな");
    assert_eq!(*cell.borrow(), "abかな");
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    app.on_key_down(Key::Char('z'), ctrl);
    assert_eq!(*cell.borrow(), "ab");
}

#[test]
fn preedit_is_shown_inline_without_touching_the_text() {
    let (mut app, cell) = field_app("ab");
    app.on_key_down(Key::End, Modifiers::default());
    let before = app.ime_cursor_area().expect("text field wants ime");
    assert!(app.on_ime_preedit("にほん", Some((9, 9))));
    assert_eq!(*cell.borrow(), "ab", "a composition is not text yet");
    assert_eq!(
        field(&app).preedit().map(|p| p.text.as_str()),
        Some("にほん")
    );
    let during = app.ime_cursor_area().unwrap();
    assert!(
        during.x > before.x + 10.0,
        "the caret sits after the composition: {} vs {}",
        during.x,
        before.x
    );
}

#[test]
fn preedit_is_replaced_on_commit() {
    let (mut app, cell) = field_app("ab");
    app.on_key_down(Key::End, Modifiers::default());
    app.on_ime_preedit("に", Some((3, 3)));
    app.on_ime_preedit("にほん", Some((9, 9)));
    assert_eq!(field(&app).preedit().unwrap().text, "にほん");
    app.on_text_input("日本");
    assert_eq!(*cell.borrow(), "ab日本");
    assert!(field(&app).preedit().is_none());
}

#[test]
fn empty_preedit_cancels_the_composition() {
    let (mut app, cell) = field_app("ab");
    app.on_ime_preedit("にほん", None);
    assert!(field(&app).preedit().is_some());
    app.on_ime_preedit("", None);
    assert!(field(&app).preedit().is_none());
    assert_eq!(*cell.borrow(), "ab");
}

#[test]
fn losing_focus_drops_the_composition() {
    let (mut app, _cell) = field_app("ab");
    app.on_ime_preedit("にほん", None);
    app.root_mut().on_event(&Event::FocusLost);
    assert!(field(&app).preedit().is_none());
}

#[test]
fn a_read_only_field_ignores_text_input() {
    let cell = Rc::new(RefCell::new("ro".to_string()));
    let ro = TextField::new(font())
        .with_text_cell(Rc::clone(&cell))
        .with_read_only(true);
    let mut app = App::new(Box::new(ro));
    app.layout(Size::new(400.0, VIEW_H));
    focus_click(&mut app);
    assert!(!app.wants_ime());
    assert!(app.ime_cursor_area().is_none());
    assert!(!app.on_text_input("x"));
    assert!(!app.on_ime_preedit("x", None));
    assert_eq!(*cell.borrow(), "ro");
}

#[test]
fn ime_is_wanted_only_while_a_text_widget_has_focus() {
    let mut app = App::new(Box::new(Button::new("Go", font())));
    app.layout(Size::new(400.0, VIEW_H));
    assert!(!app.wants_ime());
    assert!(app.ime_cursor_area().is_none());
    assert!(
        !app.on_text_input("x"),
        "no focused widget, nobody consumes it"
    );
    let (app, _) = field_app("");
    assert!(app.wants_ime());
    let area = app.ime_cursor_area().unwrap();
    assert!(area.x >= 0.0 && area.y >= 0.0 && area.y <= VIEW_H);
}

#[test]
fn forwarder_events_reach_the_focused_field() {
    let (mut app, cell) = field_app("");
    let mut input = InputForwarder::new();
    input.simulated(
        &mut app,
        ForwarderEvent::ImePreedit {
            text: "か".into(),
            cursor: Some((3, 3)),
        },
    );
    assert_eq!(field(&app).preedit().unwrap().text, "か");
    input.simulated(&mut app, ForwarderEvent::TextInput { text: "漢".into() });
    assert_eq!(*cell.borrow(), "漢");
}

#[test]
fn committed_text_and_preedit_reach_a_text_area() {
    let (mut app, state) = area_app("ab");
    app.on_key_down(Key::End, Modifiers::default());
    app.on_ime_preedit("にほん", Some((9, 9)));
    assert_eq!(
        state.borrow().text,
        "abにほん",
        "shown inline in the buffer"
    );
    app.on_ime_preedit("にほ", Some((6, 6)));
    assert_eq!(state.borrow().text, "abにほ");
    app.on_text_input("日本");
    assert_eq!(state.borrow().text, "ab日本");
    // The commit is one undo step back to the text before the composition.
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    app.on_key_down(Key::Char('z'), ctrl);
    assert_eq!(state.borrow().text, "ab");
}

#[test]
fn a_cancelled_text_area_composition_restores_the_selection() {
    let (mut app, state) = area_app("hello");
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    app.on_key_down(Key::Char('a'), ctrl);
    app.on_ime_preedit("に", Some((3, 3)));
    assert_eq!(
        state.borrow().text,
        "に",
        "the selection is replaced while composing"
    );
    app.on_ime_preedit("", None);
    assert_eq!(state.borrow().text, "hello");
    let st = state.borrow();
    assert_eq!((st.anchor, st.cursor), (0, 5), "selection is back");
}

#[test]
fn a_committed_composition_replaces_the_text_area_selection() {
    let (mut app, state) = area_app("hello");
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    app.on_key_down(Key::Char('a'), ctrl);
    app.on_ime_preedit("に", Some((3, 3)));
    app.on_text_input("日");
    assert_eq!(state.borrow().text, "日");
}

#[test]
fn function_keys_reach_the_app_as_named_keys() {
    let seen: Rc<RefCell<Vec<Key>>> = Rc::new(RefCell::new(Vec::new()));
    let mut app = App::new(Box::new(Button::new("Go", font())));
    let sink = Rc::clone(&seen);
    app.set_global_key_handler(move |key, _| {
        sink.borrow_mut().push(key);
        true
    });
    app.layout(Size::new(400.0, VIEW_H));
    app.on_key_down(Key::F(1), Modifiers::default());
    app.on_key_down(Key::F(5), Modifiers::default());
    assert_eq!(*seen.borrow(), vec![Key::F(1), Key::F(5)]);
}

#[test]
fn function_key_names_parse_f1_to_f24_only() {
    assert_eq!(Key::function_key_from_name("F1"), Some(Key::F(1)));
    assert_eq!(Key::function_key_from_name("F12"), Some(Key::F(12)));
    assert_eq!(Key::function_key_from_name("F24"), Some(Key::F(24)));
    for bad in ["F0", "F25", "F", "f5", "F05", "Fn", "F1x", "F-1", ""] {
        assert_eq!(Key::function_key_from_name(bad), None, "{bad:?}");
    }
}
