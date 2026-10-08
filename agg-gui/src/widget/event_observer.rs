//! Event observers: watch the events one widget receives without wrapping
//! it — the counterpart of agg-sharp's per-widget events
//! (`widget.MouseMove += ...`, `MouseEnter`, `MouseLeave`, ...), which any
//! code can subscribe to on any `GuiWidget`.
//!
//! Wrapping a widget to watch it changes the tree (the wrapper becomes the
//! hit widget, the first under the mouse, the capture holder), so a test of
//! how a [`Button`](crate::widgets::Button) itself is treated cannot use one.
//! [`observe_events`] registers a callback against a [`WidgetId`] instead;
//! the dispatch walk (`tree.rs`'s `deliver`) calls it with every event the
//! widget receives, in the widget's local coordinates, just before the
//! widget's own `on_event`. Observers are per UI thread and cost one
//! emptiness check per delivery while none are registered.

use std::cell::RefCell;
use std::rc::Rc;

use super::{Widget, WidgetId};
use crate::event::Event;

type Callback = Rc<RefCell<dyn FnMut(&Event)>>;

thread_local! {
    static OBSERVERS: RefCell<Vec<(u64, WidgetId, Callback)>> = const { RefCell::new(Vec::new()) };
    static NEXT_KEY: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A registered observer; dropping it stops the callback. Keep it for as
/// long as the widget should be watched (a widget's identity can be reused
/// by another widget once it is dropped).
#[must_use = "dropping the observer unregisters it"]
pub struct EventObserver {
    key: u64,
}

impl Drop for EventObserver {
    fn drop(&mut self) {
        // The thread-local may already be gone during thread teardown.
        let _ = OBSERVERS.try_with(|o| o.borrow_mut().retain(|(k, _, _)| *k != self.key));
    }
}

/// Call `callback` with every event delivered to the widget `id` on this
/// thread (positions in the widget's local space), before the widget
/// handles it, until the returned [`EventObserver`] is dropped. Take `id`
/// once the widget is boxed ([`WidgetId::of`]).
pub fn observe_events(id: WidgetId, callback: impl FnMut(&Event) + 'static) -> EventObserver {
    let key = NEXT_KEY.with(|k| {
        let key = k.get();
        k.set(key + 1);
        key
    });
    let callback: Callback = Rc::new(RefCell::new(callback));
    OBSERVERS.with(|o| o.borrow_mut().push((key, id, callback)));
    EventObserver { key }
}

/// Tell the observers of `widget` about `event` (called by the dispatch walk
/// just before the widget's `on_event`).
pub(crate) fn notify(widget: &dyn Widget, event: &Event) {
    let callbacks: Vec<Callback> = OBSERVERS.with(|o| {
        let o = o.borrow();
        if o.is_empty() {
            return Vec::new();
        }
        let id = WidgetId::of(widget);
        o.iter()
            .filter(|(_, w, _)| *w == id)
            .map(|(_, _, cb)| Rc::clone(cb))
            .collect()
    });
    // Called outside the registry borrow, so a callback may register or
    // drop observers; a callback that re-enters itself is skipped.
    for cb in callbacks {
        if let Ok(mut cb) = cb.try_borrow_mut() {
            cb(event);
        }
    }
}
