//! Thread-local programmatic focus-request channel.
//!
//! Widgets built in app code can't reach the [`App`](crate::widget::App)'s
//! private focus path to focus themselves when they appear — e.g. a search
//! field that should grab the keyboard the instant its overlay opens. This
//! channel mirrors [`crate::animation::request_draw`]:
//!
//! 1. The widget is built with a stable [`FocusId`] and returns it from
//!    [`Widget::focus_id`](crate::widget::Widget::focus_id).
//! 2. App logic calls [`request_focus`] with that id (typically from the
//!    same handler that makes the widget visible).
//! 3. The `App` consumes the pending request on its next `layout`, locates
//!    the focusable widget whose `focus_id` matches, and moves focus to it
//!    — dispatching `FocusGained` and (for text inputs) raising the
//!    on-screen keyboard.
//!
//! Only one request is held at a time; a later [`request_focus`] before the
//! `App` services the previous one wins.

use std::cell::Cell;

/// Opaque, app-chosen identifier for a focusable widget. Values only need to
/// be unique among the widgets that opt into focus-by-request.
pub type FocusId = u64;

std::thread_local! {
    static PENDING_FOCUS: Cell<Option<FocusId>> = const { Cell::new(None) };
}

/// Request that the widget whose [`Widget::focus_id`](crate::widget::Widget::focus_id)
/// equals `id` receive focus on the next frame. Also wakes the host loop
/// (via [`crate::animation::request_draw`]) so the request is serviced
/// promptly.
pub fn request_focus(id: FocusId) {
    PENDING_FOCUS.with(|c| c.set(Some(id)));
    crate::animation::request_draw();
}

/// Read-and-clear the pending focus request. Called by the `App` once per
/// `layout`.
pub fn take_focus_request() -> Option<FocusId> {
    PENDING_FOCUS.with(|c| c.replace(None))
}

/// Discard any pending focus request without acting on it.
pub fn clear_focus_request() {
    PENDING_FOCUS.with(|c| c.set(None));
}
