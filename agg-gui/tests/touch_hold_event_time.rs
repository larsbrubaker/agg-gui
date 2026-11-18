//! A finger's hold judged by when its events happened, through the real `App`
//! touch entry points on the virtual UI clock, as a browser on a machine with
//! seconds-long frames drives them: the landing is handled at once, the
//! moves only after the slow frame.
//!
//! 1. A drag that began inside the long-press delay stays a drag when its
//!    move is handled after the deadline, whether the next frame's poll runs
//!    before or after that move is delivered.
//! 2. A lift inside the delay, handled after it, is still a tap.
//! 3. A real hold still long presses: from the frame poll once a whole
//!    frame has begun after the deadline, or at once from a move stamped
//!    after it.
//! 4. Without event times (a native shell) a late frame poll fires the
//!    long press as soon as the clock passes the deadline, as before.
//! 5. A browser folds a slow frame's moves into one; the shell forwards the
//!    samples in order, so a drag whose first sample crossed the slop inside
//!    the delay stays a drag though its last sample is stamped long after.
//! 6. An idle app, driven frame by frame as the shells drive it
//!    (`frame_policy::LayoutTracker::tick`, which lays out only when
//!    something asks), still long presses a finger held still.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use agg_gui::frame_policy::{LayoutTracker, TickMode};
use agg_gui::touch_emulation::LONG_PRESS_DELAY;
use agg_gui::{
    clock, App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, MouseButton, Rect, Size,
    TouchDeviceId, TouchId, Widget,
};
use web_time::Instant;

type Presses = Rc<RefCell<Vec<MouseButton>>>;

/// Fills the window and logs every press it hears; takes none of them.
struct Recorder {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    presses: Presses,
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
        if let Event::MouseDown { button, .. } = event {
            self.presses.borrow_mut().push(*button);
        }
        EventResult::Ignored
    }
}

const VIEWPORT: Size = Size {
    width: 400.0,
    height: 400.0,
};
const DEV: TouchDeviceId = TouchDeviceId(0);
const FINGER: TouchId = TouchId(1);

/// An app laid out once (its first frame) at the current virtual time.
fn app() -> (App, Presses) {
    let presses = Presses::default();
    let mut app = App::new(Box::new(Recorder {
        bounds: Rect::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height),
        children: Vec::new(),
        presses: Rc::clone(&presses),
    }));
    app.layout(VIEWPORT);
    (app, presses)
}

