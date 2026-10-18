//! The keyboard-resized layout: while the on-screen keyboard is up, the
//! tree is laid out in the room above it rather than lifted whole.
//!
//! A host opts in with
//! [`on_screen_keyboard::set_resizes_layout`](crate::widgets::on_screen_keyboard::set_resizes_layout).
//! [`App::layout`](crate::widget::App::layout) then lays the root out
//! [`layout_room`](crate::widgets::on_screen_keyboard::layout_room) shorter
//! than the viewport, and this module keeps the rest in step:
//!
//! * **Where the root shows.** The root keeps its origin at `(0, 0)` and is
//!   shown raised by exactly the room, through the same global lift
//!   `keyboard_scroll.rs` animates for the default mode. Every coordinate
//!   rule of that lift holds unchanged (root logical space is unlifted;
//!   pointer events, overlays and the IME caret all account for it), and
//!   the lift's slide matches the keyboard's, so the root's bottom edge
//!   travels with the keyboard's top.
//! * **The focused field.** Everything in the root is above the keyboard,
//!   so the only way a focused field is hidden is a scrolling ancestor
//!   clipping it, which happens when the shorter layout shrinks that
//!   ancestor under a field that was showing. On the first layout after a
//!   focus change, or after the room changes (the keyboard shows, or
//!   switches to a page of another height), the field's innermost
//!   ancestor that scrolls ([`Widget::scroll_rect_into_view`]) scrolls the
//!   least amount that shows the whole field. Only then: a field the user
//!   scrolls away from while typing is not pulled back on every layout.
//!
//! `keyboard_scroll.rs` hands over to this module whenever the mode is on;
//! with it off nothing here runs.

use std::cell::Cell;

use crate::geometry::{Rect, Size};
use crate::widget::Widget;

thread_local! {
    /// A focus change happened since the last layout: reveal the field then.
    static REVEAL_PENDING: Cell<bool> = const { Cell::new(false) };
    /// The keyboard room the last layout used, so a change of room reveals.
    static LAST_ROOM: Cell<f64> = const { Cell::new(0.0) };
}

/// Whether the host turned the resized layout on.
pub(crate) fn active() -> bool {
    crate::widgets::on_screen_keyboard::resizes_layout()
}

/// Focus moved (or the App was asked to show the focused field): the next
/// layout, the first with the keyboard's new room, reveals the field.
pub(crate) fn note_focus_change() {
    REVEAL_PENDING.with(|c| c.set(true));
    crate::animation::request_layout();
}

/// After the root was laid out in `viewport` less the keyboard's room: show
/// the root raised by the room, and reveal the focused text field when a
/// focus change or a change of room asks for it.
pub(crate) fn after_layout(focus: Option<&[usize]>, viewport: Size, root: &mut dyn Widget) {
    let room = crate::widgets::on_screen_keyboard::layout_room(viewport);
    super::keyboard_scroll::request_lift(room);
    // A focus change applied at the end of this layout changed the room after
    // the root was laid out: the reveal waits for the layout in the new room.
    if (viewport.height - room - root.bounds().height).abs() > 0.5 {
        crate::animation::request_layout();
        return;
    }
    let room_changed = LAST_ROOM.with(|c| c.replace(room)) != room;
    let pending = REVEAL_PENDING.with(|c| c.replace(false));
    if room <= 0.0 || !(pending || room_changed) {
        return;
    }
    let Some(path) = focus else {
        return;
    };
    if super::keyboard_scroll::mutable_widget_at_path(root, path)
        .is_some_and(|w| w.accepts_text_input())
        && reveal_in_innermost_scroller(root, path)
    {
        // The scroller moves its content at its next layout.
        crate::animation::request_layout();
    }
}

/// Ask the focused widget's ancestors, innermost first, to scroll its rect
/// into view; the first that scrolls takes it. Returns whether one did.
/// Outer scrollers are not asked: the inner one's move only shows in its
/// children's bounds at the next layout, so an outer one would act on
/// stale positions.
fn reveal_in_innermost_scroller(root: &mut dyn Widget, path: &[usize]) -> bool {
    for depth in (0..path.len()).rev() {
        let (ancestor, below) = path.split_at(depth);
        let Some(rect) = rect_in_ancestor(&*root, ancestor, below) else {
            return false;
        };
        let Some(widget) = super::keyboard_scroll::mutable_widget_at_path(root, ancestor) else {
            return false;
        };
        if widget.scroll_rect_into_view(rect) {
            return true;
        }
    }
    false
}

/// The rect of the widget at `ancestor ++ below` in the ancestor's own local
/// space (the space [`Widget::scroll_rect_into_view`] takes).
fn rect_in_ancestor(root: &dyn Widget, ancestor: &[usize], below: &[usize]) -> Option<Rect> {
    let mut widget = root;
    for &i in ancestor {
        widget = widget.children().get(i)?.as_ref();
    }
    // In the ancestor's parent space (the walk folds in the ancestor's own
    // offset and child transform), then moved into its local space.
    let r = super::keyboard_scroll::widget_rect_in_root(widget, below, |w| w.bounds())?;
    let origin = widget.bounds();
    Some(Rect::new(r.x - origin.x, r.y - origin.y, r.width, r.height))
}
