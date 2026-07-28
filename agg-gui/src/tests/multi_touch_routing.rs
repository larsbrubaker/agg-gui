//! Widget-scoped multi-touch gesture routing tests.
//!
//! Regression guard for three confirmed bugs in the old global-thread-local
//! design: (1) every open gesture consumer reacted to any two-finger gesture;
//! (2) gestures reacted regardless of where they started; (3) capture didn't
//! hold once the centroid drifted. These drive REAL touches through
//! `App::on_touch_start/move` (screen coords) and paint through the headless
//! `GfxCtx`/`Framebuffer` harness so the whole pipeline — recogniser,
//! aggregate publish, and captured `Event::MultiTouch` routing — runs.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, TouchDeviceId, TouchId,
    Widget,
};

/// Leaf that consumes `Event::MultiTouch` and counts deliveries. Anything
/// else is ignored, so mouse emulation from the touch pipeline can't inflate
/// the count.
struct GestureCounter {
    bounds: Rect,
    count: Rc<Cell<usize>>,
}

impl GestureCounter {
    fn new(count: Rc<Cell<usize>>) -> Self {
        Self {
            bounds: Rect::default(),
            count,
        }
    }
}

impl Widget for GestureCounter {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &[]
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        panic!("GestureCounter has no children")
    }
    fn layout(&mut self, available: Size) -> Size {
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MultiTouch { .. } => {
                self.count.set(self.count.get() + 1);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// Root container that lays out two `GestureCounter` panes at explicit,
/// non-overlapping bounds with a dead-space gap between them. It does NOT
/// consume `Event::MultiTouch`, so a gesture starting in the gap finds no
/// consumer at all.
struct TwoPane {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for TwoPane {
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
        // Left pane [0,150], gap (150,250), right pane [250,400] on a
        // 400-wide viewport. The gap is the "dead space" case.
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        self.children[0].set_bounds(Rect::new(0.0, 0.0, 150.0, available.height));
        self.children[1].set_bounds(Rect::new(250.0, 0.0, 150.0, available.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

const DEV: TouchDeviceId = TouchDeviceId(0);
const VIEWPORT: Size = Size {
    width: 400.0,
    height: 200.0,
};

/// Build an App over two side-by-side gesture counters. Returns the app and
/// the (left, right) delivery counters.
fn app_with_two_panes() -> (App, Rc<Cell<usize>>, Rc<Cell<usize>>) {
    let left = Rc::new(Cell::new(0));
    let right = Rc::new(Cell::new(0));
    let root = TwoPane {
        bounds: Rect::default(),
        children: vec![
            Box::new(GestureCounter::new(Rc::clone(&left))),
            Box::new(GestureCounter::new(Rc::clone(&right))),
        ],
    };
    let mut app = App::new(Box::new(root));
    app.layout(VIEWPORT);
    (app, left, right)
}

/// One headless paint — this is where the recogniser aggregates, the
/// thread-local publishes, and `dispatch_gesture` routes the event.
fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// Land two fingers whose centroid (in Y-up world space) sits at
/// `screen_x` given `flip_y` maps `y_down` → `viewport_h - y_down`.
/// Screen X passes straight through, so a centroid screen-x maps to the same
/// world x.
fn two_fingers_at(app: &mut App, cx_screen: f64, y_screen: f64) {
    app.on_touch_start(DEV, TouchId(0), cx_screen - 20.0, y_screen, None);
    app.on_touch_start(DEV, TouchId(1), cx_screen + 20.0, y_screen, None);
}

fn move_two_fingers_to(app: &mut App, cx_screen: f64, y_screen: f64) {
    app.on_touch_move(DEV, TouchId(0), cx_screen - 20.0, y_screen, None);
    app.on_touch_move(DEV, TouchId(1), cx_screen + 20.0, y_screen, None);
}

/// Issues 1 + 2: a gesture that STARTS over the left pane is delivered only to
/// the left pane. The right pane — even though it also consumes the event —
/// gets nothing. Under the old global design both would have reacted.
#[test]
fn gesture_over_left_pane_delivers_only_to_left() {
    let (mut app, left, right) = app_with_two_panes();

    // Centroid at screen x=75 → world x=75, inside the left pane [0,150].
    two_fingers_at(&mut app, 75.0, 100.0);
    paint(&mut app); // start frame: hit-test + capture + first delivery

    assert_eq!(left.get(), 1, "left pane must receive the gesture");
    assert_eq!(right.get(), 0, "right pane must NOT react (issue 1 + 2)");
    assert!(
        crate::current_multi_touch().is_some(),
        "the display-only thread-local must stay published"
    );

    // A real move keeps the gesture alive; still only the left pane.
    move_two_fingers_to(&mut app, 75.0, 110.0);
    paint(&mut app);
    assert_eq!(left.get(), 2);
    assert_eq!(right.get(), 0);
}

/// Capture holds: once the left pane owns the gesture, dragging the centroid
/// all the way over the right pane keeps delivering to the left (standard
/// pointer-capture semantics). Issue 3's guard.
#[test]
fn capture_holds_when_centroid_drifts_over_other_pane() {
    let (mut app, left, right) = app_with_two_panes();

    two_fingers_at(&mut app, 75.0, 100.0);
    paint(&mut app);
    assert_eq!(left.get(), 1);

    // Drift the centroid to screen x=325 → world x=325, over the RIGHT pane.
    move_two_fingers_to(&mut app, 325.0, 100.0);
    paint(&mut app);

    assert_eq!(left.get(), 2, "captured left pane keeps receiving");
    assert_eq!(
        right.get(),
        0,
        "the right pane must never receive a gesture it didn't start"
    );
}

/// A gesture that STARTS over the dead-space gap (no consumer) is captured by
/// nobody and delivered to nobody for its whole lifetime — even as it drifts
/// over a pane that would otherwise consume it.
#[test]
fn gesture_over_dead_space_delivers_to_neither() {
    let (mut app, left, right) = app_with_two_panes();

    // Centroid at screen x=200 → world x=200, in the gap (150,250).
    two_fingers_at(&mut app, 200.0, 100.0);
    paint(&mut app);
    assert_eq!(left.get(), 0);
    assert_eq!(right.get(), 0);

    // Drift over the left pane: capture was never established, so no delivery.
    move_two_fingers_to(&mut app, 75.0, 100.0);
    paint(&mut app);
    assert_eq!(
        left.get(),
        0,
        "no capture means no delivery for this gesture"
    );
    assert_eq!(right.get(), 0);
}
