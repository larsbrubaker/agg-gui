//! Per-axis consumption of a wheel event while it bubbles.
//!
//! agg-sharp's `MouseEventArgs` is mutable: a widget that uses only one of a
//! gesture's two axes zeroes that axis (`WheelDelta = 0` or
//! `WheelDeltaX = 0`) and the rest keeps bubbling to its ancestors
//! (`ScrollableWidget.OnMouseWheel`, `HorizontalWheelRoutingTests`). agg-gui
//! dispatches an immutable [`Event`] with a whole-event
//! [`EventResult`](crate::event::EventResult), so a widget that takes only
//! part of a wheel event calls [`take_wheel_axes`] and returns `Ignored`;
//! the dispatcher ([`crate::widget::dispatch_event`]) then hands every
//! ancestor the event with the taken axes zeroed, and stops the bubble once
//! every non-zero axis has been taken. Returning `Consumed` still takes the
//! whole event, as before.

use std::cell::Cell;

use crate::event::Event;

thread_local! {
    /// `(x, y)`: which axes of the wheel event being dispatched a widget
    /// below has already taken.
    static TAKEN: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// Mark the horizontal (`x`) and/or vertical (`y`) axis of the wheel event
/// being dispatched as used, so the ancestors it bubbles to see that axis as
/// zero. Call from `on_event` while handling [`Event::MouseWheel`], then
/// return `EventResult::Ignored` to let the remaining axis bubble.
pub fn take_wheel_axes(x: bool, y: bool) {
    TAKEN.with(|t| {
        let (tx, ty) = t.get();
        t.set((tx || x, ty || y));
    });
}

/// Forget the taken axes: the start of a new wheel dispatch.
pub(crate) fn reset_wheel_axes() {
    TAKEN.with(|t| t.set((false, false)));
}

/// What an ancestor receives of `event` once the taken axes are zeroed:
/// `None` when nothing changes (not a wheel event, or nothing taken),
/// `Some(None)` when every non-zero axis has been taken (the bubble stops),
/// `Some(Some(e))` for the event with the taken axes zeroed.
pub(crate) fn remaining_wheel(event: &Event) -> Option<Option<Event>> {
    let Event::MouseWheel {
        pos,
        delta_y,
        delta_x,
        modifiers,
    } = event
    else {
        return None;
    };
    let (tx, ty) = TAKEN.with(|t| t.get());
    if !tx && !ty {
        return None;
    }
    let delta_x = if tx { 0.0 } else { *delta_x };
    let delta_y = if ty { 0.0 } else { *delta_y };
    if delta_x == 0.0 && delta_y == 0.0 {
        return Some(None);
    }
    Some(Some(Event::MouseWheel {
        pos: *pos,
        delta_y,
        delta_x,
        modifiers: *modifiers,
    }))
}
