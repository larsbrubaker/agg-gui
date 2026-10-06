//! `TextField::select_all_on_focus` through the real [`App`] pointer and
//! keyboard routing.
//!
//! Ports agg-sharp `InternalTextEditWidget`'s `selectAllOnMouseUpIfNoSelection`:
//! the click that focuses the field selects all of its text on release (so
//! typing replaces it), unless that press drags out a range, which is kept.
//! Clicks once the field has focus position the caret, and Tab focus selects
//! all immediately. The App sends `FocusGained` before the focusing
//! `MouseDown`, which is exactly the ordering these tests drive.

use super::*;
use crate::text::{measure_advance, Font};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const VIEW_H: f64 = 40.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// An App whose root is one bound `TextField` holding `text`.
fn field_app(text: &str, select_all_on_focus: bool) -> (App, Rc<RefCell<String>>) {
    let cell = Rc::new(RefCell::new(text.to_string()));
    let field = TextField::new(font())
        .with_text_cell(Rc::clone(&cell))
        .with_select_all_on_focus(select_all_on_focus);
    let mut app = App::new(Box::new(field));
    app.layout(Size::new(400.0, VIEW_H));
    (app, cell)
}

/// Screen x of the left edge of byte `off` (padding 8, font size 14, no scroll).
fn x_at(text: &str, off: usize) -> f64 {
    8.0 + measure_advance(&font(), &text[..off], 14.0)
}

fn down(app: &mut App, x: f64) {
    app.on_mouse_down(x, VIEW_H * 0.5, MouseButton::Left, Modifiers::default());
}

fn up(app: &mut App, x: f64) {
    app.on_mouse_up(x, VIEW_H * 0.5, MouseButton::Left, Modifiers::default());
}

fn click(app: &mut App, x: f64) {
    down(app, x);
    up(app, x);
}

fn type_str(app: &mut App, s: &str) {
    for c in s.chars() {
        app.on_key_down(Key::Char(c), Modifiers::default());
    }
}

/// The reported bug: clicking into "20" and typing "30" produced "2030".
#[test]
fn focusing_click_then_typing_replaces_text() {
    let (mut app, cell) = field_app("20", true);
    click(&mut app, x_at("20", 1));
    type_str(&mut app, "30");
    assert_eq!(cell.borrow().as_str(), "30");
}

/// A drag during the focusing click selects the dragged range, not everything.
#[test]
fn focusing_click_drag_selects_dragged_range() {
    let text = "12345";
    let (mut app, cell) = field_app(text, true);
    down(&mut app, x_at(text, 2));
    app.on_mouse_move(x_at(text, 4), VIEW_H * 0.5);
    up(&mut app, x_at(text, 4));
    type_str(&mut app, "x");
    assert_eq!(cell.borrow().as_str(), "12x5");
}

/// Once the field has focus, a click positions the caret as usual.
#[test]
fn second_click_positions_caret() {
    let text = "12345";
    let (mut app, cell) = field_app(text, true);
    click(&mut app, x_at(text, 1));
    // Far enough apart in x that the multi-click tracker sees a new single
    // click rather than a double-click.
    let (a, b) = (x_at(text, 1), x_at(text, 4));
    assert!(b - a > 10.0, "test clicks must be far apart");
    click(&mut app, b);
    type_str(&mut app, "x");
    assert_eq!(cell.borrow().as_str(), "1234x5");
}

/// Keyboard focus selects all at once, with no click involved.
#[test]
fn tab_focus_selects_all() {
    let (mut app, cell) = field_app("20", true);
    app.on_key_down(Key::Tab, Modifiers::default());
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));
    type_str(&mut app, "30");
    assert_eq!(cell.borrow().as_str(), "30");
}

/// After Tab focus, a click positions the caret (it is not the focusing click).
#[test]
fn click_after_tab_focus_positions_caret() {
    let text = "12345";
    let (mut app, cell) = field_app(text, true);
    app.on_key_down(Key::Tab, Modifiers::default());
    click(&mut app, x_at(text, 2));
    type_str(&mut app, "x");
    assert_eq!(cell.borrow().as_str(), "12x345");
}

/// Without `select_all_on_focus` the focusing click just places the caret.
#[test]
fn focusing_click_without_select_all_places_caret() {
    let text = "12345";
    let (mut app, cell) = field_app(text, false);
    click(&mut app, x_at(text, 3));
    type_str(&mut app, "x");
    assert_eq!(cell.borrow().as_str(), "123x45");
}
