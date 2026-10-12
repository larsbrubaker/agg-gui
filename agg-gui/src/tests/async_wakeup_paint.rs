//! Regression coverage for a cross-thread async wakeup racing `App::paint`.
//!
//! A background worker (image fetch + decode) calls
//! [`signal_async_state_change`](crate::animation::signal_async_state_change),
//! which bumps the wakeup counter of the UI thread's queue; that thread merges
//! it into its thread-local epochs on the next `wants_draw` / epoch read.  `App::paint`
//! starts with `clear_draw_request()`, which previously marked that counter
//! as seen *without* merging it — so a signal landing after the host's last
//! `wants_draw()` but before the paint was swallowed: the paint's
//! async-state dirty walk never fired (retained backbuffers composited stale
//! pixels) and no later frame was requested.
//!
//! Lives in `crate::tests` so it drives the real `App::paint` through the
//! headless `GfxCtx`/`Framebuffer` harness.

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::{clear_draw_request, wants_draw};
use crate::ui_thread::{self, UiQueue};
use crate::{App, DrawCtx, Event, EventResult, Framebuffer, GfxCtx, Rect, Size, Widget};

/// Leaf that counts `mark_dirty` calls — the observable effect of
/// `App::paint`'s async-state dirty walk.
struct DirtyCounter {
    bounds: Rect,
    marks: Rc<Cell<usize>>,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for DirtyCounter {
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
    fn mark_dirty(&mut self) {
        self.marks.set(self.marks.get() + 1);
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

/// A worker-thread signal that arrives after the host's last `wants_draw()`
/// read but before `App::paint` must still reach that paint's async-state
/// dirty walk, so retained backbuffers re-rasterise the new content.
///
/// This thread is a UI thread on a queue of its own, and the worker signals
/// through that queue's handle, so no other test's signal reaches it.
#[test]
fn signal_between_wants_draw_and_paint_reaches_the_paint() {
    UiQueue::new().attach_current_thread();
    let marks = Rc::new(Cell::new(0));
    let root = DirtyCounter {
        bounds: Rect::default(),
        marks: Rc::clone(&marks),
        children: Vec::new(),
    };
    let mut app = App::new(Box::new(root));
    app.layout(VIEWPORT);

    // Settle: one paint syncs the app's async epoch with this thread's.
    clear_draw_request();
    paint(&mut app);
    marks.set(0);

    // The host's end-of-loop read — it pumps everything pending so far.
    let _ = wants_draw();

    // The race: the fetch completes on a worker thread right now.
    let queue = ui_thread::current_queue();
    std::thread::spawn(move || queue.signal_async_state_change())
        .join()
        .expect("signal thread");

    paint(&mut app);

    assert!(
        marks.get() > 0,
        "the paint must observe the cross-thread async signal (dirty walk), \
         not discard it in clear_draw_request"
    );
    ui_thread::unbind_current_thread();
}
