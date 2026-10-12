//! Port of agg-sharp `WindowWidgetTitleBarDragTests`: a drag of a `Window`'s
//! title bar moves the window and nothing else — one screen pixel of mouse
//! travel is one pixel of movement, and only while the button that started
//! the drag is held.
//!
//! The drags run through the real `App` pointer path; the `App` entry points
//! take Y-down screen coordinates, so C#'s Y-up mouse positions go through
//! `screen_y`. C#'s `MouseButtons.None` move to (-10, -10) — what a platform
//! sink sends when the pointer leaves — is `App::on_mouse_leave`.
//!
//! `HoveringAfterALostMouseUpDoesNotMoveTheWindow` is skipped as a duplicate of
//! `agg-gui-automation/tests/capture_lost_drag_tests.rs::rust_only_a_window_move_ends_on_capture_loss`.

use crate::event::{Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widgets::window::Window;
use crate::{App, SizedBox, Stack};
use std::sync::Arc;

const VP_W: f64 = 800.0;
const VP_H: f64 = 600.0;
/// `Window`'s title bar height.
const TITLE_H: f64 = 28.0;

fn window_bounds(app: &App) -> Rect {
    app.root().children()[0].bounds()
}

fn move_to(app: &mut App, mouse: Point) {
    app.on_mouse_move(mouse.x, VP_H - mouse.y);
}

fn release(app: &mut App, mouse: Point) {
    app.on_mouse_up(
        mouse.x,
        VP_H - mouse.y,
        MouseButton::Left,
        Modifiers::default(),
    );
}

/// Builds a window on an app and presses the middle of its title bar,
/// returning the Y-up position the mouse was pressed at.
fn start_title_bar_drag() -> (App, Point) {
    let font = Arc::new(Font::from_slice(super::TEST_FONT).expect("test font"));
    // Windows register as snap targets in a thread-local list; drop the ones
    // an earlier test on this thread left, which would pull this window.
    crate::snap::clear_targets_for_testing();
    // C#: new RectangleDouble(100, 100, 400, 400)
    let window = Window::new("Drag", font, Box::new(SizedBox::new()))
        .with_bounds(Rect::new(100.0, 100.0, 300.0, 300.0));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(window))));
    app.layout(Size::new(VP_W, VP_H));
    let b = window_bounds(&app);
    let mouse = Point::new(b.x + b.width / 2.0, b.y + b.height - TITLE_H / 2.0);
    move_to(&mut app, mouse);
    app.on_mouse_down(
        mouse.x,
        VP_H - mouse.y,
        MouseButton::Left,
        Modifiers::default(),
    );
    (app, mouse)
}

#[test]
fn title_bar_drag_moves_the_window_by_exactly_the_mouse_travel() {
    let (mut app, mut mouse) = start_title_bar_drag();
    let start = window_bounds(&app);
    let step = (7.0, -5.0);

    for step_index in 1..=10 {
        let s = step_index as f64;
        mouse = Point::new(mouse.x + step.0, mouse.y + step.1);
        move_to(&mut app, mouse);
        let b = window_bounds(&app);
        let (want_x, want_y) = (start.x + step.0 * s, start.y + step.1 * s);
        assert!(
            (b.x - want_x).abs() <= 0.001,
            "after {step_index} steps of {step:?} the left should be {want_x} but was {}",
            b.x
        );
        assert!(
            (b.y - want_y).abs() <= 0.001,
            "after {step_index} steps of {step:?} the bottom should be {want_y} but was {}",
            b.y
        );
    }

    let b = window_bounds(&app);
    assert!(
        (b.width - start.width).abs() <= 0.001,
        "dragging the title bar must not resize the window"
    );
    assert!(
        (b.height - start.height).abs() <= 0.001,
        "dragging the title bar must not resize the window"
    );
    release(&mut app, mouse);
}

// Regression: a platform reports the pointer leaving the window as a move to
// a point far outside it. The title bar took it for a drag and threw the
// window at the corner of the screen.
#[test]
fn pointer_leaving_the_window_does_not_move_it() {
    let (mut app, mut mouse) = start_title_bar_drag();
    mouse = Point::new(mouse.x + 20.0, mouse.y + 20.0);
    move_to(&mut app, mouse);
    let dragged = window_bounds(&app);

    app.on_mouse_leave();

    let b = window_bounds(&app);
    assert!(
        (b.x - dragged.x).abs() <= 0.001,
        "the pointer leaving the window must not move it (left went from {} to {})",
        dragged.x,
        b.x
    );
    assert!(
        (b.y - dragged.y).abs() <= 0.001,
        "the pointer leaving the window must not move it (bottom went from {} to {})",
        dragged.y,
        b.y
    );
}

#[test]
fn a_move_after_the_button_is_released_does_not_move_the_window() {
    let (mut app, mut mouse) = start_title_bar_drag();
    mouse = Point::new(mouse.x + 20.0, mouse.y + 20.0);
    move_to(&mut app, mouse);
    release(&mut app, mouse);
    let dropped = window_bounds(&app);

    // a drag of something else entirely passing over the bar
    mouse = Point::new(mouse.x + 30.0, mouse.y + 30.0);
    move_to(&mut app, mouse);

    let b = window_bounds(&app);
    assert!(
        (b.x - dropped.x).abs() <= 0.001,
        "the drag ended with the button, a later move is not part of it"
    );
    assert!(
        (b.y - dropped.y).abs() <= 0.001,
        "the drag ended with the button, a later move is not part of it"
    );
}
