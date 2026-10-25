//! A finger held still opens the context menu: `App` turns a long press
//! into a right press and release at the touch-down point that reaches the
//! widget under the finger, wakes an idle app for it, marks the press as a
//! finger's, tells the long-press listeners, and draws the filling ring. A
//! widget that claims the finger keeps the right button down until the lift.
//! Driven on the virtual UI clock through `App`'s touch entry points, the
//! way the touch shells call them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use super::touch_pan_fixtures::{reset, Board, Finger};
use crate::touch_emulation::{self, LONG_PRESS_CUE_DELAY, LONG_PRESS_DELAY};
use crate::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, MouseButton, Point, Rect, Size, Widget,
};

const WINDOW: f64 = 400.0;
const FINGERS: Finger = Finger { height: WINDOW };

/// Every press and release a [`Recorder`] saw, with whether the press was a
/// finger's ([`touch_emulation::last_press_was_touch`]) at the time.
#[derive(Default)]
struct Seen {
    events: RefCell<Vec<(&'static str, MouseButton, Point, bool)>>,
}

/// A leaf that records presses and releases of every button, and, when
/// `claims`, claims a long press's finger and records the moves it gets.
struct Recorder {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    seen: Rc<Seen>,
    claims: bool,
}

impl Widget for Recorder {
    fn type_name(&self) -> &'static str {
        "Recorder"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
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
        let (kind, button, pos) = match event {
            Event::MouseDown { button, pos, .. } => ("down", *button, *pos),
            Event::MouseUp { button, pos, .. } => ("up", *button, *pos),
            // (The lift's pointer-leave sentinel move lands off the widget.)
            Event::MouseMove { pos } if self.claims && pos.x >= 0.0 => {
                ("move", MouseButton::Left, *pos)
            }
            Event::MouseCaptureLost if self.claims => ("lost", MouseButton::Left, Point::ORIGIN),
            _ => return EventResult::Ignored,
        };
        if self.claims && kind == "down" && button == MouseButton::Right {
            touch_emulation::claim_long_press_drag();
        }
        let touch = touch_emulation::last_press_was_touch();
        self.seen
            .events
            .borrow_mut()
            .push((kind, button, pos, touch));
        EventResult::Consumed
    }
}

/// A 400 x 400 window filled by a [`Recorder`], on the virtual clock.
fn make() -> (App, Rc<Seen>, crate::clock::ClockGuard) {
    make_with(false)
}

/// [`make`], with a recorder that claims long presses when `claims`.
fn make_with(claims: bool) -> (App, Rc<Seen>, crate::clock::ClockGuard) {
    reset(1.0);
    touch_emulation::set_long_press_ring_drawn(true);
    let clock = crate::clock::scoped_virtual(None);
    crate::animation::clear_draw_request();
    let seen = Rc::new(Seen::default());
    let recorder = Recorder {
        bounds: Rect::default(),
        children: Vec::new(),
        seen: Rc::clone(&seen),
        claims,
    };
    let root =
        Board::new(WINDOW, WINDOW).place(Rect::new(0.0, 0.0, WINDOW, WINDOW), Box::new(recorder));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(WINDOW, WINDOW));
    (app, seen, clock)
}

