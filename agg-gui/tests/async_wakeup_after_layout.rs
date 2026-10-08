//! Regression coverage for a cross-thread async wakeup landing *between*
//! `App::layout` and `App::paint`.
//!
//! The paint's async-state dirty walk re-rasterises retained backbuffers,
//! but the layout for this frame already ran against the old state — a
//! freshly-decoded image would paint into the placeholder rect the previous
//! layout reserved.  `App` therefore records the async-state epoch it laid
//! out against and, when the paint observes a newer one, requests exactly one
//! follow-up draw so the next frame re-lays-out.  These tests pin both halves:
//! the follow-up happens, and it cannot turn into a perpetual redraw loop.
//!
//! Integration-test binary (own process) so the main queue's wakeup count,
//! which these unbound test threads and their workers share, is not shared
//! with the unit-test binary's signalling tests; the tests here serialize on
//! a local mutex so they cannot signal into each other.

use std::sync::Mutex;

use agg_gui::animation::{clear_draw_request, signal_async_state_change, wants_draw};
use agg_gui::{App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, Widget};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|p| p.into_inner())
}

/// Inert leaf: never requests a draw on its own, so any `wants_draw()` after
/// a paint comes from `App` itself.
struct Leaf {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Leaf {
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
}

const VIEWPORT: Size = Size {
    width: 64.0,
    height: 64.0,
};

fn paint(app: &mut App) {
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

/// An idle app: laid out and painted once, nothing pending.
fn settled_app() -> App {
    let mut app = App::new(Box::new(Leaf {
        bounds: Rect::default(),
        children: Vec::new(),
    }));
    clear_draw_request();
    app.layout(VIEWPORT);
    paint(&mut app);
    assert!(!wants_draw(), "setup: a settled app must be idle");
    app
}

fn signal_from_worker() {
    std::thread::spawn(signal_async_state_change)
        .join()
        .expect("signal thread");
}

/// The race: signal after this frame's layout, before its paint.  The paint
/// must ask for one more frame so the next layout sees the new state.
#[test]
fn signal_after_layout_requests_a_follow_up_frame() {
    let _guard = serial();
    let mut app = settled_app();

    app.layout(VIEWPORT);
    signal_from_worker();
    paint(&mut app);

    assert!(
        wants_draw(),
        "a signal that arrived after layout must request a follow-up frame \
         so the next frame re-lays-out against the new async state"
    );
}

/// The follow-up frame (layout + paint) returns the app to idle.
#[test]
fn follow_up_frame_goes_idle() {
    let _guard = serial();
    let mut app = settled_app();

    app.layout(VIEWPORT);
    signal_from_worker();
    paint(&mut app);
    assert!(wants_draw(), "precondition: follow-up requested");

    app.layout(VIEWPORT);
    paint(&mut app);
    assert!(
        !wants_draw(),
        "after the follow-up frame the app must go idle (no redraw loop)"
    );
}

/// Even a host that repaints without re-laying-out gets only ONE follow-up
/// per signal — the request must not repeat on every paint.
#[test]
fn follow_up_is_requested_once_per_signal() {
    let _guard = serial();
    let mut app = settled_app();

    app.layout(VIEWPORT);
    signal_from_worker();
    paint(&mut app);
    assert!(wants_draw(), "precondition: follow-up requested");

    paint(&mut app);
    assert!(
        !wants_draw(),
        "a second paint without a new signal must not request another frame"
    );
}

/// A signal that lands *before* layout is already reflected in that layout,
/// so no follow-up frame is needed.
#[test]
fn signal_before_layout_needs_no_follow_up() {
    let _guard = serial();
    let mut app = settled_app();

    signal_from_worker();
    app.layout(VIEWPORT);
    paint(&mut app);

    assert!(
        !wants_draw(),
        "a signal the layout already saw must not cost an extra frame"
    );
}
