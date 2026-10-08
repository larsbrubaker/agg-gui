//! Root-space geometry for the widget handling the current event.
//!
//! While [`dispatch_event`](super::dispatch_event) (and its `_dyn`,
//! broadcast and exact-delivery siblings in [`tree`](super::tree)) walks a
//! path down the tree, it keeps a thread-local stack of "local → root"
//! transforms, one per level, so a widget's `on_event` can ask where it sits
//! in root (window) coordinates without having recorded anything at paint
//! time. A button that opens a popup anchored to itself calls
//! [`event_rect_to_root`] with its own `(0, 0, w, h)` from inside the click
//! handler and gets the rect a root-level popup layer needs, wherever the
//! button sits in the tree.
//!
//! # Coordinate system
//!
//! Root coordinates are the App root's **logical Y-up** space — the same
//! space the App passes as `pos_in_root` — not device pixels (contrast with
//! [`DrawCtx::root_transform`](crate::draw_ctx::DrawCtx::root_transform),
//! which carries the device scale). Each level composes the child's
//! `bounds()` offset and its parent's
//! [`Widget::child_transform`](super::Widget::child_transform), exactly as
//! hit-testing maps positions down.
//!
//! A dispatch that starts while another is already running (a widget
//! routing an event into a sub-tree it owns outside `children()`, such as a
//! window's title bar) places that sub-root at its `bounds()` origin inside
//! the calling widget's local space — the same convention its callers use
//! when they localise the position they pass in.

use std::cell::RefCell;

use super::Widget;
use crate::geometry::{Point, Rect};
use crate::TransAffine;

std::thread_local! {
    static EVENT_ROOT_STACK: RefCell<Vec<TransAffine>> = const { RefCell::new(Vec::new()) };
}

/// The transform from the local space of the widget whose `on_event` is
/// running to the App root's logical space, or `None` when no event is being
/// dispatched through the tree.
pub fn event_root_transform() -> Option<TransAffine> {
    EVENT_ROOT_STACK.with(|s| s.borrow().last().copied())
}

/// The pointer in the local space of the widget whose `on_event` is running
/// (agg-sharp's `MouseEventArgs.Position`), or `None` outside event dispatch
/// or before the pointer has been seen. Events that carry no position —
/// `MouseEnter`/`MouseLeave` (agg-sharp's `MouseEnterBounds`/
/// `MouseLeaveBounds`, which do carry one), `MouseOver`/`MouseOut` — read it
/// here, so a widget built under a still pointer learns where the pointer is
/// from its enter alone.
pub fn event_pointer_local() -> Option<Point> {
    let to_root = event_root_transform()?;
    let world = super::tree_inspector::current_mouse_world()?;
    let (mut x, mut y) = (world.x, world.y);
    to_root.inverse_transform(&mut x, &mut y);
    Some(Point::new(x, y))
}

/// The root-space axis-aligned bounds of `local` (a rect in the local space
/// of the widget handling the current event), or `None` outside event
/// dispatch. Pass `Rect::new(0.0, 0.0, w, h)` for the widget's own bounds.
pub fn event_rect_to_root(local: Rect) -> Option<Rect> {
    event_root_transform().map(|t| transform_rect(&t, local))
}

/// Axis-aligned bounds of `r` under `t`.
pub(crate) fn transform_rect(t: &TransAffine, r: Rect) -> Rect {
    let mut pts = [
        (r.x, r.y),
        (r.x + r.width, r.y),
        (r.x, r.y + r.height),
        (r.x + r.width, r.y + r.height),
    ];
    for (x, y) in &mut pts {
        t.transform(x, y);
    }
    let (mut x0, mut y0) = (f64::INFINITY, f64::INFINITY);
    let (mut x1, mut y1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in pts {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// Pops its stack entry on drop, so a panicking handler (caught by a test
/// harness) cannot leave a stale transform behind.
pub(crate) struct EventRootGuard(());

impl Drop for EventRootGuard {
    fn drop(&mut self) {
        EVENT_ROOT_STACK.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

fn push(t: TransAffine) -> EventRootGuard {
    EVENT_ROOT_STACK.with(|s| s.borrow_mut().push(t));
    EventRootGuard(())
}

/// Enter a dispatch whose root is `root`. The outermost dispatch maps the
/// root's local space to itself; a nested one (started from inside a
/// handler) places `root` at its `bounds()` origin in the caller's space.
pub(crate) fn enter_dispatch(root: &dyn Widget) -> EventRootGuard {
    let t = match event_root_transform() {
        None => TransAffine::new(),
        Some(caller) => {
            let b = root.bounds();
            let mut t = TransAffine::new_translation(b.x, b.y);
            t.multiply(&caller);
            t
        }
    };
    push(t)
}

/// Descend from `parent` (whose transform is on top of the stack) into the
/// child whose bounds are `child_bounds`.
pub(crate) fn enter_child(parent: &dyn Widget, child_bounds: Rect) -> EventRootGuard {
    let mut t = TransAffine::new_translation(child_bounds.x, child_bounds.y);
    if let Some(ct) = parent.child_transform() {
        t.multiply(&ct);
    }
    if let Some(parent_to_root) = event_root_transform() {
        t.multiply(&parent_to_root);
    }
    push(t)
}
