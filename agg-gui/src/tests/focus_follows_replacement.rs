//! Keyboard focus follows the focused widget's replacement, and the
//! replacement is told it has focus.
//!
//! `App` keeps focus as a child-index path that follows the focused widget
//! when its parent reorders, re-lays out or re-parents it
//! (`widget/app/path_anchor.rs`). Apps also rebuild a focused subtree in
//! place (a list re-created after a selection change) and rely on keys still
//! reaching the widget at the focused place. That replacement must then be
//! sent `FocusGained` (so a text field shows its caret and handles typing)
//! and the keyboard layer re-told which widget has focus (so the on-screen
//! keyboard stays up for a new text field and lowers for anything else);
//! `widget/app/focus_change.rs`. The widget-with-a-`WidgetBase` case must
//! hold even when the replacement reuses the old widget's heap address.

use crate::widgets::on_screen_keyboard::{self, state::with_state_ref, test_hook};
use crate::{
    App, Button, DrawCtx, Event, EventResult, Font, Key, Label, Modifiers, MouseButton, Rect, Size,
    TextField, Widget,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use super::TEST_FONT;

const SIZE: Size = Size {
    width: 400.0,
    height: 40.0,
};

type Text = Rc<RefCell<String>>;

/// A container laying its children out side by side, counting every
/// `FocusGained` dispatched through it (an ancestor previews each event on
/// its way down to the focused widget).
struct Row {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    gains: Rc<Cell<usize>>,
}

impl Widget for Row {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let w = available.width / self.children.len().max(1) as f64;
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(w, available.height));
            child.set_bounds(Rect::new(i as f64 * w, 0.0, w, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn preview_event(&mut self, event: &Event) -> EventResult {
        if matches!(event, Event::FocusGained) {
            self.gains.set(self.gains.get() + 1);
        }
        EventResult::Ignored
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn row(children: Vec<Box<dyn Widget>>, gains: &Rc<Cell<usize>>) -> Box<dyn Widget> {
    Box::new(Row {
        bounds: Rect::default(),
        children,
        gains: Rc::clone(gains),
    })
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("font"))
}

fn text() -> Text {
    Rc::new(RefCell::new(String::new()))
}

fn field(cell: &Text) -> Box<dyn Widget> {
    Box::new(TextField::new(font()).with_text_cell(Rc::clone(cell)))
}

fn click(app: &mut App, x: f64) {
    let y = SIZE.height * 0.5;
    app.on_mouse_down(x, y, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(x, y, MouseButton::Left, Modifiers::default());
}

fn type_text(app: &mut App, s: &str) {
    for c in s.chars() {
        app.on_key_down(Key::Char(c), Modifiers::default());
    }
}

/// The on-screen keyboard state the shells read: (a text input is focused,
/// the slide is heading up).
fn keyboard_up() -> (bool, bool) {
    with_state_ref(|s| (s.text_input_focused, s.slide.target() > 0.5))
}

/// Enables the on-screen keyboard for one test and resets it afterwards,
/// also when an assertion fails (its state is per thread).
struct KeyboardOn;

impl KeyboardOn {
    fn new() -> Self {
        test_hook::reset();
        on_screen_keyboard::set_enabled(true);
        KeyboardOn
    }
}

impl Drop for KeyboardOn {
    fn drop(&mut self) {
        test_hook::reset();
        crate::widget::keyboard_scroll::reset_lift_for_test();
    }
}

/// Two fields side by side under a counting root, the left one focused by a
/// click (one `FocusGained`), with the keyboard up for it.
fn focused_left(left: &Text, right: &Text, gains: &Rc<Cell<usize>>) -> App {
    let mut app = App::new(row(vec![field(left), field(right)], gains));
    app.layout(SIZE);
    click(&mut app, 50.0);
    assert_eq!(app.focused_path(), Some(&[0usize][..]));
    assert_eq!(gains.get(), 1);
    assert_eq!(keyboard_up(), (true, true), "focusing the field raises it");
    app
}

/// The replacement holds focus, was told so exactly once, takes typed keys
/// (a `TextField` handles keys only while it believes it is focused), and
/// the keyboard stays up for it.
fn assert_replacement_focused(app: &mut App, gains: &Rc<Cell<usize>>, fresh: &Text, old: &Text) {
    assert_eq!(app.focused_path(), Some(&[0usize][..]));
    assert_eq!(gains.get(), 2, "the replacement gets FocusGained once");
    assert!(app.focused_is_text_input());
    assert_eq!(keyboard_up(), (true, true), "a new text field keeps it up");
    type_text(app, "xy");
    assert_eq!(fresh.borrow().as_str(), "xy");
    assert_eq!(old.borrow().as_str(), "a");
    app.layout(SIZE);
    assert_eq!(gains.get(), 2, "no second FocusGained on the next frame");
}

#[test]
fn replaced_focused_field_is_told_it_has_focus() {
    let _kb = KeyboardOn::new();
    let (left, right, fresh, gains) = (text(), text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);
    type_text(&mut app, "a");

    app.root_mut().children_mut()[0] = field(&fresh);
    app.layout(SIZE);

    assert_replacement_focused(&mut app, &gains, &fresh, &left);
}

/// The old field is dropped before the new one is built, so the allocator
/// is free to hand the new field the old one's address: the replacement
/// must still be told.
#[test]
fn replacement_at_reused_address_is_told_it_has_focus() {
    let _kb = KeyboardOn::new();
    let (left, right, fresh, gains) = (text(), text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);
    type_text(&mut app, "a");

    drop(app.root_mut().children_mut().remove(0));
    let replacement = field(&fresh);
    app.root_mut().children_mut().insert(0, replacement);
    app.layout(SIZE);

    assert_replacement_focused(&mut app, &gains, &fresh, &left);
}

/// A key arriving between the rebuild and the next layout reaches the
/// replacement, which is told it has focus first.
#[test]
fn key_before_next_layout_reaches_told_replacement() {
    let _kb = KeyboardOn::new();
    let (left, right, fresh, gains) = (text(), text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);
    type_text(&mut app, "a");

    app.root_mut().children_mut()[0] = field(&fresh);
    type_text(&mut app, "z");

    assert_eq!(fresh.borrow().as_str(), "z", "the first key reaches it");
    assert_eq!(gains.get(), 2, "told before the key arrived");
    assert_eq!(keyboard_up(), (true, true));
    fresh.borrow_mut().clear();
    app.layout(SIZE);
    assert_replacement_focused(&mut app, &gains, &fresh, &left);
}

/// A focused field replaced by a focusable button: focus follows to the
/// button (it is told), and the keyboard lowers because a button takes no
/// typed text.
#[test]
fn field_replaced_by_button_moves_focus_and_lowers_keyboard() {
    let _kb = KeyboardOn::new();
    let (left, right, gains) = (text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);

    app.root_mut().children_mut()[0] = Box::new(Button::new("Go", font()));
    app.layout(SIZE);

    assert_eq!(app.focused_path(), Some(&[0usize][..]));
    assert_eq!(gains.get(), 2, "the button gets FocusGained");
    assert!(!app.focused_is_text_input());
    assert_eq!(keyboard_up(), (false, false));
}

/// A focused field replaced by something that cannot take focus: focus
/// clears, as a click on that widget would clear it, and the keyboard
/// lowers.
#[test]
fn field_replaced_by_unfocusable_widget_clears_focus() {
    let _kb = KeyboardOn::new();
    let (left, right, gains) = (text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);

    app.root_mut().children_mut()[0] = Box::new(Label::new("Done", font()));
    app.layout(SIZE);

    assert!(!app.has_focus(), "focus names {:?}", app.focused_path());
    assert_eq!(gains.get(), 1);
    assert_eq!(keyboard_up(), (false, false));
    type_text(&mut app, "x");
    assert_eq!(right.borrow().as_str(), "");
}

/// The focused field removed with nothing left at its place: focus clears
/// and the keyboard lowers.
#[test]
fn removed_focused_field_clears_focus_and_lowers_keyboard() {
    let _kb = KeyboardOn::new();
    let (only, gains) = (text(), Rc::default());
    let mut app = App::new(row(vec![field(&only)], &gains));
    app.layout(SIZE);
    click(&mut app, 50.0);
    assert_eq!(keyboard_up(), (true, true));

    app.root_mut().children_mut().clear();
    app.layout(SIZE);

    assert!(!app.has_focus());
    assert!(!app.focused_is_text_input());
    assert_eq!(keyboard_up(), (false, false));
}

/// The focused field removed while a sibling slides into its index: the
/// sibling now at the focused place takes focus, and is told so.
#[test]
fn removed_focused_field_hands_focus_to_field_now_in_its_place() {
    let _kb = KeyboardOn::new();
    let (left, right, gains) = (text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);

    app.root_mut().children_mut().remove(0);
    app.layout(SIZE);

    assert_eq!(app.focused_path(), Some(&[0usize][..]));
    assert_eq!(gains.get(), 2);
    assert_eq!(keyboard_up(), (true, true));
    type_text(&mut app, "x");
    assert_eq!(right.borrow().as_str(), "x");
}

#[test]
fn relaid_out_field_keeps_focus_without_second_gain() {
    let _kb = KeyboardOn::new();
    let (left, right, gains) = (text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);

    app.layout(SIZE);
    app.layout(Size::new(600.0, 60.0));
    app.layout(SIZE);

    assert_eq!(app.focused_path(), Some(&[0usize][..]));
    assert_eq!(gains.get(), 1);
    assert_eq!(keyboard_up(), (true, true));
    type_text(&mut app, "ab");
    assert_eq!(left.borrow().as_str(), "ab");
}

#[test]
fn reordered_field_keeps_focus_without_second_gain() {
    let _kb = KeyboardOn::new();
    let (left, right, gains) = (text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);

    app.root_mut().children_mut().swap(0, 1);
    app.layout(SIZE);

    assert_eq!(app.focused_path(), Some(&[1usize][..]));
    assert_eq!(gains.get(), 1);
    assert_eq!(keyboard_up(), (true, true));
    type_text(&mut app, "ab");
    assert_eq!(left.borrow().as_str(), "ab");
    assert_eq!(right.borrow().as_str(), "");
}

/// The same field moved under a new parent (wrapped in a fresh container)
/// is still the focused widget.
#[test]
fn reparented_field_keeps_focus_without_second_gain() {
    let _kb = KeyboardOn::new();
    let (left, right, gains) = (text(), text(), Rc::default());
    let mut app = focused_left(&left, &right, &gains);

    let moved = app.root_mut().children_mut().remove(0);
    let wrapper = row(vec![moved], &Rc::default());
    app.root_mut().children_mut().insert(0, wrapper);
    app.layout(SIZE);

    assert_eq!(app.focused_path(), Some(&[0usize, 0][..]));
    assert_eq!(gains.get(), 1);
    assert_eq!(keyboard_up(), (true, true));
    type_text(&mut app, "ab");
    assert_eq!(left.borrow().as_str(), "ab");
}
