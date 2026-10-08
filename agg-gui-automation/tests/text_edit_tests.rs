//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/TextEditTests.cs`.
//!
//! C#'s single-line `TextEditWidget(text, x, y, pixelWidth: w)` is an
//! agg-gui [`TextField`] at C#'s 12 point size with no inset (C#'s edit
//! widget draws its text from its own left edge), `w` wide at its own
//! height, its lower-left corner at `(x, y)`. C#'s `container` is a
//! [`HeadlessWindow`] the container's size; `container.OnMouseDown(...)`
//! and friends take the same Y-up window points. C#'s `SendKey(Keys.A, 'a')`
//! (a KeyDown followed by a KeyPress) is one `Key::Char('a')` key down,
//! which is how agg-gui delivers a typed character; `SendKeyDown(Keys.X)`
//! for a non-character key is that key's key down. `Keyboard.SetKeyDownState
//! (Keys.Shift, ...)` is the window's held modifiers.
//!
//! C#'s `ContainsFocus` is the `App`'s focused path starting at the field.
//! C#'s `Focused` on the outer `TextEditWidget` is false whenever its inner
//! `InternalTextEditWidget` holds focus — an artifact of C#'s two-widget
//! edit control; an agg-gui `TextField` is one widget, so the assertions of
//! C#'s `Focused == false` while `ContainsFocus` is true have nothing to
//! test and are noted where they occur, and `Focused == false` after the
//! focus moved away is asserted as `!ContainsFocus` (stronger).
//!
//! The tests of the multi-line editor and the special keys live in
//! `text_edit/`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::event::{Event, Key, Modifiers};
use agg_gui::widgets::{TextArea, TextField};
use agg_gui::{observe_events, MouseButton, Rect, Size, Widget, WidgetId};
use agg_gui_automation::{HeadlessWindow, UiDriver, WidgetHandle};

#[path = "text_edit/multi_line.rs"]
mod multi_line;
#[path = "text_edit/special_keys.rs"]
mod special_keys;

/// C#'s default `TextEditWidget` point size.
const POINT_SIZE: f64 = 12.0;

/// C# `new TextEditWidget(text, x, y, pixelWidth: width) { Name = name }`.
fn text_edit_widget(name: &str, text: &str, x: f64, y: f64, width: f64) -> TextField {
    let mut field = TextField::new(Arc::new(agg_gui::fonts::standard_ui_font()))
        .with_font_size(POINT_SIZE)
        .with_padding(0.0)
        .with_text(text)
        .with_name(name);
    let height = field.layout(Size::new(width, 1000.0)).height;
    field.set_bounds(Rect::new(x, y, width, height));
    field
}

/// C#'s `container` (`LocalBounds = (0, 0, width, height)`) holding `field`.
fn container_with(width: f64, height: f64, fields: Vec<TextField>) -> HeadlessWindow {
    let mut container = HeadlessWindow::new(width, height);
    for field in fields {
        container.add_child(Box::new(field));
    }
    container
}

fn handle(container: &HeadlessWindow, name: &str) -> WidgetHandle {
    container
        .handle(name)
        .expect("the edit field is in the container")
}

fn field<'a>(container: &'a HeadlessWindow, name: &str) -> &'a TextField {
    handle(container, name)
        .downcast::<TextField>(container.root())
        .expect("the edit field is a TextField")
}

fn field_mut<'a>(container: &'a mut HeadlessWindow, name: &str) -> &'a mut TextField {
    let handle = handle(container, name);
    handle
        .widget_mut(container.driver_mut().root_mut())
        .and_then(|widget| widget.as_any_mut())
        .and_then(|any| any.downcast_mut::<TextField>())
        .expect("the edit field is a TextField")
}

/// C# `editField.Text`.
fn text(container: &HeadlessWindow, name: &str) -> String {
    field(container, name).text()
}

/// C# `editField.Text = text`.
fn set_text(container: &mut HeadlessWindow, name: &str, text: &str) {
    field_mut(container, name).set_text(text);
}

/// C# `editField.Selection`.
fn selection(container: &HeadlessWindow, name: &str) -> String {
    field(container, name).selection()
}

/// C# `editField.CharIndexToInsertBefore` (the texts here are ASCII, so a
/// byte offset is a character index).
fn char_index_to_insert_before(container: &HeadlessWindow, name: &str) -> usize {
    field(container, name).cursor_pos()
}

