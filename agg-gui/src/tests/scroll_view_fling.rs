//! Momentum scrolling through `App`'s touch entry points: a finger that
//! flicks a `ScrollView`'s content and lifts while moving throws it on; it
//! slows to a stop, stops at the content's ends, and keeps frames coming
//! only while it moves. A finger landing on the moving content catches it
//! without tapping (or long pressing) what is under it; a finger elsewhere
//! taps as usual. Stop-then-lift, a second finger and the mouse never fling,
//! and the wheel, a press or a programmatic scroll stop a fling. Driven on
//! the virtual UI clock, a 60 Hz frame at a time, the way the touch shells
//! call `App`. The maths alone is in `widgets/scroll_view/fling_tests.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use super::touch_pan_fixtures::{reset, screen, scroll_at, Board, Finger, DEV, SECOND_FINGER};
use crate::widgets::scroll_view::fling::{FLING_TIME_CONSTANT, PAUSE_BEFORE_LIFT};
use crate::widgets::scroll_view::ScrollBarStyle;
use crate::{
    App, DrawCtx, Event, EventResult, Modifiers, MouseButton, Rect, ScrollView, Size, Widget,
};

const W: f64 = 200.0;
const H: f64 = 100.0;
const FINGERS: Finger = Finger { height: H };
const FRAME: Duration = Duration::from_millis(16);

/// Every press a [`Pad`] saw, by button.
#[derive(Default)]
struct Presses {
    seen: RefCell<Vec<MouseButton>>,
    clicks: Cell<u32>,
}

/// A clickable surface covering the whole content: whatever row a finger
/// lands on, it lands on this.
struct Pad {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    presses: Rc<Presses>,
    pressed: bool,
}

