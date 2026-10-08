//! Tooltip shown/popped notifications from the central tooltip
//! [`controller`](super::controller): C# `ToolTipManager.ToolTipShown` and
//! `ToolTipPop`.
//!
//! The controller queues an event while it holds its own state and delivers
//! the queue once it has let go ([`dispatch_pending`]), so an observer may
//! read [`controller::is_visible`](super::controller::is_visible) or
//! [`controller::current_text`](super::controller::current_text) from its
//! callback. Observers are per UI thread, like the controller.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What the central tooltip did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TooltipEvent {
    /// A tip came up showing this text (C# `ToolTipShown` with its text).
    Shown(String),
    /// The visible tip went away (C# `ToolTipPop`).
    Popped,
}

type Observer = Rc<RefCell<dyn FnMut(&TooltipEvent)>>;

thread_local! {
    static OBSERVERS: RefCell<Vec<(u64, Observer)>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(0) };
    static PENDING: RefCell<Vec<TooltipEvent>> = const { RefCell::new(Vec::new()) };
}

/// Keeps a [`observe_tooltips`] callback installed; dropping it removes the
/// callback.
#[must_use = "dropping the observer removes the callback"]
pub struct TooltipObserver {
    id: u64,
}

impl Drop for TooltipObserver {
    fn drop(&mut self) {
        let id = self.id;
        OBSERVERS.with(|o| o.borrow_mut().retain(|(i, _)| *i != id));
    }
}

/// Call `callback` with every [`TooltipEvent`] of this thread's central
/// tooltip until the returned guard drops.
pub fn observe_tooltips(callback: impl FnMut(&TooltipEvent) + 'static) -> TooltipObserver {
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let observer: Observer = Rc::new(RefCell::new(callback));
    OBSERVERS.with(|o| o.borrow_mut().push((id, observer)));
    TooltipObserver { id }
}

/// Queue `event` for the next [`dispatch_pending`].
pub(super) fn queue(event: TooltipEvent) {
    PENDING.with(|p| p.borrow_mut().push(event));
}

/// Deliver the queued events, in order, to every observer.
pub(super) fn dispatch_pending() {
    let events = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    if events.is_empty() {
        return;
    }
    let observers: Vec<Observer> =
        OBSERVERS.with(|o| o.borrow().iter().map(|(_, ob)| Rc::clone(ob)).collect());
    for event in &events {
        for observer in &observers {
            (observer.borrow_mut())(event);
        }
    }
}