/// C# `editField.ContainsFocus`.
fn contains_focus(container: &HeadlessWindow, name: &str) -> bool {
    let app = container.driver().app();
    match (
        handle(container, name).resolve(app.root()),
        app.focused_path(),
    ) {
        (Some(path), Some(focus)) => focus.starts_with(&path),
        _ => false,
    }
}

/// C# `SendKey(key, ch, container)` for a typed character.
fn send_char(container: &mut HeadlessWindow, ch: char) {
    container.on_key_down(Key::Char(ch), Modifiers::default());
}

/// C# `SendKey`/`SendKeyDown` for a non-character key with modifiers.
fn send_key(container: &mut HeadlessWindow, key: Key, modifiers: Modifiers) {
    container.on_key_down(key, modifiers);
}

fn control() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

fn down(container: &mut HeadlessWindow, clicks: u32, x: f64, y: f64) {
    container.on_mouse_down(x, y, MouseButton::Left, clicks);
}

fn up(container: &mut HeadlessWindow, x: f64, y: f64) {
    container.on_mouse_up(x, y, MouseButton::Left);
}

#[test]
fn corect_line_counts() {
    // C#'s `TypeFacePrinter(text).NumLines()` is the line count an agg-gui
    // `TextArea` lays the text out in, wide enough that nothing wraps.
    let num_lines = |text: &str| {
        let mut printer = TextArea::new(Arc::new(agg_gui::fonts::standard_ui_font()))
            .with_font_size(POINT_SIZE)
            .with_text(text);
        printer.layout(Size::new(10_000.0, 10_000.0));
        printer.visual_line_count()
    };

    let lines7 = "; activate T0
; move up a bit
G91
G1 Z1 F1500
G90
; do the switch to T0
G1 X-29.5 F6000 ; NO_PROCESSING";
    assert_eq!(num_lines(lines7), 7);

    let lines8 = "; activate T0
; move up a bit
G91
G1 Z1 F1500
G90
; do the switch to T0
G1 X-29.5 F6000 ; NO_PROCESSING
";
    assert_eq!(num_lines(lines8), 8);
}

#[test]
fn text_edit_text_selection_tests() {
    let edit = "editField1";
    let mut container = container_with(
        200.0,
        200.0,
        vec![text_edit_widget(edit, "", 0.0, 0.0, 51.0)],
    );
    let c = &mut container;

    // select the control and type something in it
    down(c, 1, 0.0, 0.0);
    up(c, 0.0, 0.0);
    send_char(c, 'a');
    assert!(text(c, edit) == "a");

    // select the beginning again and type something else in it
    down(c, 1, 0.0, 0.0);
    up(c, 0.0, 0.0);
    send_char(c, 'b');
    assert!(text(c, edit) == "ba");

    // select the ba and delete them
    down(c, 1, 0.0, 0.0);
    c.on_mouse_move(15.0, 0.0);
    up(c, 15.0, 0.0);
    send_key(c, Key::Backspace, Modifiers::default());
    assert!(text(c, edit).is_empty());

    // select the other way
    set_text(c, edit, "ab");
    assert!(text(c, edit) == "ab");
    down(c, 1, 15.0, 0.0);
    c.on_mouse_move(0.0, 0.0);
    up(c, 0.0, 0.0);
    send_key(c, Key::Backspace, Modifiers::default());
    assert!(text(c, edit).is_empty());

    // select the other way but start far to the right
    set_text(c, edit, "abc");
    assert!(text(c, edit) == "abc");
    down(c, 1, 30.0, 0.0);
    c.on_mouse_move(0.0, 0.0);
    up(c, 0.0, 0.0);
    send_key(c, Key::Backspace, Modifiers::default());
    assert!(text(c, edit).is_empty());

    // double click empty does nothing
    // select the other way but start far to the right
    set_text(c, edit, "");
    down(c, 1, 1.0, 0.0);
    up(c, 0.0, 0.0);
    down(c, 2, 1.0, 0.0);
    up(c, 0.0, 0.0);
    assert_eq!(selection(c, edit), ""); //, "First word selected");

    // double click first word selects
    set_text(c, edit, "abc 123");
    down(c, 1, 1.0, 0.0);
    up(c, 0.0, 0.0);
    down(c, 2, 1.0, 0.0);
    up(c, 0.0, 0.0);
    assert_eq!(selection(c, edit), "abc"); //, "First word selected");

    // double click last word selects
    set_text(c, edit, "abc 123");
    down(c, 1, 30.0, 0.0);
    up(c, 0.0, 0.0);
    down(c, 2, 30.0, 0.0);
    up(c, 0.0, 0.0);
    assert_eq!(selection(c, edit), "123"); //, "Second word selected");
}

