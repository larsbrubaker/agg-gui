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

/// Make this test thread a UI thread on a queue of its own before it drains
/// (a drain binds the thread, adopting the process's main queue when no live
/// UI thread owns it — and with it every unbound thread's posts and signals).
fn own_ui_queue() {
    crate::ui_thread::UiQueue::new().attach_current_thread();
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
    assert_eq!(
        LayoutKey::for_surface(100, 100),
        LayoutKey::new(100.0, 100.0)
    );
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

/// A child that asks to be drawn while its flag is set, as a widget whose
/// animation steps in `layout` does.
struct AsksToBeDrawn {
    bounds: Rect,
    asks: Rc<Cell<bool>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for AsksToBeDrawn {
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
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn needs_draw(&self) -> bool {
        self.asks.get()
    }
}

#[test]
fn a_frame_a_widget_asks_for_lays_the_tree_out_and_leaves_the_epoch_alone() {
    quiet();
    let (mut app, layouts) = counting_app();
    let asks = Rc::new(Cell::new(false));
    app.root_mut().children_mut().push(Box::new(AsksToBeDrawn {
        bounds: Rect::default(),
        asks: Rc::clone(&asks),
        children: Vec::new(),
    }));
    let viewport = Size::new(50.0, 40.0);
    let key = || LayoutKey::for_viewport(viewport);
    let mut tracker = LayoutTracker::new();
    assert!(tracker.layout_if_needed(&mut app, viewport));
    quiet();
    assert!(!tracker.needs_layout_for(&app, key()), "nothing asks");
    assert!(!tracker.layout_if_needed(&mut app, viewport));

    asks.set(true);
    let epoch = animation::invalidation_epoch();
    let before = layouts.get();
    assert!(
        tracker.needs_layout_for(&app, key()),
        "a widget asking to be drawn is laid out with its frame"
    );
    assert!(tracker.layout_if_needed(&mut app, viewport));
    assert_eq!(layouts.get(), before + 1);
    assert_eq!(
        animation::invalidation_epoch(),
        epoch,
        "such a frame leaves the invalidation epoch, and so every retained layer, alone"
    );
    assert!(
        !tracker.needs_layout(key()),
        "the layout key alone does not ask for it"
    );

    asks.set(false);
    assert!(!tracker.layout_if_needed(&mut app, viewport));
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
    own_ui_queue();
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
    quiet();
    assert!(
        !tracker.tick(&mut app, viewport, TickMode::Reactive, |_| {}),
        "an idle app is not painted"
    );
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
    own_ui_queue();
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

/// The headless tick never paints for a scheduled draw withdrawn before its
/// time (a watchdog whose work finished early), and still paints for the
/// next one when that comes due.
#[test]
fn reactive_tick_skips_a_cancelled_scheduled_draw() {
    own_ui_queue();
    let _clock = crate::clock::scoped_virtual(None);
    animation::reset_draw_signals();
    quiet();
    let (mut app, _) = counting_app();
    let mut tracker = LayoutTracker::new();
    let viewport = Size::new(10.0, 10.0);
    let paints = Cell::new(0);
    // What `App::paint` does to the draw signals, without a draw context.
    let paint = |_: &mut App| {
        paints.set(paints.get() + 1);
        animation::clear_draw_request();
    };
    let watchdog = animation::schedule_draw_after(Duration::from_millis(100));
    let blink = animation::schedule_draw_after(Duration::from_millis(500));
    assert_eq!(app.next_draw_deadline(), Some(watchdog.deadline()));
    watchdog.cancel();
    assert_eq!(app.next_draw_deadline(), Some(blink.deadline()));

    crate::clock::advance(Duration::from_millis(200));
    assert!(
        !tracker.tick(&mut app, viewport, TickMode::Reactive, paint),
        "the withdrawn deadline passed without a paint"
    );
    crate::clock::advance(Duration::from_millis(300));
    assert!(tracker.tick(&mut app, viewport, TickMode::Reactive, paint));
    assert_eq!(paints.get(), 1);
    assert_eq!(app.next_draw_deadline(), None, "the served draw is gone");
    quiet();
}
