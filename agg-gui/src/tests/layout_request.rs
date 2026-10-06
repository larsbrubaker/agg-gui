//! End-to-end coverage for [`request_layout`](crate::animation::request_layout)
//! through the real `App::layout` / `App::paint` pair.
//!
//! A widget that has more work for its next layout pass (mattercad's design
//! page pumps its model executor from `layout` while a rebuild runs) must get
//! another laid-out frame even though `App::paint` clears the immediate draw
//! request.  The frame loop here mirrors the shells: lay out when the
//! layout-skip key changed *or* a layout is requested, then paint, then keep
//! drawing while `App::wants_draw()`.

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::{
    clear_draw_request, invalidation_epoch, layout_requested, request_layout, take_layout_request,
};
use crate::{App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, Widget};

/// Leaf that requests another layout on every pass numbered below
/// `request_in_layout_until` (and once from `paint` when `request_in_paint` is
/// set), counting the layout passes it receives.
struct Pump {
    bounds: Rect,
    layouts: Rc<Cell<usize>>,
    request_in_layout_until: usize,
    request_in_paint: Rc<Cell<bool>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for Pump {
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
        let n = self.layouts.get() + 1;
        self.layouts.set(n);
        if n < self.request_in_layout_until {
            request_layout();
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        if self.request_in_paint.replace(false) {
            request_layout();
        }
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

const VIEWPORT: Size = Size {
    width: 64.0,
    height: 64.0,
};

/// One shell-style frame. `last_epoch` is the layout-skip key (the shells also
/// key on size and scale, which are constant here).
fn frame(app: &mut App, last_epoch: &mut Option<u64>) {
    let key = invalidation_epoch();
    if *last_epoch != Some(key) || layout_requested() {
        app.layout(VIEWPORT);
    }
    *last_epoch = Some(key);
    let mut fb = Framebuffer::new(VIEWPORT.width as u32, VIEWPORT.height as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    app.paint(&mut ctx);
}

fn app_with(layouts: &Rc<Cell<usize>>, until: usize, in_paint: &Rc<Cell<bool>>) -> App {
    take_layout_request();
    clear_draw_request();
    App::new(Box::new(Pump {
        bounds: Rect::default(),
        layouts: Rc::clone(layouts),
        request_in_layout_until: until,
        request_in_paint: Rc::clone(in_paint),
        children: Vec::new(),
    }))
}

/// A request made during layout survives the paint's clear, keeps the host
/// drawing, and yields exactly one more layout pass per request; once the
/// widget stops asking, the loop goes idle.
#[test]
fn request_during_layout_yields_another_layout_pass() {
    let layouts = Rc::new(Cell::new(0));
    let in_paint = Rc::new(Cell::new(false));
    // Requests on passes 1 and 2; pass 3 makes no request.
    let mut app = app_with(&layouts, 3, &in_paint);
    let mut last = None;

    frame(&mut app, &mut last);
    assert_eq!(layouts.get(), 1);
    assert!(
        layout_requested(),
        "the request outlives App::paint's clear"
    );
    assert!(app.wants_draw(), "a pending layout keeps the host drawing");

    frame(&mut app, &mut last);
    assert_eq!(layouts.get(), 2, "the request earned a second layout pass");
    assert!(app.wants_draw());

    frame(&mut app, &mut last);
    assert_eq!(layouts.get(), 3);
    assert!(!layout_requested(), "pass 3 made no request");
    assert!(!app.wants_draw(), "the loop goes idle once work is done");
}

/// A request made during paint (after this frame's layout) is not consumed by
/// that frame and lays out the next one.
#[test]
fn request_during_paint_lays_out_next_frame() {
    let layouts = Rc::new(Cell::new(0));
    let in_paint = Rc::new(Cell::new(false));
    let mut app = app_with(&layouts, 0, &in_paint);
    let mut last = None;

    frame(&mut app, &mut last);
    assert_eq!(layouts.get(), 1);
    assert!(!app.wants_draw(), "idle with no request");

    in_paint.set(true);
    frame(&mut app, &mut last);
    // Idle frame: key unchanged, no request pending at its start -> no layout.
    assert_eq!(layouts.get(), 1);
    assert!(
        layout_requested(),
        "the paint-time request is still pending"
    );
    assert!(app.wants_draw());

    frame(&mut app, &mut last);
    assert_eq!(
        layouts.get(),
        2,
        "the paint-time request laid out this frame"
    );
    assert!(!app.wants_draw());
}