#[test]
fn text_selection_with_shift_click() {
    // C#'s WindowsKeyBindings scope: agg-gui's Control+Arrow moves by word
    // on every platform unless the Mac bindings are asked for.
    const FULL_TEXT: &str = "This is a text";
    let edit = "editField1";
    let mut container = container_with(
        200.0,
        200.0,
        vec![text_edit_widget(edit, FULL_TEXT, 0.0, 0.0, 100.0)],
    );
    let c = &mut container;
    let shift_held = |c: &mut HeadlessWindow, held: bool| {
        c.set_modifiers(if held { shift() } else { Modifiers::default() });
    };

    // select all from left to right with shift click
    down(c, 1, 1.0, 0.0);
    up(c, 1.0, 0.0);
    assert_eq!(char_index_to_insert_before(c, edit), 0);
    assert_eq!(selection(c, edit), "");
    shift_held(c, true);
    down(c, 1, 100.0, 0.0);
    up(c, 100.0, 0.0);
    shift_held(c, false);
    assert_eq!(char_index_to_insert_before(c, edit), FULL_TEXT.len());
    assert_eq!(selection(c, edit), FULL_TEXT); //, "It should select full text");

    // select all from right to left with shift click
    down(c, 1, 100.0, 0.0);
    up(c, 100.0, 0.0);
    assert_eq!(char_index_to_insert_before(c, edit), FULL_TEXT.len());
    assert_eq!(selection(c, edit), "");
    shift_held(c, true);
    down(c, 1, 1.0, 0.0);
    up(c, 1.0, 0.0);
    shift_held(c, false);
    assert_eq!(char_index_to_insert_before(c, edit), 0);
    assert_eq!(selection(c, edit), FULL_TEXT); //, "It should select full text");

    // select parts of the text with shift click
    down(c, 1, 1.0, 0.0);
    up(c, 1.0, 0.0);
    send_key(c, Key::ArrowRight, control());
    send_key(c, Key::ArrowRight, control());
    assert_eq!(char_index_to_insert_before(c, edit), "This is ".len());
    assert_eq!(selection(c, edit), "");
    shift_held(c, true);
    down(c, 1, 100.0, 0.0);
    up(c, 100.0, 0.0);
    shift_held(c, false);
    assert_eq!(char_index_to_insert_before(c, edit), FULL_TEXT.len());
    assert_eq!(selection(c, edit), "a text"); //, "It should select second part of the text");
    shift_held(c, true);
    down(c, 1, 1.0, 0.0);
    up(c, 1.0, 0.0);
    shift_held(c, false);
    assert_eq!(char_index_to_insert_before(c, edit), 0);
    assert_eq!(selection(c, edit), "This is "); //, "It should select first part of the text");
}

#[test]
fn text_changed_events_tests() {
    let (edit1, edit2) = ("editField1", "editField2");
    let field1 = text_edit_widget(edit1, "", 0.0, 0.0, 20.0);
    assert!(field1.bounds().top() < 40.0);
    let text_field1_edit_complete = Rc::new(Cell::new(false));
    let edit_complete = Rc::clone(&text_field1_edit_complete);
    let field1 = field1.on_edit_complete(move |_| edit_complete.set(true));
    let field2 = text_edit_widget(edit2, "", 0.0, 40.0, 20.0);
    let mut container = container_with(200.0, 200.0, vec![field1, field2]);

    // C#'s `ContainsFocusChanged`: the field gaining or losing focus.
    let text_field1_lost_focus = Rc::new(Cell::new(false));
    let text_field1_got_focus = Rc::new(Cell::new(false));
    let (got, lost) = (
        Rc::clone(&text_field1_got_focus),
        Rc::clone(&text_field1_lost_focus),
    );
    let field1_id = WidgetId::of(field(&container, edit1) as &dyn Widget);
    let _observer = observe_events(field1_id, move |event| match event {
        Event::FocusGained => got.set(true),
        Event::FocusLost => lost.set(true),
        _ => {}
    });
    let c = &mut container;

    // mouse select on the control when it contains nothing
    down(c, 1, 1.0, 1.0);
    up(c, 1.0, 1.0);
    assert!(text_field1_got_focus.get());
    assert!(!text_field1_edit_complete.get());
    send_char(c, 'b');
    assert!(text(c, edit1) == "b");
    assert!(!text_field1_edit_complete.get());
    send_key(c, Key::Enter, Modifiers::default());
    assert!(text_field1_edit_complete.get());
    text_field1_edit_complete.set(false);
    send_char(c, 'a');
    assert!(text(c, edit1) == "ba");
    assert!(!text_field1_edit_complete.get());

    assert!(!text_field1_lost_focus.get());
    text_field1_got_focus.set(false);
    down(c, 1, 1.0, 41.0);
    up(c, 1.0, 1.0);
    send_char(c, 'e');
    assert!(text_field1_lost_focus.get());
    assert!(text_field1_edit_complete.get());
    assert!(text(c, edit1) == "ba");
    assert!(text(c, edit2) == "e");

    text_field1_edit_complete.set(false);
    text_field1_lost_focus.set(false);
    down(c, 1, 1.0, 1.0);
    up(c, 1.0, 1.0);
    assert!(!text_field1_lost_focus.get());
    assert!(!text_field1_edit_complete.get());
    down(c, 1, 1.0, 41.0);
    up(c, 1.0, 1.0);
    assert!(text_field1_lost_focus.get());
    assert!(!text_field1_edit_complete.get());
}