impl Widget for Pad {
    fn type_name(&self) -> &'static str {
        "Pad"
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
        match event {
            // A middle press is a pan's, which a row leaves to its view.
            Event::MouseDown { button, .. } if *button != MouseButton::Middle => {
                self.presses.seen.borrow_mut().push(*button);
                self.pressed = *button == MouseButton::Left;
                EventResult::Consumed
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                if self.pressed {
                    self.presses.clicks.set(self.presses.clicks.get() + 1);
                }
                self.pressed = false;
                EventResult::Consumed
            }
            Event::MouseCaptureLost => {
                self.pressed = false;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

fn pad(presses: &Rc<Presses>) -> Box<dyn Widget> {
    Box::new(Pad {
        bounds: Rect::default(),
        children: Vec::new(),
        presses: Rc::clone(presses),
        pressed: false,
    })
}

struct View {
    app: App,
    offset: Rc<Cell<f64>>,
    /// Presses on the scrolled content.
    content: Rc<Presses>,
    /// Presses on the strip below the view (window y 0..20).
    outside: Rc<Presses>,
    _clock: crate::clock::ClockGuard,
}

/// A 200 x 80 view (window y 20..100) over `content_height` of content that
/// is all one pad, above a 20 px pad of its own; `tweak` adjusts the view.
fn make_with(content_height: f64, tweak: impl FnOnce(ScrollView) -> ScrollView) -> View {
    reset(1.0);
    let clock = crate::clock::scoped_virtual(None);
    let content = Rc::new(Presses::default());
    let outside = Rc::new(Presses::default());
    let offset = Rc::new(Cell::new(0.0));
    let board =
        Board::new(W, content_height).place(Rect::new(0.0, 0.0, W, content_height), pad(&content));
    let scroll = tweak(
        ScrollView::new(Box::new(board))
            .with_style(ScrollBarStyle::solid())
            .with_offset_cell(Rc::clone(&offset)),
    );
    let root = Board::new(W, H)
        .place(Rect::new(0.0, 20.0, W, 80.0), Box::new(scroll))
        .place(Rect::new(0.0, 0.0, W, 20.0), pad(&outside));
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(W, H));
    View {
        app,
        offset,
        content,
        outside,
        _clock: clock,
    }
}

fn make() -> View {
    make_with(5000.0, |s| s)
}

fn flinging(v: &mut View) -> bool {
    scroll_at(&mut v.app, &[0]).is_flinging()
}

/// One 60 Hz frame: the clock moves on and the app lays out.
fn frame(v: &mut View) {
    crate::clock::advance(FRAME);
    v.app.layout(Size::new(W, H));
}

/// A finger down at window y `y`, then `moves` moves a frame apart, each
/// `step` px up (content scrolls further down). Leaves the finger down.
fn drag_up(v: &mut View, y: f64, step: f64, moves: u32) -> f64 {
    FINGERS.down(&mut v.app, 50.0, y);
    let mut at = y;
    for _ in 0..moves {
        crate::clock::advance(FRAME);
        at += step;
        FINGERS.move_to(&mut v.app, 50.0, at);
    }
    at
}

/// A flick: `moves` moves of `step` px a frame apart, lifted 4 ms after the
/// last.
fn flick_by(v: &mut View, y: f64, step: f64, moves: u32) {
    drag_up(v, y, step, moves);
    crate::clock::advance(Duration::from_millis(4));
    FINGERS.up(&mut v.app);
}

/// Frames until the fling ends (at most ten seconds' worth); returns how
/// many ran.
/// The usual flick: from window y 25 up to 97, 12 px a frame (750 px/s).
fn flick(v: &mut View) {
    flick_by(v, 25.0, 12.0, 6);
}

fn run_out(v: &mut View) -> u32 {
    let mut frames = 0;
    while flinging(v) {
        frame(v);
        frames += 1;
        assert!(frames < 600, "the fling ends");
    }
    frames
}

#[test]
fn a_flick_keeps_scrolling_after_the_lift_and_slows_to_a_stop() {
    let mut v = make();
    flick(&mut v);
    let lifted_at = v.offset.get();
    assert_eq!(lifted_at, 72.0, "the pan followed the finger");
    assert!(flinging(&mut v));

    let mut last = lifted_at;
    let mut last_gain = f64::INFINITY;
    let mut frames = 0;
    while flinging(&mut v) {
        frame(&mut v);
        frames += 1;
        let gain = v.offset.get() - last;
        assert!(gain >= 0.0, "it keeps going the way it was thrown");
        // whole-pixel offsets: allow a pixel of rounding
        assert!(gain <= last_gain + 1.0, "it slows down");
        last_gain = gain;
        last = v.offset.get();
        assert!(frames < 600, "the fling ends");
    }
    let travel = last - lifted_at;
    let expected = 750.0 * FLING_TIME_CONSTANT;
    assert!(
        (travel - expected).abs() <= 1.0,
        "a 750 px/s throw travels {expected}, got {travel}"
    );
    // and it is still afterwards
    frame(&mut v);
    assert_eq!(v.offset.get(), last);
}

#[test]
fn frames_are_asked_for_only_while_the_content_moves() {
    let mut v = make();
    crate::animation::take_layout_request();
    flick(&mut v);
    assert!(
        crate::animation::layout_requested(),
        "the lift asks for the first frame"
    );
    while flinging(&mut v) {
        frame(&mut v);
        assert_eq!(
            crate::animation::layout_requested(),
            flinging(&mut v),
            "a frame is asked for exactly while the fling still moves"
        );
    }
    frame(&mut v);
    assert!(!crate::animation::layout_requested());
    assert!(!crate::touch_emulation::fling::any_flinging());
}

#[test]
fn stop_then_lift_does_not_fling() {
    let mut v = make();
    drag_up(&mut v, 25.0, 12.0, 6);
    let held = v.offset.get();
    crate::clock::advance(PAUSE_BEFORE_LIFT + Duration::from_millis(20));
    FINGERS.up(&mut v.app);
    assert!(!flinging(&mut v));
    frame(&mut v);
    frame(&mut v);
    assert_eq!(v.offset.get(), held);
}

#[test]
fn a_flick_stops_at_the_contents_end() {
    // 500 of content in an 80 view: 420 is the end
    let mut v = make_with(500.0, |s| s);
    scroll_at(&mut v.app, &[0]).set_scroll_offset(300.0);
    v.app.layout(Size::new(W, H));
    // lifted at 372 at 750 px/s: a 244 px throw with 48 left to the end
    flick(&mut v);
    assert!(flinging(&mut v));
    run_out(&mut v);
    assert_eq!(v.offset.get(), 420.0);

    // and back towards the top, lifted at 345 at 1562 px/s: stops at 0
    flick_by(&mut v, 95.0, -25.0, 3);
    assert!(flinging(&mut v));
    run_out(&mut v);
    assert_eq!(v.offset.get(), 0.0);
}

#[test]
fn a_finger_on_the_moving_content_stops_it_and_is_no_tap() {
    let mut v = make();
    flick(&mut v);
    for _ in 0..5 {
        frame(&mut v);
    }
    assert!(flinging(&mut v));

    FINGERS.down(&mut v.app, 50.0, 60.0);
    let caught_at = v.offset.get();
    frame(&mut v);
    assert!(!flinging(&mut v), "the touch stopped it");
    assert_eq!(v.offset.get(), caught_at, "where the finger landed");

    // held past the long-press time and lifted: neither a tap nor a long press
    for _ in 0..40 {
        frame(&mut v);
    }
    FINGERS.up(&mut v.app);
    assert!(
        v.content.seen.borrow().is_empty(),
        "nothing was pressed: {:?}",
        v.content.seen.borrow()
    );
    assert_eq!(v.content.clicks.get(), 0);

    // with the content at rest, a tap taps again
    FINGERS.down(&mut v.app, 50.0, 60.0);
    FINGERS.up(&mut v.app);
    assert_eq!(v.content.clicks.get(), 1);
}

#[test]
fn a_finger_that_catches_a_fling_can_drag_on_from_there() {
    let mut v = make();
    flick(&mut v);
    frame(&mut v);
    frame(&mut v);
    let caught_at = v.offset.get();
    // caught, then dragged up 30 at once: the pan starts where the throw got to
    FINGERS.down(&mut v.app, 50.0, 40.0);
    FINGERS.move_to(&mut v.app, 50.0, 70.0);
    assert!(!flinging(&mut v));
    assert_eq!(v.offset.get(), caught_at + 30.0);
    crate::clock::advance(PAUSE_BEFORE_LIFT * 2);
    FINGERS.up(&mut v.app);
    assert_eq!(v.content.clicks.get(), 0);
    assert!(v.content.seen.borrow().is_empty());
}

#[test]
fn a_finger_elsewhere_leaves_the_fling_running_and_taps() {
    let mut v = make();
    flick(&mut v);
    frame(&mut v);
    // the strip below the view
    FINGERS.down(&mut v.app, 50.0, 10.0);
    FINGERS.up(&mut v.app);
    assert_eq!(v.outside.clicks.get(), 1);
    let before = v.offset.get();
    frame(&mut v);
    assert!(flinging(&mut v));
    assert!(v.offset.get() > before);
}

#[test]
fn a_second_finger_ends_the_pan_without_a_fling() {
    let mut v = make();
    drag_up(&mut v, 25.0, 12.0, 6);
    FINGERS.down_id(&mut v.app, SECOND_FINGER, 150.0, 50.0);
    assert!(!flinging(&mut v));
    FINGERS.up(&mut v.app);
    v.app.on_touch_end(DEV, SECOND_FINGER);
    assert!(!flinging(&mut v));
}

#[test]
fn a_cancelled_touch_does_not_fling() {
    let mut v = make();
    drag_up(&mut v, 25.0, 12.0, 6);
    v.app
        .on_touch_cancel(DEV, super::touch_pan_fixtures::FINGER);
    assert!(!flinging(&mut v));
}

#[test]
fn the_wheel_stops_a_fling() {
    let mut v = make();
    flick(&mut v);
    frame(&mut v);
    let (sx, sy) = screen(H, 50.0, 60.0);
    v.app.on_mouse_wheel(sx, sy, 1.0);
    assert!(!flinging(&mut v));
    let after_wheel = v.offset.get();
    frame(&mut v);
    assert_eq!(v.offset.get(), after_wheel);
}

#[test]
fn a_programmatic_scroll_stops_a_fling() {
    let mut v = make();
    flick(&mut v);
    frame(&mut v);
    scroll_at(&mut v.app, &[0]).set_scroll_offset(10.0);
    assert!(!flinging(&mut v));
    frame(&mut v);
    assert_eq!(v.offset.get(), 10.0);

    // through the offset cell too
    flick(&mut v);
    frame(&mut v);
    v.offset.set(25.0);
    frame(&mut v);
    assert!(!flinging(&mut v));
    assert_eq!(v.offset.get(), 25.0);
    frame(&mut v);
    assert_eq!(v.offset.get(), 25.0);
}

#[test]
fn a_view_can_opt_out() {
    let mut v = make_with(5000.0, |s| s.with_touch_fling(false));
    flick(&mut v);
    let lifted_at = v.offset.get();
    assert!(!flinging(&mut v));
    frame(&mut v);
    assert_eq!(v.offset.get(), lifted_at);
}

#[test]
fn the_mouse_scrolls_as_it_always_did() {
    let mut v = make();
    // the wheel: one notch, no momentum
    let (sx, sy) = screen(H, 50.0, 60.0);
    v.app.on_mouse_wheel(sx, sy, -1.0);
    assert_eq!(v.offset.get(), crate::wheel::PIXELS_PER_NOTCH);
    assert!(!flinging(&mut v));
    frame(&mut v);
    frame(&mut v);
    assert_eq!(v.offset.get(), crate::wheel::PIXELS_PER_NOTCH);

    // a fast middle-button drag pans and stops where it is let go
    let (sx, sy) = screen(H, 50.0, 30.0);
    v.app
        .on_mouse_down(sx, sy, MouseButton::Middle, Modifiers::default());
    let mut y = 30.0;
    for _ in 0..6 {
        crate::clock::advance(FRAME);
        y += 10.0;
        let (sx, sy) = screen(H, 50.0, y);
        v.app.on_mouse_move(sx, sy);
    }
    let (sx, sy) = screen(H, 50.0, y);
    v.app
        .on_mouse_up(sx, sy, MouseButton::Middle, Modifiers::default());
    let let_go = v.offset.get();
    assert_eq!(let_go, crate::wheel::PIXELS_PER_NOTCH + 60.0);
    assert!(!flinging(&mut v));
    frame(&mut v);
    assert_eq!(v.offset.get(), let_go);
}
