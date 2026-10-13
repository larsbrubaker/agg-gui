//! Port of agg-sharp `Tests/Agg.Tests/Agg Automation Tests/PaintExceptionContainmentTests.cs`:
//! what a widget that panics while drawing may cost. One frame, and a
//! report - never the window's event loop.
//!
//! C# runs its own `SystemWindow` (no `AutomationRunner`, so no harness
//! watching UI-thread exceptions) and listens on `UiThread.UnhandledException`
//! itself; here the window is a [`HeadlessDriver`] under an attached
//! [`AutomationRunner`] on the test's own thread, and the listener is the
//! thread's `agg_gui::set_unhandled_handler`. Where C# shows the window from a
//! worker and polls with `Task.Delay`, the body pumps frames with
//! `wait_for` on the driver's virtual clock.
//!
//! Translations:
//! - C#'s `GlStackBalance` throws "PushMatrix being called without matching
//!   PopMatrix" once 32 pushes leak. agg-gui's contexts keep no such guard, so
//!   the leaked level is the widget's `ctx.save()`. The proof that it did not
//!   outlive its frame is a `Bystander` drawn after the thrower: it is entered
//!   at the thrower's own depth in each of `MAX_MATRIX_DEPTH + 8` throwing
//!   frames (see `agg-gui/tests/paint_containment.rs` for the same mechanism
//!   pinned on a context that outlives its frames).
//! - "Showing" the window is its first forced frame.
//! - C#'s widget stretches to the window with anchors; the root here keeps its
//!   children where they are placed, so the widget is given the window's size.
//!   `RepaintAfterThrowing` is the same `RunOnIdle(Invalidate)`.
//! - "The window still closes" is the driver still taking a close request.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agg_gui::draw_ctx::PaintNesting;
use agg_gui::unhandled::{set_unhandled_handler, UnhandledReport};
use agg_gui::{animation, ui_thread, DrawCtx, Event, EventResult, Rect, Size, Widget};
use agg_gui_automation::{
    AutomationConfig, AutomationRunner, FrameKind, HeadlessDriver, ProbeWidget, UiDriver,
};

/// C# `GlStackBalance.MaxMatrixDepth`: how deep the matrix stack may go.
const MAX_MATRIX_DEPTH: usize = 32;

const THROW_MESSAGE: &str =
    "A widget threw while drawing, deliberately (PaintExceptionContainmentTests).";

/// What the test reads back from the [`ThrowOnDrawWidget`] it hands to the tree.
#[derive(Default)]
struct ThrowerState {
    throw_count: Cell<usize>,
    clean_draw_count: Cell<usize>,
    throwing: Cell<bool>,
    /// The `save` depth the widget was entered at, per draw.
    entry_nesting: RefCell<Vec<PaintNesting>>,
}

/// A widget whose draw always throws - the shape of the bug this pins.
struct ThrowOnDrawWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    state: Rc<ThrowerState>,
    /// Open a `save` level before throwing and never close it - what a 3D
    /// view's draw does when the throw comes from inside its scene pass.
    leak_a_push_before_throwing: bool,
    /// Ask for another frame from inside the draw, so the throw repeats
    /// without any input.
    repaint_after_throwing: bool,
}

impl ThrowOnDrawWidget {
    fn new(bounds: Rect) -> (Self, Rc<ThrowerState>) {
        let state = Rc::new(ThrowerState {
            throwing: Cell::new(true),
            ..ThrowerState::default()
        });
        (
            Self {
                bounds,
                children: Vec::new(),
                state: Rc::clone(&state),
                leak_a_push_before_throwing: false,
                repaint_after_throwing: false,
            },
            state,
        )
    }
}

impl Widget for ThrowOnDrawWidget {
    fn type_name(&self) -> &'static str {
        "ThrowOnDrawWidget"
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

    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.state
            .entry_nesting
            .borrow_mut()
            .push(ctx.paint_nesting());

        if !self.state.throwing.get() {
            self.state
                .clean_draw_count
                .set(self.state.clean_draw_count.get() + 1);
            return;
        }

        if self.repaint_after_throwing {
            ui_thread::run_on_idle(animation::request_draw);
        }

        if self.leak_a_push_before_throwing {
            ctx.save();
        }

        self.state.throw_count.set(self.state.throw_count.get() + 1);

