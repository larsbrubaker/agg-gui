//! Click-to-focus opt-out — `Button::with_focus_on_click(false)` (stored on
//! `WidgetBase::focus_on_click`, read by the `App`'s click-to-focus rule in
//! `widget/app/pointer.rs`).
//!
//! An opted-out button still fires on click but doesn't take focus, so a
//! following Space reaches the unconsumed-key path; Tab still focuses it and
//! Space then activates it. Default buttons keep focus-on-click.

use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// A button that counts its clicks.
fn counting_button(label: &str, count: &Rc<Cell<u32>>) -> Button {
    let c = Rc::clone(count);
    Button::new(label, font()).on_click(move || c.set(c.get() + 1))
}

/// An App whose only content is `button` at the top of a column, with a
/// focusable text field (focus id 9) below it and a global key handler
/// recording every key that went unconsumed.
fn app_with(button: Button) -> (App, Rc<RefCell<Vec<Key>>>) {
    let mut col = FlexColumn::new();
    col.push(Box::new(button), 0.0);
    col.push(Box::new(TextField::new(font()).with_focus_id(9)), 0.0);
    let mut app = App::new(Box::new(col));
    let unconsumed = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&unconsumed);
    app.set_global_key_handler(move |key, _| {
        sink.borrow_mut().push(key);
        true
    });
    app.layout(Size::new(300.0, 300.0));
    (app, unconsumed)
}

/// Click near the top-left corner (Y-down screen coordinates), where the
/// column's first child — the button — sits.
fn click_button(app: &mut App) {
    app.on_mouse_down(12.0, 10.0, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(12.0, 10.0, MouseButton::Left, Modifiers::default());
}

fn space(app: &mut App) {
    app.on_key_down(Key::Char(' '), Modifiers::default());
}

/// Default buttons are unchanged: a click focuses the button and a following
/// Space re-clicks it instead of going unconsumed.
#[test]
fn default_button_takes_focus_on_click() {
    let hits = Rc::new(Cell::new(0));
    let (mut app, unconsumed) = app_with(counting_button("Go", &hits));
    click_button(&mut app);
    assert_eq!(hits.get(), 1, "the click activates the button");
    assert_eq!(app.focused_widget_type_name(), Some("Button"));

    space(&mut app);
    assert_eq!(hits.get(), 2, "Space activates the focused button");
    assert!(unconsumed.borrow().is_empty(), "the button consumed Space");
}

/// An opted-out button fires on click but stays unfocused, so Space goes to
/// the unconsumed-key handling.
#[test]
fn opted_out_button_click_does_not_take_focus() {
    let hits = Rc::new(Cell::new(0));
    let (mut app, unconsumed) = app_with(counting_button("Go", &hits).with_focus_on_click(false));
    click_button(&mut app);
    assert_eq!(hits.get(), 1, "the click still activates the button");
    assert!(
        app.focused_widget_type_name().is_none(),
        "the button did not take focus"
    );

    space(&mut app);
    assert_eq!(hits.get(), 1, "Space does not re-click the button");
    assert_eq!(*unconsumed.borrow(), vec![Key::Char(' ')]);
}

/// Clicking an opted-out button behaves like clicking any non-focusable
/// widget: a text field that had focus loses it, so Space doesn't type
/// into the field either.
#[test]
fn opted_out_button_click_clears_previous_focus() {
    let hits = Rc::new(Cell::new(0));
    let (mut app, unconsumed) = app_with(counting_button("Go", &hits).with_focus_on_click(false));
    crate::focus::request_focus(9);
    app.layout(Size::new(300.0, 300.0));
    assert_eq!(app.focused_widget_type_name(), Some("TextField"));

    click_button(&mut app);
    assert_eq!(hits.get(), 1);
    assert!(app.focused_widget_type_name().is_none());
    space(&mut app);
    assert_eq!(*unconsumed.borrow(), vec![Key::Char(' ')]);
}

/// Keyboard navigation still works: Tab focuses an opted-out button and
/// Space then activates it.
#[test]
fn opted_out_button_still_focuses_with_tab() {
    let hits = Rc::new(Cell::new(0));
    let (mut app, unconsumed) = app_with(counting_button("Go", &hits).with_focus_on_click(false));
    app.on_key_down(Key::Tab, Modifiers::default());
    assert_eq!(app.focused_widget_type_name(), Some("Button"));

    space(&mut app);
    assert_eq!(hits.get(), 1, "Space activates the Tab-focused button");
    assert!(unconsumed.borrow().is_empty(), "the button consumed Space");
}
