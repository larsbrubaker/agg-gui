//! A widget whose paint callback panics inside `App::paint` is contained
//! (C# hosts' paint catch blocks plus `GlStackBalance.ResetForFrame`; the
//! ported automation tests are in `agg-gui-automation`'s
//! `paint_exception_containment_tests.rs`).
//!
//! These pin the mechanism with agg-gui's own software context: the panic is
//! reported as `UnhandledOrigin::Paint`, the widgets after the faulty one
//! still paint, the context's save and layer stacks are back to their entry
//! depth whatever the faulty callback left open, and with no handler on the
//! thread the panic is re-raised rather than swallowed.

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use agg_gui::draw_ctx::PaintNesting;
use agg_gui::unhandled::{set_unhandled_handler, UnhandledOrigin, UnhandledReport};
use agg_gui::{App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, Widget};

/// Which paint callback of a [`Faulty`] widget panics.
#[derive(Clone, Copy)]
enum Fault {
    None,
    Paint,
    Overlay,
    Finish,
}

/// Leaves a save level and a layer open, then panics, in one callback.
struct Faulty {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    fault: Fault,
}

impl Faulty {
    fn blow_up(&self, ctx: &mut dyn DrawCtx) {
        ctx.save();
        ctx.push_layer(8.0, 8.0);
        ctx.save();
        panic!("faulty widget");
    }
}

impl Widget for Faulty {
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
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if matches!(self.fault, Fault::Paint) {
            self.blow_up(ctx);
        }
    }
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if matches!(self.fault, Fault::Overlay) {
            self.blow_up(ctx);
        }
    }
    fn finish_paint(&mut self, ctx: &mut dyn DrawCtx) {
        if matches!(self.fault, Fault::Finish) {
            self.blow_up(ctx);
        }
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Records the nesting it paints at and how often it painted.
struct Witness {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    painted: Rc<Cell<u32>>,
    nesting: Rc<Cell<PaintNesting>>,
}

impl Widget for Witness {
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
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.painted.set(self.painted.get() + 1);
        self.nesting.set(ctx.paint_nesting());
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

struct Frame {
    app: App,
    painted: Rc<Cell<u32>>,
    nesting: Rc<Cell<PaintNesting>>,
}

/// A root holding a faulty widget and, after it, a witness.
fn frame(fault: Fault) -> Frame {
    let painted = Rc::new(Cell::new(0));
    let nesting = Rc::new(Cell::new(PaintNesting::default()));
    let faulty = Faulty {
        bounds: Rect::new(0.0, 0.0, 32.0, 32.0),
        children: Vec::new(),
        fault,
    };
    let witness = Witness {
        bounds: Rect::new(32.0, 0.0, 32.0, 32.0),
        children: Vec::new(),
        painted: Rc::clone(&painted),
        nesting: Rc::clone(&nesting),
    };
    let root = Container {
        bounds: Rect::new(0.0, 0.0, 64.0, 64.0),
        children: vec![Box::new(faulty), Box::new(witness)],
    };
    let mut app = App::new(Box::new(root));
    app.layout(Size::new(64.0, 64.0));
    Frame {
        app,
        painted,
        nesting,
    }
}

/// A container that paints nothing and never faults.
struct Container {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Container {
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
    fn layout(&mut self, _available: Size) -> Size {
        for child in &mut self.children {
            let b = child.bounds();
            child.layout(Size::new(b.width, b.height));
            child.set_bounds(b);
        }
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn reports() -> (
    Rc<RefCell<Vec<UnhandledReport>>>,
    agg_gui::unhandled::UnhandledHandlerGuard,
) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let guard = set_unhandled_handler(move |report| sink.borrow_mut().push(report.clone()));
    (seen, guard)
}

fn assert_contained(fault: Fault) {
    let (seen, _guard) = reports();
    let mut f = frame(fault);
    let mut fb = Framebuffer::new(64, 64);
    let mut ctx = GfxCtx::new(&mut fb);
    f.app.paint(&mut ctx);

    let seen = seen.borrow();
    assert_eq!(seen.len(), 1, "one panic, one report");
    assert_eq!(seen[0].origin, UnhandledOrigin::Paint);
    assert_eq!(seen[0].message, "faulty widget");
    assert_eq!(
        ctx.paint_nesting(),
        PaintNesting::default(),
        "the faulty callback's save levels and layer are closed"
    );
    assert_eq!(
        f.painted.get(),
        1,
        "the widget after the faulty one still paints"
    );
    let baseline = frame(Fault::None);
    let mut baseline_app = baseline.app;
    let mut baseline_fb = Framebuffer::new(64, 64);
    baseline_app.paint(&mut GfxCtx::new(&mut baseline_fb));
    assert_eq!(
        f.nesting.get(),
        baseline.nesting.get(),
        "the next widget paints at the nesting a clean frame gives it, with nothing leaked"
    );
}

#[test]
fn rust_only_a_panicking_paint_is_reported_and_the_context_rebalanced() {
    assert_contained(Fault::Paint);
}

#[test]
fn rust_only_a_panicking_paint_overlay_is_reported_and_the_context_rebalanced() {
    assert_contained(Fault::Overlay);
}

#[test]
fn rust_only_a_panicking_finish_paint_is_reported_and_the_context_rebalanced() {
    assert_contained(Fault::Finish);
}

#[test]
fn rust_only_a_widget_that_panics_every_frame_leaks_nothing_across_frames() {
    let (seen, _guard) = reports();
    let mut f = frame(Fault::Paint);
    let mut fb = Framebuffer::new(64, 64);
    let mut ctx = GfxCtx::new(&mut fb);
    let mut entry_nestings = Vec::new();
    for _ in 0..50 {
        f.app.paint(&mut ctx);
        entry_nestings.push(f.nesting.get());
    }
    assert_eq!(seen.borrow().len(), 50);
    assert!(
        entry_nestings.windows(2).all(|w| w[0] == w[1]),
        "every frame's widgets paint at the same depth: {entry_nestings:?}"
    );
    assert_eq!(ctx.paint_nesting(), PaintNesting::default());
}

#[test]
fn rust_only_with_no_handler_a_paint_panic_is_re_raised() {
    let mut f = frame(Fault::Paint);
    let mut fb = Framebuffer::new(64, 64);
    let mut ctx = GfxCtx::new(&mut fb);
    let payload = catch_unwind(AssertUnwindSafe(|| f.app.paint(&mut ctx)))
        .expect_err("nothing took the report, so the panic escapes");
    assert_eq!(
        agg_gui::unhandled::panic_message(payload.as_ref()),
        "faulty widget"
    );
    assert_eq!(
        f.painted.get(),
        0,
        "with nobody to report to, the frame is abandoned as it was before containment"
    );
}