#[test]
fn text_edit_gets_focus_tests() {
    let (edit1, edit2) = ("editField1", "editField2");
    let mut container = container_with(
        200.0,
        200.0,
        vec![
            text_edit_widget(edit1, "", 0.0, 0.0, 160.0),
            text_edit_widget(edit2, "", 0.0, 20.0, 160.0),
        ],
    );
    let c = &mut container;

    // select no edit field
    assert!(text(c, edit1).is_empty());
    send_char(c, 'a');
    assert!(text(c, edit1).is_empty());
    assert!(text(c, edit2).is_empty());

    // select edit field 1
    // (C#'s `editField1.Focused == false` checks below are the two-widget
    // artifact the module docs describe.)
    c.on_mouse_move(1.0, 1.0); // we move into the widget to make sure we have separate focus and enter events.
    assert!(!contains_focus(c, edit1));
    down(c, 1, 1.0, 1.0);
    assert!(contains_focus(c, edit1));
    up(c, 1.0, 1.0);
    assert!(contains_focus(c, edit1));
    send_char(c, 'b');
    assert!(text(c, edit1) == "b");
    down(c, 1, 150.0, 1.0);
    assert!(contains_focus(c, edit1));
    up(c, 150.0, 1.0);
    assert!(contains_focus(c, edit1));
    send_char(c, 'c');
    assert!(text(c, edit1) == "bc");

    // select edit field 2
    down(c, 1, 1.0, 21.0);
    assert!(contains_focus(c, edit2));
    up(c, 1.0, 21.0);
    send_char(c, 'd');
    assert!(text(c, edit1) == "bc");
    assert!(text(c, edit2) == "d");
}

#[test]
fn add_then_delete_causes_no_visual_change() {
    let edit = "editField1";
    let mut container = container_with(
        200.0,
        200.0,
        vec![text_edit_widget(edit, "Test", 10.0, 10.0, 50.0)],
    );
    let c = &mut container;
    c.draw();
    let before_edit_image = c.driver().current_screen().pixels().to_vec();
    let before_bounds = field(c, edit).bounds();

    down(c, 1, 10.0, 10.0);
    up(c, 10.0, 10.0);
    assert!(contains_focus(c, edit));
    send_char(c, 'b');
    assert!(text(c, edit) == "bTest");
    let after_b_bounds = field(c, edit).bounds();
    assert!(
        before_bounds.bottom() == after_b_bounds.bottom()
            && before_bounds.top() == after_b_bounds.top()
    );

    send_key(c, Key::Backspace, Modifiers::default());
    assert!(text(c, edit) == "Test");

    // C#'s LocalBounds and OriginRelativeParent are both in agg-gui's bounds.
    let after_bounds = field(c, edit).bounds();
    assert!(before_bounds == after_bounds);

    // click off it so the cursor is not in it.
    down(c, 1, 1.0, 1.0);
    up(c, 1.0, 1.0);
    assert!(!contains_focus(c, edit));

    c.draw();
    assert!(c.driver().current_screen().pixels() == before_edit_image.as_slice());
}
