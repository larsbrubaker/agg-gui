//! Keyboard modifiers during pointer interaction.
//!
//! `Event::MouseMove` has no `modifiers` field, so widgets read
//! [`crate::current_modifiers`] while handling a move; a modifier-only
//! change mid-drag is delivered as `Event::ModifiersChanged` to the widget
//! holding mouse capture (see `App::on_modifiers_changed`). These drive the
//! real `App` entry points a platform shell calls.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    current_modifiers, App, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Rect, Size,
    Widget,
};

#[derive(Debug, PartialEq)]
enum Seen {
    /// A `MouseMove`, with `current_modifiers().shift` at that moment.
    Move {
        shift: bool,
    },
    Changed {
        shift: bool,
    },
}

/// Leaf that captures the pointer on `MouseDown` and records moves and
/// modifier changes.
struct Recorder {
    bounds: Rect,
    seen: Rc<RefCell<Vec<Seen>>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Recorder {
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
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::MouseDown { .. } | Event::MouseUp { .. } => EventResult::Consumed,
            Event::MouseMove { .. } => {
                seen.push(Seen::Move {
                    shift: current_modifiers().shift,
                });
                EventResult::Consumed
            }
            Event::ModifiersChanged { modifiers } => {
                seen.push(Seen::Changed {
                    shift: modifiers.shift,
                });
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

const SHIFT: Modifiers = Modifiers {
    shift: true,
    ctrl: false,
    alt: false,
    meta: false,
};

fn app_with_recorder() -> (App, Rc<RefCell<Vec<Seen>>>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let rec = Recorder {
        bounds: Rect::default(),
        seen: Rc::clone(&seen),
        children: Vec::new(),
    };
    let mut app = App::new(Box::new(rec));
    app.layout(Size::new(200.0, 200.0));
    app.on_modifiers_changed(Modifiers::default());
    (app, seen)
}

#[test]
fn move_after_shift_down_carries_shift() {
    let (mut app, seen) = app_with_recorder();
    app.on_mouse_move(50.0, 50.0);
    // Shift pressed with no other key: winit sends ModifiersChanged plus a
    // KeyDown for the Shift key itself.
    app.on_modifiers_changed(SHIFT);
    app.on_key_down(Key::Other("Shift".into()), SHIFT);
    app.on_mouse_move(60.0, 50.0);
    let seen = seen.borrow();
    assert_eq!(seen.first(), Some(&Seen::Move { shift: false }));
    assert_eq!(seen.last(), Some(&Seen::Move { shift: true }));
}

#[test]
fn captured_drag_sees_shift_release() {
    let (mut app, seen) = app_with_recorder();
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, SHIFT);
    assert!(app.has_captured_pointer());
    app.on_mouse_move(60.0, 50.0);
    // Release Shift mid-drag; no focused widget, so only capture routing
    // can deliver it. A key-up arriving before winit's ModifiersChanged must
    // deliver it too (and the later ModifiersChanged must not repeat it).
    app.on_key_up(Key::Other("Shift".into()), Modifiers::default());
    app.on_modifiers_changed(Modifiers::default());
    app.on_mouse_move(70.0, 50.0);
    app.on_mouse_up(70.0, 50.0, MouseButton::Left, Modifiers::default());
    assert!(!app.has_captured_pointer());
    // The last `Move` is not part of the drag: once capture ends,
    // `on_mouse_up` re-resolves hover at the release point (so the widget
    // under the pointer reclaims the cursor icon) by sending it a
    // `MouseMove`. It must see the released modifier state, not the Shift
    // the drag started with.
    assert_eq!(
        *seen.borrow(),
        vec![
            Seen::Move { shift: true },
            Seen::Changed { shift: false },
            Seen::Move { shift: false },
            Seen::Move { shift: false },
        ]
    );
}

#[test]
fn hover_refresh_after_release_uses_release_modifiers() {
    let (mut app, seen) = app_with_recorder();
    app.on_mouse_down(50.0, 50.0, MouseButton::Left, SHIFT);
    app.on_mouse_move(60.0, 50.0);
    // Shift let go while the button was held, but the only report of it is
    // the release event's own modifiers (no key-up, no ModifiersChanged).
    // The hover refresh `on_mouse_up` sends must not carry the stale Shift.
    app.on_mouse_up(60.0, 50.0, MouseButton::Left, Modifiers::default());
    assert!(!current_modifiers().shift);
    assert_eq!(seen.borrow().last(), Some(&Seen::Move { shift: false }));
}