/// One frame the way a shell runs it: layout, then paint. Returns the
/// number of pixels the frame drew.
fn frame(app: &mut App) -> usize {
    app.layout(Size::new(WINDOW, WINDOW));
    let mut fb = Framebuffer::new(WINDOW as u32, WINDOW as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
    drop(ctx);
    fb.pixels().chunks(4).filter(|p| p[3] != 0).count()
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn holding_a_finger_right_clicks_the_widget_under_it_marked_as_touch() {
    let (mut app, seen, _clock) = make();
    FINGERS.down(&mut app, 120.0, 250.0);
    FINGERS.move_to(&mut app, 125.0, 250.0); // drift inside the slop
    crate::clock::advance(LONG_PRESS_DELAY);
    frame(&mut app);
    // The menus' own touch test (a touch event within the last 50 ms) holds
    // for the right click too, so a menu it opens has finger-sized rows.
    assert_eq!(
        crate::touch_state::last_touch_event_age(),
        Some(Duration::ZERO)
    );
    FINGERS.move_to(&mut app, 180.0, 250.0);
    FINGERS.up(&mut app);
    let at = Point::new(120.0, 250.0);
    assert_eq!(
        *seen.events.borrow(),
        vec![
            ("down", MouseButton::Right, at, true),
            ("up", MouseButton::Right, at, true),
        ],
        "one right click at the touch-down point and nothing on the lift"
    );
}

#[test]
fn an_idle_app_is_woken_for_the_cue_and_then_drawn_until_the_long_press_fires() {
    let (mut app, seen, _clock) = make();
    let fired = Rc::new(Cell::new(0));
    let fired_at = Rc::clone(&fired);
    touch_emulation::on_long_press(move |cue| {
        assert_eq!((cue.x, cue.y, cue.progress), (200.0, 200.0, 1.0));
        fired_at.set(fired_at.get() + 1);
    });
    let down = crate::clock::now();
    FINGERS.down(&mut app, 200.0, 200.0);
    assert_eq!(app.next_draw_deadline(), Some(down + LONG_PRESS_CUE_DELAY));
    crate::clock::advance(LONG_PRESS_CUE_DELAY);
    assert!(app.wants_draw(), "the cue's wake-up came due");
    frame(&mut app);
    // While the ring fills every frame asks for the next one.
    for _ in 0..7 {
        assert!(app.wants_draw(), "the ring keeps animating");
        crate::clock::advance(ms(50));
        frame(&mut app);
    }
    assert_eq!(fired.get(), 1, "fired at 500 ms");
    assert_eq!(seen.events.borrow().len(), 2);
    assert!(!app.wants_draw(), "idle again once the long press fired");
    assert_eq!(touch_emulation::long_press_cue(), None);
    FINGERS.up(&mut app);
    assert_eq!(fired.get(), 1);
}

#[test]
fn the_ring_fills_around_the_finger_and_an_app_can_turn_it_off() {
    let (mut app, _seen, _clock) = make();
    FINGERS.down(&mut app, 200.0, 200.0);
    assert_eq!(frame(&mut app), 0, "no ring for a quick tap");
    crate::clock::advance(LONG_PRESS_CUE_DELAY + ms(175));
    let drawn = frame(&mut app);
    assert!(drawn > 0, "the ring is drawn");
    let cue = touch_emulation::long_press_cue().expect("cue published");
    assert_eq!((cue.x, cue.y), (200.0, 200.0));
    assert!(
        (cue.progress - 0.5).abs() < 1e-9,
        "half way: {}",
        cue.progress
    );

    touch_emulation::set_long_press_ring_drawn(false);
    assert_eq!(frame(&mut app), 0, "the app draws its own feedback");
    assert!(touch_emulation::long_press_cue().is_some());
    touch_emulation::set_long_press_ring_drawn(true);
}

#[test]
fn a_mouse_press_after_a_touch_is_not_marked_as_touch() {
    let (mut app, seen, _clock) = make();
    FINGERS.down(&mut app, 100.0, 100.0);
    FINGERS.up(&mut app);
    app.on_mouse_down(
        50.0,
        WINDOW - 50.0,
        MouseButton::Right,
        crate::Modifiers::default(),
    );
    let touch_flags: Vec<bool> = seen
        .events
        .borrow()
        .iter()
        .filter(|e| e.0 == "down")
        .map(|e| e.3)
        .collect();
    assert_eq!(touch_flags, vec![true, false], "the tap, then the mouse");
}

/// What the recorder saw from the first press on (the touch-down's hover
/// move comes before it).
fn after_press(seen: &Seen) -> Vec<(&'static str, MouseButton, Point, bool)> {
    let events = seen.events.borrow();
    let first = events.iter().position(|e| e.0 == "down").unwrap_or(0);
    events[first..].to_vec()
}

#[test]
fn a_widget_that_claims_the_long_press_gets_the_slide_and_a_right_release_on_lift() {
    let (mut app, seen, _clock) = make_with(true);
    FINGERS.down(&mut app, 120.0, 250.0);
    crate::clock::advance(LONG_PRESS_DELAY);
    frame(&mut app);
    FINGERS.move_to(&mut app, 180.0, 240.0);
    FINGERS.up(&mut app);
    let at = Point::new(120.0, 250.0);
    let to = Point::new(180.0, 240.0);
    assert_eq!(
        after_press(&seen),
        vec![
            ("down", MouseButton::Right, at, true),
            ("move", MouseButton::Left, to, true),
            ("up", MouseButton::Right, to, true),
        ],
        "the press stays down through the slide and lifts where the finger did"
    );
    assert!(!app.has_captured_pointer(), "the lift released the capture");
}

#[test]
fn a_second_finger_cancels_a_claimed_long_press() {
    let (mut app, seen, _clock) = make_with(true);
    FINGERS.down(&mut app, 120.0, 250.0);
    crate::clock::advance(LONG_PRESS_DELAY);
    frame(&mut app);
    FINGERS.down_id(
        &mut app,
        super::touch_pan_fixtures::SECOND_FINGER,
        300.0,
        250.0,
    );
    let kinds: Vec<&str> = after_press(&seen).iter().map(|e| e.0).collect();
    assert_eq!(kinds, vec!["down", "lost"]);
    assert!(!app.has_captured_pointer());
}

#[test]
fn the_lift_of_a_claimed_long_press_whose_capture_was_dropped_releases_nothing() {
    let (mut app, seen, _clock) = make_with(true);
    FINGERS.down(&mut app, 120.0, 250.0);
    crate::clock::advance(LONG_PRESS_DELAY);
    frame(&mut app);
    app.on_window_deactivated();
    assert!(!app.has_captured_pointer());
    FINGERS.up(&mut app);
    let kinds: Vec<&str> = after_press(&seen).iter().map(|e| e.0).collect();
    assert_eq!(
        kinds,
        vec!["down", "lost"],
        "no right release after the capture went"
    );
}
