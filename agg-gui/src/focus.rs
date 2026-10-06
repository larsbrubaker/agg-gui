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
//!
//! The reverse direction — giving the keyboard *up* — uses the same channel:
//!
//! - [`request_blur`] clears focus unconditionally (whatever holds it).
//! - [`release_focus`] clears focus only if the widget whose `focus_id` is
//!   `id` still holds it, so a stale release (focus already moved elsewhere
//!   by a click or Tab) can never steal focus from another widget.
//!
//! Either is serviced on the next `layout` as a plain focus clear: the widget
//! that had focus receives `FocusLost` (a `TextField` commits its edit then),
//! nothing is focused afterwards, and later keys take the App's
//! unconsumed-key path. The most recent call wins between [`request_focus`]
//! and [`request_blur`]; a [`release_focus`] only cancels a pending
//! [`request_focus`] for the same `id`. When a blur and a focus request are
//! both pending, the blur is applied first.

use std::cell::Cell;

/// Opaque, app-chosen identifier for a focusable widget. Values only need to
/// be unique among the widgets that opt into focus-by-request.
pub type FocusId = u64;

std::thread_local! {
    static PENDING_FOCUS: Cell<Option<FocusId>> = const { Cell::new(None) };
    static PENDING_BLUR: Cell<Option<BlurRequest>> = const { Cell::new(None) };
}

/// A pending request to clear keyboard focus, read by the `App` through
/// [`take_blur_request`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlurRequest {
    /// Clear focus whatever widget holds it ([`request_blur`]).
    Any,
    /// Clear focus only if the widget with this `focus_id` holds it
    /// ([`release_focus`]).
    Owner(FocusId),
}

/// Request that the widget whose [`Widget::focus_id`](crate::widget::Widget::focus_id)
/// equals `id` receive focus on the next frame. Also wakes the host loop
/// (via [`crate::animation::request_draw`]) so the request is serviced
/// promptly.
pub fn request_focus(id: FocusId) {
    PENDING_FOCUS.with(|c| c.set(Some(id)));
    PENDING_BLUR.with(|c| c.set(None));
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

/// Request that keyboard focus be cleared on the next frame, whatever widget
/// holds it. Safe to call from a widget's own event handler (e.g. a field's
/// `on_enter` callback): the App applies it after the handler returns,
/// dispatching `FocusLost` to the widget that had focus. Cancels a pending
/// [`request_focus`]. Wakes the host loop.
pub fn request_blur() {
    PENDING_BLUR.with(|c| c.set(Some(BlurRequest::Any)));
    PENDING_FOCUS.with(|c| c.set(None));
    crate::animation::request_draw();
}

/// Request that the widget whose [`Widget::focus_id`](crate::widget::Widget::focus_id)
/// equals `id` give up keyboard focus on the next frame. A no-op when that
/// widget doesn't hold focus by then. Also cancels a pending
/// [`request_focus`] for the same `id` (other ids are left alone). A pending
/// [`request_blur`] is not narrowed by a later `release_focus`. Wakes the host
/// loop.
pub fn release_focus(id: FocusId) {
    PENDING_BLUR.with(|c| {
        if c.get() != Some(BlurRequest::Any) {
            c.set(Some(BlurRequest::Owner(id)));
        }
    });
    PENDING_FOCUS.with(|c| {
        if c.get() == Some(id) {
            c.set(None);
        }
    });
    crate::animation::request_draw();
}

/// Read-and-clear the pending blur request. Called by the `App` once per
/// `layout`, before [`take_focus_request`].
pub fn take_blur_request() -> Option<BlurRequest> {
    PENDING_BLUR.with(|c| c.replace(None))
}

/// Discard any pending blur request without acting on it.
pub fn clear_blur_request() {
    PENDING_BLUR.with(|c| c.set(None));
}
