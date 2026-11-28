//! `Widget::depends_on_async_state` on the retained GL layer path
//! (`paint_subtree_gl_backbuffer` in `widget/paint/offscreen.rs`): a layer
//! whose widget answers `false` is composited as it is after an async-state
//! change, while a default one is painted again.
//!
//! Uses the retained-layer test context from `retained_layers.rs`, which keeps
//! a layer as a GPU backend would. Each test thread is a UI thread on a queue
//! of its own, and the worker signals through that queue's handle, so no other
//! test's wake reaches it.

use std::cell::Cell;
use std::rc::Rc;

use super::retained_layers::RetainedLayerCtx;
use crate::widget::{paint_subtree, BackbufferKind, BackbufferSpec, BackbufferState, Widget};
use crate::{DrawCtx, Event, EventResult, Rect, Size};

/// A retained GL layer that counts its paints and answers `depends` to
/// `depends_on_async_state`.
struct LayerProbe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    backbuffer: BackbufferState,
    paints: Rc<Cell<usize>>,
    depends: bool,
}

impl Widget for LayerProbe {
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
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        self.paints.set(self.paints.get() + 1);
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn backbuffer_spec(&mut self) -> BackbufferSpec {
        BackbufferSpec {
            kind: BackbufferKind::GlFbo,
            cached: true,
            alpha: 1.0,
            outsets: crate::Insets::ZERO,
            rounded_clip: None,
        }
    }
    fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
        Some(&mut self.backbuffer)
    }
    fn depends_on_async_state(&self) -> bool {
        self.depends
    }
}

/// The calling test thread as a UI thread on a fresh queue of its own until
/// dropped (also when an assertion fails).
struct OwnUiThread;

impl OwnUiThread {
    fn new() -> Self {
        crate::ui_thread::mark_current_thread_as_ui_thread_with_own_queue();
        OwnUiThread
    }
}

impl Drop for OwnUiThread {
    fn drop(&mut self) {
        crate::ui_thread::unbind_current_thread();
    }
}

/// Paints of a `depends` layer: once, once more with nothing changed, and
/// after a worker's wake on this thread's own queue.
fn paints_after_a_wake(depends: bool) -> usize {
    let _ui = OwnUiThread::new();
    let paints = Rc::new(Cell::new(0));
    let mut widget = LayerProbe {
        bounds: Rect::new(0.0, 0.0, 20.0, 20.0),
        children: Vec::new(),
        backbuffer: BackbufferState::new(),
        paints: Rc::clone(&paints),
        depends,
    };
    let mut ctx = RetainedLayerCtx::new();
    paint_subtree(&mut widget, &mut ctx);
    paint_subtree(&mut widget, &mut ctx);
    assert_eq!(paints.get(), 1, "setup: a clean retained layer is reused");

    let queue = crate::ui_thread::current_queue();
    std::thread::spawn(move || queue.signal_async_state_change())
        .join()
        .expect("signalling worker");
    paint_subtree(&mut widget, &mut ctx);
    // Composited as it is from then on, with nothing else changed.
    paint_subtree(&mut widget, &mut ctx);
    paints.get()
}

#[test]
fn an_opted_out_retained_layer_ignores_the_async_state_epoch() {
    assert_eq!(
        paints_after_a_wake(false),
        1,
        "a layer whose widget answers false is composited, not painted, after a wake"
    );
}

#[test]
fn a_default_retained_layer_repaints_after_an_async_state_change() {
    assert_eq!(
        paints_after_a_wake(true),
        2,
        "a default layer is painted once more after a wake, then reused"
    );
}
