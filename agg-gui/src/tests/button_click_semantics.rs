//! Where a `Button` click fires — the rule agg-sharp's `GuiWidget.OnMouseUp`
//! applies (`WidgetClickTests` in the Agg Automation Tests): a click needs
//! the press *and* the release on the button. The release position decides,
//! not the hover state cached from the last move, because a platform can
//! report a release somewhere the pointer was never seen moving to (a
//! browser `pointerup` after the pointer left the canvas, an automation
//! test calling `App::on_mouse_up` directly).

use super::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::text::Font;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// A 300 × 300 app whose column holds one counting button at its top-left.
fn app_with_button(count: &Rc<Cell<u32>>) -> App {
    let c = Rc::clone(count);
    let button = Button::new("Go", font()).on_click(move || c.set(c.get() + 1));
    let mut col = FlexColumn::new();
    col.push(Box::new(button), 0.0);
    let mut app = App::new(Box::new(col));
    app.layout(Size::new(300.0, 300.0));
    app
}

/// On the button (Y-down screen coordinates).
const ON: (f64, f64) = (12.0, 10.0);
/// Well below the button, on the bare column.
const OFF: (f64, f64) = (250.0, 290.0);

fn down(app: &mut App, at: (f64, f64)) {
    app.on_mouse_down(at.0, at.1, MouseButton::Left, Modifiers::default());
}

fn up(app: &mut App, at: (f64, f64)) {
    app.on_mouse_up(at.0, at.1, MouseButton::Left, Modifiers::default());
}

#[test]
fn rust_only_press_and_release_on_a_button_clicks_it_once() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with_button(&hits);
    app.on_mouse_move(ON.0, ON.1);
    down(&mut app, ON);
    up(&mut app, ON);
    assert_eq!(hits.get(), 1);
}

#[test]
fn rust_only_a_release_off_the_button_does_not_click_it_even_without_a_move() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with_button(&hits);
    // The pointer is seen over the button, so it is hovered when pressed...
    app.on_mouse_move(ON.0, ON.1);
    down(&mut app, ON);
    // ...and the release is reported elsewhere with no move in between.
    up(&mut app, OFF);
    assert_eq!(hits.get(), 0, "the release landed off the button");
}

#[test]
fn rust_only_a_release_off_the_button_after_a_move_does_not_click_it() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with_button(&hits);
    app.on_mouse_move(ON.0, ON.1);
    down(&mut app, ON);
    app.on_mouse_move(OFF.0, OFF.1);
    up(&mut app, OFF);
    assert_eq!(hits.get(), 0);
}

#[test]
fn rust_only_a_press_off_the_button_released_on_it_does_not_click_it() {
    let hits = Rc::new(Cell::new(0));
    let mut app = app_with_button(&hits);
    app.on_mouse_move(OFF.0, OFF.1);
    down(&mut app, OFF);
    app.on_mouse_move(ON.0, ON.1);
    up(&mut app, ON);
    assert_eq!(hits.get(), 0, "the press did not start on the button");
}
