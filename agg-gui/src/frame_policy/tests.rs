//! Tests for [`crate::frame_policy`]: the layout key, the needs-layout rule,
//! the paint decision (the cases the native and web shells pinned in their
//! own copies before they delegated here), and the headless tick.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use super::*;
use crate::{animation, DrawCtx, Event, EventResult, Rect, Widget};

/// Root that counts its layouts.
struct Counting {
    bounds: Rect,
    layouts: Rc<Cell<u32>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Counting {
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
        self.layouts.set(self.layouts.get() + 1);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn counting_app() -> (App, Rc<Cell<u32>>) {
    let layouts = Rc::new(Cell::new(0));
    let app = App::new(Box::new(Counting {
        bounds: Rect::default(),
        layouts: Rc::clone(&layouts),
        children: Vec::new(),
    }));
    (app, layouts)
}

fn quiet() {
    animation::take_layout_request();
    animation::clear_draw_request();
}

/// A pending `request_layout` forces layout even when the skip key is
/// unchanged; without one an unchanged key still skips.
#[test]
fn layout_request_forces_layout_with_unchanged_key() {
    quiet();
    let key = LayoutKey::for_surface(10, 10);
    assert!(!frame_needs_layout(Some(key), key));
    assert!(frame_needs_layout(None, key));
    animation::request_layout();
    assert!(frame_needs_layout(Some(key), key));
    quiet();
    assert!(!frame_needs_layout(Some(key), key));
}

#[test]
fn layout_key_tracks_size_scale_and_epoch() {
    assert_ne!(
        LayoutKey::for_surface(100, 100),
        LayoutKey::for_surface(101, 100)
    );
    // Equal unless another test's wakeup moved the epoch between the reads.
    assert!((0..IDLE_ATTEMPTS)
        .any(|_| LayoutKey::for_surface(100, 100) == LayoutKey::new(100.0, 100.0)));
    let before = LayoutKey::for_surface(100, 100);
    animation::request_draw();
    assert_ne!(before, LayoutKey::for_surface(100, 100), "epoch moved");
    let scale = crate::device_scale();
    let at_scale = LayoutKey::for_surface(100, 100);
    crate::set_device_scale(scale * 2.0);
    assert_ne!(at_scale, LayoutKey::for_surface(100, 100), "scale moved");
    crate::set_device_scale(scale);
    quiet();
}

fn demand(continuous: bool, dirty: bool, app: bool, deadline: Option<Instant>) -> FrameDemand {
    FrameDemand {
        continuous,
        dirty,
        app_wants_draw: app,
        next_deadline: deadline,
        now: Instant::now(),
    }
}

#[test]
fn reactive_idle_does_not_paint() {
    assert!(!wants_frame(&demand(false, false, false, None)));
}

#[test]
fn continuous_always_paints() {
    assert!(wants_frame(&demand(true, false, false, None)));
}

#[test]
fn dirty_or_app_request_paints() {
    assert!(wants_frame(&demand(false, true, false, None)));
    assert!(wants_frame(&demand(false, false, true, None)));
}

#[test]
fn deadline_paints_only_once_due() {
    let now = Instant::now();
    let later = now + Duration::from_millis(500);
    let at = |deadline, now| FrameDemand {
        now,
        ..demand(false, false, false, Some(deadline))
    };
    assert!(!wants_frame(&at(later, now)));
    assert!(wants_frame(&at(now, now)));
    assert!(wants_frame(&at(now, later)));
}

/// Other tests' cross-thread wakeups (`animation::signal_async_state_change`,
/// which `ui_thread::run_on_idle` also sends) bump a process-wide counter that
/// every thread folds into its invalidation epoch and draw request — as a
/// real worker wakes a real shell. Assertions that need a quiet epoch take
/// their keys explicitly, or retry an idle check that such a wakeup hit.
const IDLE_ATTEMPTS: usize = 5;

#[test]
fn tracker_skips_an_unchanged_key() {
    quiet();
    let mut tracker = LayoutTracker::new();
    let key = LayoutKey::for_surface(50, 40);
    assert!(tracker.needs_layout(key), "nothing laid out yet");
    tracker.record(key);
    assert_eq!(tracker.last(), Some(key));
    assert!(!tracker.needs_layout(key));
    assert!(tracker.needs_layout(LayoutKey::for_surface(60, 40)));
    animation::request_layout();
    assert!(tracker.needs_layout(key), "a layout request forces one");
    quiet();
    tracker.reset();
    assert!(tracker.needs_layout(key), "reset forgets the last layout");
}

#[test]
fn tracker_lays_out_when_the_key_changes() {
    quiet();
    let (mut app, layouts) = counting_app();
    let mut tracker = LayoutTracker::new();
    assert!(tracker.layout_if_needed(&mut app, Size::new(50.0, 40.0)));
    assert_eq!(layouts.get(), 1);
    assert!(tracker.layout_if_needed(&mut app, Size::new(60.0, 40.0)));
    assert_eq!(layouts.get(), 2);
    tracker.reset();
    quiet();
    assert!(tracker.layout_if_needed(&mut app, Size::new(60.0, 40.0)));
    assert_eq!(layouts.get(), 3);
}

#[test]
fn reactive_tick_paints_only_when_the_app_wants_to_draw() {
    quiet();
    let (mut app, layouts) = counting_app();
    let mut tracker = LayoutTracker::new();
    let viewport = Size::new(50.0, 40.0);
    let paints = Cell::new(0);
    // A forced tick paints and lays out the first frame.
    assert!(tracker.tick(&mut app, viewport, TickMode::Forced, |_| {
        paints.set(paints.get() + 1)
    }));
    assert_eq!((layouts.get(), paints.get()), (1, 1));
    let idle_tick_skipped = (0..IDLE_ATTEMPTS).any(|_| {
        // Fold in any wakeup already pending, then clear it.
        app.wants_draw();
        quiet();
        !tracker.tick(&mut app, viewport, TickMode::Reactive, |_| {})
    });
    assert!(idle_tick_skipped, "an idle app is not painted");
    animation::request_draw();
    let before = (layouts.get(), paints.get());
    assert!(tracker.tick(&mut app, viewport, TickMode::Reactive, |_| {
        paints.set(paints.get() + 1)
    }));
    assert_eq!(paints.get(), before.1 + 1, "a draw request paints");
    assert_eq!(
        layouts.get(),
        before.0 + 1,
        "a draw request moves the invalidation epoch, so the frame lays out too"
    );
    quiet();
}

#[test]
fn tick_drains_the_ui_thread_queue_first() {
    quiet();
    let (mut app, _) = counting_app();
    let mut tracker = LayoutTracker::new();
    crate::ui_thread::run_on_idle(animation::request_draw);
    let painted = tracker.tick(&mut app, Size::new(10.0, 10.0), TickMode::Reactive, |_| {});
    assert!(
        painted,
        "work queued for the UI thread runs before the decision"
    );
    quiet();
}
