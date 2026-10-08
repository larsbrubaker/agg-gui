//! [`crate::shell_input::InputForwarder`]: the App calls each shell made
//! before it delegated here (pinned event by event), held-button and
//! modifier bookkeeping, click counting, and the real-input gate. Drives the
//! real `App` with a recording root widget.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use crate::shell_input::{ClickCount, ClickPolicy, ForwarderEvent, InputForwarder, InputSource};
use crate::{
    current_modifiers, App, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Rect, Size,
    Widget,
};

#[derive(Debug, PartialEq)]
enum Seen {
    Move(f64, f64),
    Down(f64, f64, MouseButton, bool),
    Up(f64, f64, MouseButton, bool),
    Wheel(f64, f64, f64, bool),
    Key(bool),
    Dropped(f64, f64, usize),
}

/// Root filling a 200×200 window that records what reaches it. Positions
/// are widget-local Y-up; the window is 200 tall, so screen y maps to 200-y.
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
    fn is_focusable(&self) -> bool {
        true
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        let mut seen = self.seen.borrow_mut();
        match event {
            Event::MouseMove { pos } => seen.push(Seen::Move(pos.x, pos.y)),
            Event::MouseDown {
                pos,
                button,
                modifiers,
            } => seen.push(Seen::Down(pos.x, pos.y, *button, modifiers.shift)),
            Event::MouseUp {
                pos,
                button,
                modifiers,
            } => seen.push(Seen::Up(pos.x, pos.y, *button, modifiers.shift)),
            Event::MouseWheel {
                pos,
                delta_y,
                modifiers,
                ..
            } => seen.push(Seen::Wheel(pos.x, pos.y, *delta_y, modifiers.shift)),
            Event::KeyDown { modifiers, .. } => seen.push(Seen::Key(modifiers.shift)),
            Event::FileDropped { pos, paths } => {
                seen.push(Seen::Dropped(pos.x, pos.y, paths.len()))
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }
}

const SHIFT: Modifiers = Modifiers {
    shift: true,
    ctrl: false,
    alt: false,
    meta: false,
};

fn setup() -> (App, InputForwarder, Rc<RefCell<Vec<Seen>>>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut app = App::new(Box::new(Recorder {
        bounds: Rect::default(),
        seen: Rc::clone(&seen),
        children: Vec::new(),
    }));
    app.layout(Size::new(200.0, 200.0));
    app.on_modifiers_changed(Modifiers::default());
    (app, InputForwarder::new(), seen)
}

fn moved(x: f64, y: f64) -> ForwarderEvent {
    ForwarderEvent::MouseMove {
        x,
        y,
        modifiers: None,
    }
}

fn down(button: MouseButton, clicks: ClickCount) -> ForwarderEvent {
    ForwarderEvent::MouseDown {
        at: None,
        button,
        modifiers: None,
        clicks,
    }
}

fn up(button: MouseButton) -> ForwarderEvent {
    ForwarderEvent::MouseUp {
        at: None,
        button,
        modifiers: None,
    }
}

/// agg-gui-shell's arms: `CursorMoved` tracks the cursor; `MouseInput`
/// presses and releases at it with the modifiers `ModifiersChanged` set.
#[test]
fn native_shell_press_lands_at_the_tracked_cursor_with_held_modifiers() {
    let (mut app, mut fwd, seen) = setup();
    fwd.platform(&mut app, moved(30.0, 50.0));
    fwd.platform(&mut app, ForwarderEvent::ModifiersChanged(SHIFT));
    assert!(current_modifiers().shift);
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
    fwd.platform(&mut app, up(MouseButton::Left));
    let seen = seen.borrow();
    assert_eq!(seen[0], Seen::Move(30.0, 150.0));
    assert!(seen.contains(&Seen::Down(30.0, 150.0, MouseButton::Left, true)));
    assert!(seen.contains(&Seen::Up(30.0, 150.0, MouseButton::Left, true)));
    assert_eq!(fwd.cursor(), (30.0, 50.0));
    assert_eq!(fwd.modifiers(), SHIFT);
}

/// agg-gui-web-shell's listeners: every DOM pointer event carries its own
/// position and modifiers; `pointermove` is `on_mouse_move_mods`.
#[test]
fn web_shell_events_carry_their_own_position_and_modifiers() {
    let (mut app, mut fwd, seen) = setup();
    fwd.platform(
        &mut app,
        ForwarderEvent::MouseMove {
            x: 10.0,
            y: 20.0,
            modifiers: Some(SHIFT),
        },
    );
    assert!(
        current_modifiers().shift,
        "move with mods updates modifiers"
    );
    fwd.platform(
        &mut app,
        ForwarderEvent::MouseDown {
            at: Some((12.0, 20.0)),
            button: MouseButton::Left,
            modifiers: Some(Modifiers::default()),
            clicks: ClickCount::Auto,
        },
    );
    let seen = seen.borrow();
    assert_eq!(seen[0], Seen::Move(10.0, 180.0));
    assert!(
        seen.contains(&Seen::Down(12.0, 180.0, MouseButton::Left, false)),
        "the press is at its own point, without an extra move: {seen:?}"
    );
    assert_eq!(fwd.cursor(), (12.0, 20.0));
    assert_eq!(fwd.modifiers(), Modifiers::default());
}

/// The native wheel arm scrolls at the tracked cursor with held modifiers.
#[test]
fn wheel_without_a_position_scrolls_at_the_cursor() {
    let (mut app, mut fwd, seen) = setup();
    fwd.platform(&mut app, moved(40.0, 60.0));
    fwd.platform(&mut app, ForwarderEvent::ModifiersChanged(SHIFT));
    fwd.platform(
        &mut app,
        ForwarderEvent::Wheel {
            at: None,
            delta_x: 0.0,
            delta_y: 2.0,
            modifiers: None,
        },
    );
    assert_eq!(
        seen.borrow().last(),
        Some(&Seen::Wheel(40.0, 140.0, 2.0, true))
    );
}

#[test]
fn keys_use_held_modifiers_unless_the_event_names_its_own() {
    let (mut app, mut fwd, seen) = setup();
    // Click-to-focus the (focusable) root so keys reach it.
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
    fwd.platform(&mut app, up(MouseButton::Left));
    fwd.platform(&mut app, ForwarderEvent::ModifiersChanged(SHIFT));
    let a = || Key::Char('a');
    fwd.platform(
        &mut app,
        ForwarderEvent::KeyDown {
            key: a(),
            modifiers: None,
        },
    );
    fwd.platform(
        &mut app,
        ForwarderEvent::KeyDown {
            key: a(),
            modifiers: Some(Modifiers::default()),
        },
    );
    let keys: Vec<_> = seen
        .borrow()
        .iter()
        .filter(|s| matches!(s, Seen::Key(_)))
        .map(|s| matches!(s, Seen::Key(true)))
        .collect();
    assert_eq!(keys, vec![true, false]);
    assert_eq!(fwd.modifiers(), Modifiers::default());
}

/// The native drop arm passes a live cursor position; it must not become the
/// tracked cursor (the native shell kept its stale cursor after a drop).
#[test]
fn a_file_drop_does_not_move_the_tracked_cursor() {
    let (mut app, mut fwd, seen) = setup();
    fwd.platform(&mut app, moved(5.0, 5.0));
    fwd.platform(
        &mut app,
        ForwarderEvent::FileDropped {
            x: 100.0,
            y: 100.0,
            paths: vec![PathBuf::from("a.stl")],
        },
    );
    assert_eq!(seen.borrow().last(), Some(&Seen::Dropped(100.0, 100.0, 1)));
    assert_eq!(fwd.cursor(), (5.0, 5.0));
}

/// The native shell's `mouse_buttons_down`: presses minus releases,
/// saturating, behind `pointer_idle` and the bounds auto-save gate.
#[test]
fn held_buttons_count_presses_and_never_go_below_zero() {
    let (mut app, mut fwd, _) = setup();
    fwd.platform(&mut app, up(MouseButton::Left));
    assert_eq!(fwd.buttons_down(), 0, "a stray release cannot underflow");
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
    fwd.platform(&mut app, down(MouseButton::Right, ClickCount::Auto));
    assert_eq!(fwd.buttons_down(), 2);
    assert!(fwd.is_button_down(MouseButton::Left));
    assert!(fwd.is_button_down(MouseButton::Right));
    fwd.platform(&mut app, up(MouseButton::Left));
    assert!(!fwd.is_button_down(MouseButton::Left));
    fwd.platform(&mut app, up(MouseButton::Right));
    assert_eq!(fwd.buttons_down(), 0);
}

#[test]
fn quick_presses_in_place_count_up() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, _) = setup();
    assert_eq!(fwd.click_count(), 0);
    fwd.platform(&mut app, moved(50.0, 50.0));
    for expected in 1..=3 {
        fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
        fwd.platform(&mut app, up(MouseButton::Left));
        assert_eq!(fwd.click_count(), expected);
        crate::clock::advance(Duration::from_millis(100));
    }
}