        panic!("{THROW_MESSAGE}");
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A sibling drawn after the thrower, recording the depth it is entered at.
/// A level the thrower leaked would make it deeper than the thrower was, and
/// a frame that died at the thrower would never reach it.
struct Bystander {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    entry_nesting: Rc<RefCell<Vec<PaintNesting>>>,
}

impl Widget for Bystander {
    fn type_name(&self) -> &'static str {
        "Bystander"
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

    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.entry_nesting.borrow_mut().push(ctx.paint_nesting());
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// A 300 x 200 window holding `thrower` and, drawn after it, a [`Bystander`].
fn window(thrower: ThrowOnDrawWidget) -> (HeadlessDriver, Rc<RefCell<Vec<PaintNesting>>>) {
    let bystander_entries = Rc::new(RefCell::new(Vec::new()));
    let root = ProbeWidget::new("window")
        .with_bounds(Rect::new(0.0, 0.0, 300.0, 200.0))
        .with_child(Box::new(thrower))
        .with_child(Box::new(Bystander {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
            children: Vec::new(),
            entry_nesting: Rc::clone(&bystander_entries),
        }));
    let mut driver = HeadlessDriver::new(Box::new(root), 300, 200);
    // Showing the window paints its first frame (the run's bring-up does the same).
    driver.pump(FrameKind::Forced);
    (driver, bystander_entries)
}

/// A listener on the thread's unhandled-exception channel (C#
/// `UiThread.UnhandledException += CollectReport`).
fn collect_reports() -> (
    Rc<RefCell<Vec<UnhandledReport>>>,
    agg_gui::unhandled::UnhandledHandlerGuard,
) {
    let reported = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&reported);
    let guard = set_unhandled_handler(move |report| sink.borrow_mut().push(report.clone()));
    (reported, guard)
}

#[test]
fn a_throwing_draw_is_reported_and_the_event_loop_keeps_running() {
    let (reported, _listener) = collect_reports();
    let (thrower, state) = ThrowOnDrawWidget::new(Rect::new(0.0, 0.0, 300.0, 200.0));
    let (driver, _) = window(thrower);
    let mut runner = AutomationRunner::attach(driver, AutomationConfig::default());

    // Wait for the throw to have happened, not for a length of time.
    runner.wait_for(|_| state.throw_count.get() > 0, 20.0, 10);

    assert!(
        state.throw_count.get() > 0,
        "the widget under test has to have been drawn, or nothing has been proved"
    );

    // The loop is alive if it still runs queued work. This is the part the
    // failure took away: with the paint unwinding the loop, nothing ever
    // drained the idle queue again.
    let idle_ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&idle_ran);
    ui_thread::run_on_idle(move || flag.store(true, Ordering::SeqCst));

    runner.wait_for(|_| idle_ran.load(Ordering::SeqCst), 20.0, 10);

    assert!(
        idle_ran.load(Ordering::SeqCst),
        "a paint that threw must leave the window's event loop pumping - queued work still has to run"
    );

    assert!(
        !reported.borrow().is_empty(),
        "the exception has to be reported, not swallowed: it is what fails the test that caused it"
    );

    // And the window still closes - a loop that survived the throw is the
    // only thing that can service this.
    runner.driver_mut().request_close();
    assert!(
        runner.driver_mut().take_close_request(),
        "the window has to close after a throwing paint; a dead loop leaves it on screen forever"
    );
}

#[test]
fn a_draw_that_throws_every_frame_for_longer_than_the_matrix_stack_is_deep_still_costs_only_its_own_frames(
) {
    // MatterCAD on X11: the scene renderer's submit failed wgpu validation
    // every frame, and the throw unwound out from between the 3D view's
    // PushMatrix and its PopMatrix. Each abandoned frame leaked those pushes,
    // and after 32 of them the depth guard in GL threw at the host's own first
    // push - before any widget drew, every frame, forever. The window went
    // black and the close-confirmation dialog it needed a click on could not
    // be drawn. This widget reproduces the leak; the loop, the reports and the
    // close are what have to survive it.
    let (reported, _listener) = collect_reports();
    let (mut thrower, state) = ThrowOnDrawWidget::new(Rect::new(0.0, 0.0, 300.0, 200.0));
    thrower.leak_a_push_before_throwing = true;
    thrower.repaint_after_throwing = true;
    let (driver, bystander_entries) = window(thrower);
    let mut runner = AutomationRunner::attach(driver, AutomationConfig::default());

    let enough_to_trip_the_guard = MAX_MATRIX_DEPTH + 8;
    runner.wait_for(
        |_| state.throw_count.get() >= enough_to_trip_the_guard,
        30.0,
        10,
    );

    assert!(
        state.throw_count.get() >= enough_to_trip_the_guard,
        "the widget has to keep being drawn, and throwing, past the depth of the matrix stack: a frame that dies before the widget draws does not count it"
    );

    let seen: Vec<UnhandledReport> = reported.borrow().clone();
    assert!(!seen.is_empty());
    assert_eq!(
        seen.iter()
            .filter(|report| report.message != THROW_MESSAGE)
            .count(),
        0,
        "every reported exception has to be the widget's own; anything else means the leaked pushes outlived their frame"
    );
    // The sibling after the thrower is entered at the depth the thrower was,
    // frame after frame: nothing the thrower left open reached it.
    assert_eq!(
        *bystander_entries.borrow(),
        *state.entry_nesting.borrow(),
        "the leaked save levels outlived their frames, or a frame died before the widgets after the thrower"
    );

    // Once the widget stops throwing, the next frame is an ordinary one.
    state.throwing.set(false);
    runner.wait_for(|_| state.clean_draw_count.get() > 0, 20.0, 10);
    assert!(
        state.clean_draw_count.get() > 0,
        "a frame after the throwing stopped has to draw all the way through"
    );

    runner.driver_mut().request_close();
    assert!(
        runner.driver_mut().take_close_request(),
        "the window has to close after frames of leaked pushes; that it could not is the bug"
    );
}