/// A frame starts: `App::layout` polls the long press.
fn frame(app: &mut App) {
    app.layout(VIEWPORT);
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn long_pressed(presses: &Presses) -> bool {
    presses.borrow().contains(&MouseButton::Right)
}

/// The finger lands at `t` (handled at once), then a frame stalls 700 ms.
fn land_then_stall() -> (App, Presses, Instant) {
    let (mut app, presses) = app();
    let t = clock::now();
    app.on_touch_start_at(DEV, FINGER, 100.0, 100.0, None, Some(t));
    clock::advance(ms(700));
    (app, presses, t)
}

#[test]
fn a_drag_handled_after_a_stalled_frame_stays_a_drag_when_the_poll_runs_first() {
    let _guard = clock::scoped_virtual(None);
    let (mut app, presses, t) = land_then_stall();
    // The next frame's poll runs before the browser hands over the move.
    frame(&mut app);
    assert!(
        !long_pressed(&presses),
        "the frame after the stall can't know the finger stayed still"
    );
    app.on_touch_move_at(DEV, FINGER, 160.0, 100.0, None, Some(t + ms(50)));
    frame(&mut app);
    app.on_touch_end(DEV, FINGER);
    frame(&mut app);
    assert!(!long_pressed(&presses), "{:?}", presses.borrow());
    assert!(
        presses.borrow().contains(&MouseButton::Left),
        "the drag fell to the widget as its own left drag: {:?}",
        presses.borrow()
    );
}

#[test]
fn a_drag_handled_after_a_stalled_frame_stays_a_drag_when_the_move_comes_first() {
    let _guard = clock::scoped_virtual(None);
    let (mut app, presses, t) = land_then_stall();
    app.on_touch_move_at(DEV, FINGER, 160.0, 100.0, None, Some(t + ms(50)));
    frame(&mut app);
    app.on_touch_end(DEV, FINGER);
    assert!(!long_pressed(&presses), "{:?}", presses.borrow());
    assert!(presses.borrow().contains(&MouseButton::Left));
}

#[test]
fn a_lift_inside_the_delay_handled_late_is_a_tap() {
    let _guard = clock::scoped_virtual(None);
    let (mut app, presses, t) = land_then_stall();
    app.on_touch_end_at(DEV, FINGER, Some(t + ms(100)));
    assert_eq!(*presses.borrow(), vec![MouseButton::Left], "a tap");
}

#[test]
fn a_real_hold_long_presses_from_the_frame_poll() {
    let _guard = clock::scoped_virtual(None);
    let (mut app, presses) = app();
    let t = clock::now();
    app.on_touch_start_at(DEV, FINGER, 100.0, 100.0, None, Some(t));
    // Frames every 100 ms; the finger sends nothing while it holds still.
    let mut fired_at = None;
    for _ in 0..8 {
        clock::advance(ms(100));
        frame(&mut app);
        if fired_at.is_none() && long_pressed(&presses) {
            fired_at = Some(clock::since(t));
        }
    }
    assert_eq!(
        fired_at,
        Some(LONG_PRESS_DELAY + ms(100)),
        "fires at the first frame whose previous frame began after the deadline"
    );
    assert_eq!(*presses.borrow(), vec![MouseButton::Right], "once");
}

#[test]
fn a_move_stamped_after_the_deadline_fires_the_hold_at_once() {
    let _guard = clock::scoped_virtual(None);
    let (mut app, presses, t) = land_then_stall();
    // The finger drifted within the slop after the deadline: it held.
    app.on_touch_move_at(DEV, FINGER, 103.0, 101.0, None, Some(t + ms(600)));
    assert_eq!(*presses.borrow(), vec![MouseButton::Right]);
}

#[test]
fn without_event_times_a_late_poll_fires_as_soon_as_the_deadline_passed() {
    let _guard = clock::scoped_virtual(None);
    let (mut app, presses) = app();
    app.on_touch_start(DEV, FINGER, 100.0, 100.0, None);
    clock::advance(ms(700));
    frame(&mut app);
    assert_eq!(
        *presses.borrow(),
        vec![MouseButton::Right],
        "a native hold fires on the first frame past the deadline"
    );
}

#[test]
fn a_coalesced_drag_crossing_the_slop_early_stays_a_drag() {
    let _guard = clock::scoped_virtual(None);
    let (mut app, presses) = app();
    let t = clock::now();
    app.on_touch_start_at(DEV, FINGER, 100.0, 100.0, None, Some(t));
    clock::advance(ms(2000));
    // One folded move, forwarded as its samples, oldest first.
    app.on_touch_move_at(DEV, FINGER, 140.0, 100.0, None, Some(t + ms(50)));
    app.on_touch_move_at(DEV, FINGER, 300.0, 100.0, None, Some(t + ms(1900)));
    frame(&mut app);
    app.on_touch_end(DEV, FINGER);
    assert!(!long_pressed(&presses), "{:?}", presses.borrow());
    assert!(presses.borrow().contains(&MouseButton::Left));
}

fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// Hold a finger still on an idle app whose frames run every `step` as the
/// shells run them: a reactive tick that paints only when the app wants to
/// draw and lays out only when the layout key moved or layout was asked for.
/// Returns how long after the landing the long press fired.
fn idle_hold(timed: bool, step: Duration) -> Option<Duration> {
    let (mut app, presses) = app();
    let mut tracker = LayoutTracker::new();
    tracker.tick(&mut app, VIEWPORT, TickMode::Forced, paint);
    let t = clock::now();
    app.on_touch_start_at(DEV, FINGER, 100.0, 100.0, None, timed.then_some(t));
    // The frame the shell runs after input.
    tracker.tick(&mut app, VIEWPORT, TickMode::Forced, paint);
    let frames = (Duration::from_secs(5).as_millis() / step.as_millis()) as usize;
    for _ in 0..frames {
        clock::advance(step);
        tracker.tick(&mut app, VIEWPORT, TickMode::Reactive, paint);
        if long_pressed(&presses) {
            return Some(clock::since(t));
        }
    }
    None
}

#[test]
fn an_idle_app_long_presses_a_still_finger() {
    let _guard = clock::scoped_virtual(None);
    for step in [ms(16), ms(100)] {
        for timed in [false, true] {
            let fired = idle_hold(timed, step);
            // At most one frame late without event times, two with them
            // (the frame that sees the deadline pass, then the poll after it).
            let slack = if timed { step * 3 } else { step * 2 };
            assert!(
                fired.is_some_and(
                    |after| after >= LONG_PRESS_DELAY && after <= LONG_PRESS_DELAY + slack
                ),
                "timed {timed}, frames every {step:?}: fired after {fired:?}"
            );
        }
    }
}