#[test]
fn a_slow_far_or_other_button_press_starts_a_new_sequence() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, _) = setup();
    let policy = ClickPolicy::default();
    fwd.platform(&mut app, moved(50.0, 50.0));
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
    crate::clock::advance(policy.double_click_time);
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
    assert_eq!(fwd.click_count(), 1, "the window is exclusive");
    fwd.platform(&mut app, moved(50.0 + policy.travel_tolerance + 1.0, 50.0));
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
    assert_eq!(fwd.click_count(), 1, "too far");
    fwd.platform(&mut app, down(MouseButton::Right, ClickCount::Auto));
    assert_eq!(fwd.click_count(), 1, "another button");
    fwd.platform(&mut app, down(MouseButton::Right, ClickCount::Auto));
    assert_eq!(fwd.click_count(), 2);
}

#[test]
fn an_explicit_count_is_taken_as_given_and_continues_from_there() {
    let _clock = crate::clock::scoped_virtual(None);
    let (mut app, mut fwd, _) = setup();
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Explicit(2)));
    assert_eq!(fwd.click_count(), 2);
    fwd.platform(&mut app, down(MouseButton::Left, ClickCount::Auto));
    assert_eq!(fwd.click_count(), 3);
}

#[test]
fn the_gate_drops_platform_input_but_not_simulated_input() {
    let (mut app, mut fwd, seen) = setup();
    fwd.set_platform_input_enabled(false);
    assert!(!fwd.platform(&mut app, moved(10.0, 10.0)));
    assert!(seen.borrow().is_empty());
    assert_eq!(fwd.cursor(), (0.0, 0.0), "dropped input leaves no trace");
    fwd.simulated(&mut app, moved(20.0, 10.0));
    assert!(fwd.forward(&mut app, InputSource::Simulated, moved(20.0, 10.0)));
    assert_eq!(seen.borrow()[0], Seen::Move(20.0, 190.0));
    fwd.set_platform_input_enabled(true);
    assert!(fwd.platform(&mut app, moved(30.0, 10.0)));
    assert_eq!(fwd.cursor(), (30.0, 10.0));
}
