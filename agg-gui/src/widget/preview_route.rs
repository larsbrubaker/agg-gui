//! Where an event a widget is previewing goes next.
//!
//! Routed dispatch ([`dispatch_event`](super::dispatch_event) and its `_dyn`
//! sibling in [`tree`](super::tree)) offers an event to each ancestor's
//! [`Widget::preview_event`](super::Widget::preview_event) before it walks on
//! into one of that ancestor's children. While a preview runs,
//! [`previewed_child`] answers which child that is: the index the dispatcher
//! itself is about to descend into, from the path the App already chose (an
//! active modal, a global overlay under the pointer, or the ordinary hit).
//! A root widget that hosts layers over its main content can so tell a press
//! aimed at that content from one aimed at a dialog, popup or overlay above
//! it, without hit-testing the tree a second time (which could disagree with
//! the route the press really takes, or disturb widgets whose `hit_test`
//! remembers the press it was asked about).
//!
//! The answer is `None` outside a preview, during every `on_event` the
//! dispatch delivers (the target's and the bubbling ancestors', even in a
//! dispatch nested inside a preview), and when the path names a child the
//! previewing widget no longer has (a stale captured path, which the dispatch
//! stops at instead of entering). A dispatch started from inside a preview (a
//! widget routing into a sub-tree it owns) sets and then restores its own
//! answers, so the outer preview's is never clobbered, a panic unwinding out
//! of a preview included.

use std::cell::Cell;

std::thread_local! {
    static PREVIEWED_CHILD: Cell<Option<usize>> = const { Cell::new(None) };
}

/// During [`Widget::preview_event`](super::Widget::preview_event): the index
/// of the child of the previewing widget that the event is dispatched into
/// next. `None` outside a preview, in `on_event`, and when the dispatch's
/// path names a child the previewing widget no longer has.
pub fn previewed_child() -> Option<usize> {
    PREVIEWED_CHILD.with(Cell::get)
}

/// Sets [`previewed_child`] for the preview about to run; the previous value
/// comes back when the guard drops.
pub(crate) fn enter_preview(child: Option<usize>) -> PreviewGuard {
    PreviewGuard {
        previous: PREVIEWED_CHILD.with(|c| c.replace(child)),
    }
}

/// Restores the [`previewed_child`] an outer preview had.
pub(crate) struct PreviewGuard {
    previous: Option<usize>,
}

impl Drop for PreviewGuard {
    fn drop(&mut self) {
        PREVIEWED_CHILD.with(|c| c.set(self.previous));
    }
}
