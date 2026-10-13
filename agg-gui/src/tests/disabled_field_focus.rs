//! A `TextField` or `TextArea` that its owner's enabled cell disables while it
//! is focused leaves the app's focus too: `App` drops a focused widget that can
//! no longer take focus, so the focus path stops pointing at it, the widget
//! gets `FocusLost` (an edit in progress is completed), and the web shell stops
//! treating the focus as text input.

use crate::{App, Font, Size, TextArea, TextField, Widget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");
const SIZE: Size = Size {
    width: 200.0,
    height: 60.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

fn focused_app(root: Box<dyn Widget>) -> App {
    let mut app = App::new(root);
    app.layout(SIZE);
    app.focus_first();
    assert!(app.has_focus(), "the widget takes focus");
    assert!(app.focused_is_text_input());
    app
}

#[test]
fn rust_only_a_cell_that_disables_the_focused_field_drops_the_app_focus() {
    let cell = Rc::new(Cell::new(true));
    let mut app = focused_app(Box::new(
        TextField::new(font()).with_enabled_cell(Rc::clone(&cell)),
    ));
    cell.set(false);
    app.layout(SIZE);
    assert!(!app.has_focus(), "the focus path no longer names the field");
    assert!(!app.focused_is_text_input());
}

#[test]
fn rust_only_a_cell_that_disables_the_focused_area_drops_the_app_focus() {
    let cell = Rc::new(Cell::new(true));
    let mut app = focused_app(Box::new(
        TextArea::new(font()).with_enabled_cell(Rc::clone(&cell)),
    ));
    cell.set(false);
    app.layout(SIZE);
    assert!(!app.has_focus(), "the focus path no longer names the area");
    assert!(!app.focused_is_text_input());
}

#[test]
fn rust_only_disabling_a_focused_field_completes_its_edit_once() {
    let cell = Rc::new(Cell::new(true));
    let done: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = Rc::clone(&done);
    let mut app = focused_app(Box::new(
        TextField::new(font())
            .with_enabled_cell(Rc::clone(&cell))
            .on_edit_complete(move |t| sink.borrow_mut().push(t.to_string())),
    ));
    app.on_text_input("hi");
    assert!(done.borrow().is_empty(), "still editing");
    cell.set(false);
    app.layout(SIZE);
    app.layout(SIZE);
    assert_eq!(*done.borrow(), ["hi"]);
}

#[test]
fn rust_only_a_focused_field_that_stays_enabled_keeps_the_app_focus() {
    let cell = Rc::new(Cell::new(true));
    let mut app = focused_app(Box::new(
        TextField::new(font()).with_enabled_cell(Rc::clone(&cell)),
    ));
    app.layout(SIZE);
    assert!(app.has_focus());
}
