//! `mouse_button_held` follows the button, not the widget that saw the
//! press: a release counts whoever hears it (nobody, or an ancestor the
//! capture moved to when the pressed subtree left the tree), and the window
//! losing activation lets go of every button. A widget keeping its own
//! "press under way" state relies on this to end a press whose release it
//! never heard.

use crate::{
    mouse_button_held, App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Rect, Size,
    TouchDeviceId, TouchId, Widget,
};

const SIZE: Size = Size {
    width: 200.0,
    height: 100.0,
};

/// A box that consumes pointer events but presses of other buttons than the left one
/// (`consume`) or ignores them all, around
/// optional children laid out over its whole area.
struct Pad {
    bounds: Rect,
    consume: bool,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Pad {
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
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        // Only a left press is taken, so a finger drag's middle offer (a pan) is declined.
        let other_press =
            matches!(event, Event::MouseDown { button, .. } if *button != MouseButton::Left);
        if self.consume && !other_press {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }
}

fn pad(consume: bool, children: Vec<Box<dyn Widget>>) -> Box<dyn Widget> {
    Box::new(Pad {
        bounds: Rect::default(),
        consume,
        children,
    })
}

fn app(root: Box<dyn Widget>) -> App {
    let mut app = App::new(root);
    app.layout(SIZE);
    app
}

fn none() -> Modifiers {
    Modifiers::default()
}

#[test]
fn press_and_release_heard_by_nobody_still_count() {
    let mut app = app(pad(false, vec![pad(false, Vec::new())]));
    assert!(!mouse_button_held(MouseButton::Left));

    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    assert!(!app.has_captured_pointer(), "nobody took the press");
    assert!(mouse_button_held(MouseButton::Left));
    assert!(!mouse_button_held(MouseButton::Right));

    app.on_mouse_up(150.0, 50.0, MouseButton::Left, none());
    assert!(!mouse_button_held(MouseButton::Left));
}

#[test]
fn each_button_is_held_on_its_own() {
    let mut app = app(pad(true, Vec::new()));
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    app.on_mouse_down(50.0, 50.0, MouseButton::Right, none());
    app.on_mouse_up(50.0, 50.0, MouseButton::Left, none());
    assert!(!mouse_button_held(MouseButton::Left));
    assert!(mouse_button_held(MouseButton::Right));
    app.on_mouse_up(50.0, 50.0, MouseButton::Right, none());
    assert!(!mouse_button_held(MouseButton::Right));
}

/// The pressed widget's subtree leaves the tree mid-press; the release goes
/// to the surviving ancestor, and the button is up all the same.
#[test]
fn release_after_pressed_subtree_left_the_tree() {
    let page = pad(false, vec![pad(true, Vec::new())]);
    let mut app = app(pad(false, vec![page]));
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    assert_eq!(app.captured_path(), Some(&[0usize, 0][..]));

    app.root_mut().children_mut().clear();
    app.layout(SIZE);
    assert!(mouse_button_held(MouseButton::Left));

    app.on_mouse_up(150.0, 50.0, MouseButton::Left, none());
    assert!(!mouse_button_held(MouseButton::Left));
}

/// A press carried out of the window may never send its release here.
#[test]
fn deactivation_lets_go_of_every_button() {
    let mut app = app(pad(true, Vec::new()));
    app.on_window_activated();
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, none());
    app.on_mouse_down(50.0, 50.0, MouseButton::Middle, none());

    app.on_window_deactivated();
    assert!(!mouse_button_held(MouseButton::Left));
    assert!(!mouse_button_held(MouseButton::Middle));
}

/// A finger drag nothing pans is the widget's own left drag: the left
/// button is held while the finger is down (not the declined middle offer),
/// and the lift, which ends the press without a release, lets go of it.
#[test]
fn finger_drag_holds_left_until_the_lift() {
    let mut app = app(pad(false, vec![pad(true, Vec::new())]));
    let (dev, id) = (TouchDeviceId(0), TouchId(0));
    app.on_touch_start(dev, id, 20.0, 50.0, None);
    app.on_touch_move(dev, id, 120.0, 50.0, None);
    assert!(app.has_captured_pointer());
    assert!(mouse_button_held(MouseButton::Left));
    assert!(!mouse_button_held(MouseButton::Middle));

    app.on_touch_end(dev, id);
    assert!(!mouse_button_held(MouseButton::Left));
}
