//! The gestures of touch selection ([`super`]): how a host text widget's
//! events become word selection, handle drags, toolbar taps and caret
//! placement, and the per-frame auto-scroll of a handle held past the
//! host's edge. The host calls [`route`] before its own event handling,
//! [`after_event`] after it, and [`tick`] from its layout.

use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::Point;
use crate::widgets::text_context_menu::TextMenuAction;

use super::handles::{HandleKind, HandleStyle};
use super::placement::{handle_at, hit_test, toolbar_layout};
use super::{text_units, Drag, Press, TouchSelectable, CARET_HANDLE_TIME};

/// The keyboard lift now (see [`Drag`]).
fn lift() -> f64 {
    crate::widget::keyboard_scroll::current_lift()
}

/// `pos` read as if the keyboard lift were still what it was at `then`.
fn steady(pos: Point, then: f64) -> Point {
    Point::new(pos.x, pos.y + (lift() - then))
}

/// Route `event` (host-local) before the host's own handling. `Some` when
/// it was handled here; `None` lets the host proceed (and then call
/// [`after_event`]).
pub(crate) fn route<W: TouchSelectable>(w: &mut W, event: &Event) -> Option<EventResult> {
    match event {
        Event::MouseDown { pos, button, .. } => {
            if !crate::touch_emulation::last_press_was_touch() {
                return mouse_down(w, *pos);
            }
            match button {
                MouseButton::Right => Some(long_press(w, *pos)),
                MouseButton::Middle if crate::touch_emulation::touch_pan_travel().is_some() => {
                    let kind = handle_at(w, *pos)?;
                    begin_handle_drag(w, kind, *pos);
                    Some(EventResult::Consumed)
                }
                MouseButton::Left => tap_down(w, *pos),
                _ => None,
            }
        }
        Event::MouseMove { pos } if w.touch_selection().drag.is_some() => {
            drag_to(w, *pos);
            Some(EventResult::Consumed)
        }
        Event::MouseUp { button, pos, .. } => {
            if w.touch_selection().drag.is_some() {
                end_drag(w);
                return Some(EventResult::Consumed);
            }
            if *button != MouseButton::Left {
                return None;
            }
            match w.touch_selection().press {
                Press::Swallowed => {
                    w.touch_selection_mut().press = Press::None;
                    Some(EventResult::Consumed)
                }
                Press::Button(action) => {
                    w.touch_selection_mut().press = Press::None;
                    let on_it = toolbar_layout(w).and_then(|l| l.button_at(*pos)) == Some(action);
                    if on_it {
                        apply_action(w, action);
                    }
                    Some(EventResult::Consumed)
                }
                _ => None,
            }
        }
        Event::MouseCaptureLost => {
            if w.touch_selection().drag.is_some() {
                end_drag(w);
                return Some(EventResult::Consumed);
            }
            let ts = w.touch_selection_mut();
            if matches!(ts.press, Press::Swallowed | Press::Button(_)) {
                // A press that turned into a drag (or was cancelled) runs
                // no button.
                ts.press = Press::None;
                return Some(EventResult::Consumed);
            }
            None
        }
        Event::FocusLost
        | Event::KeyDown { .. }
        | Event::TextInput { .. }
        | Event::ImePreedit { .. } => {
            hide(w);
            None
        }
        _ => None,
    }
}

/// Settle after the host handled a finger's press that [`route`] passed on:
/// once it is released (or its drag ends), a selection gets its handles and
/// toolbar and a caret Android's caret handle.
pub(crate) fn after_event<W: TouchSelectable>(w: &mut W, event: &Event) {
    if w.touch_selection().press != Press::Passed {
        return;
    }
    if !matches!(
        event,
        Event::MouseUp {
            button: MouseButton::Left,
            ..
        } | Event::MouseCaptureLost
    ) {
        return;
    }
    w.touch_selection_mut().press = Press::None;
    let (a, c) = w.ts_anchor_cursor();
    let toolbar = w.ts_toolbar_allowed();
    if a != c {
        w.touch_selection_mut().show_selection(toolbar);
    } else {
        let handle = HandleStyle::current().has_caret_handle() && w.ts_editable();
        w.touch_selection_mut().show_caret(handle, false);
    }
    crate::animation::request_draw();
}

/// Step the auto-scroll of a handle held past the host's edge (call from
/// the host's layout, every frame): scroll toward the finger at a speed
/// that grows with how far past the edge it is, re-aim the handle, and ask
/// for the next frame.
pub(crate) fn tick<W: TouchSelectable>(w: &mut W) {
    let ts = w.touch_selection();
    let (
        Some(Drag::Handle {
            grab, lift: then, ..
        }),
        Some(finger),
    ) = (ts.drag, ts.finger)
    else {
        w.touch_selection_mut().scrolled_at = None;
        return;
    };
    let target = steady(Point::new(finger.x + grab.0, finger.y + grab.1), then);
    let shown = w.ts_text_rect();
    let past = if target.y < shown.y {
        shown.y - target.y
    } else if target.y > shown.y + shown.height {
        shown.y + shown.height - target.y
    } else {
        0.0
    };
    let now = crate::clock::now();
    if past == 0.0 {
        w.touch_selection_mut().scrolled_at = None;
        return;
    }
    if let Some(at) = w.touch_selection().scrolled_at {
        let dt = now.saturating_duration_since(at).as_secs_f64().min(0.1);
        // Points per second: ten times the distance past the edge.
        let speed = (past.abs() * 10.0).clamp(60.0, 3000.0);
        if w.ts_autoscroll(speed * dt * past.signum()) {
            // Re-aim at the last whole line in view, not past the edge:
            // aiming past it would have the host scroll a line more every
            // frame, at the frame rate instead of this speed.
            let probe = w.ts_caret_line(0);
            let line = (probe.top - probe.bottom).clamp(1.0, shown.height * 0.5);
            let edge = Point::new(
                target.x,
                target
                    .y
                    .clamp(shown.y + line, shown.y + shown.height - line),
            );
            aim_handle(w, edge);
        }
    }
    w.touch_selection_mut().scrolled_at = Some(now);
    crate::animation::request_draw_after(std::time::Duration::from_millis(16));
}

fn hide<W: TouchSelectable>(w: &mut W) {
    let ts = w.touch_selection_mut();
    let shown = ts.is_shown();
    ts.hide();
    if shown {
        crate::animation::request_draw();
    }
}

/// A mouse press: a finger's leftovers go. One on a leftover handle or
/// toolbar is taken here and does nothing more, so the caret never jumps
/// to a point outside the text.
fn mouse_down<W: TouchSelectable>(w: &mut W, pos: Point) -> Option<EventResult> {
    let on_leftover = hit_test(w, pos);
    hide(w);
    if on_leftover {
        w.touch_selection_mut().press = Press::Swallowed;
        return Some(EventResult::Consumed);
    }
    None
}

/// The character under the finger at `pos`: the caret position left of
/// it, never the one after the character the finger is on.
fn cluster_under<W: TouchSelectable>(w: &W, text: &str, pos: Point) -> usize {
    let off = w.ts_offset_at(pos);
    let line = w.ts_caret_line(off);
    if pos.x < line.x && off > 0 {
        let prev = text_units::prev(text, off);
        if (w.ts_caret_line(prev).top - line.top).abs() < 0.5 {
            return prev;
        }
    }
    text_units::floor(text, off)
}

/// The cluster boundary nearest `x` around the caret offset `off`.
fn nearest_boundary<W: TouchSelectable>(w: &W, text: &str, off: usize, x: f64) -> usize {
    let (lo, hi) = (text_units::floor(text, off), text_units::ceil(text, off));
    if lo == hi {
        return lo;
    }
    let dl = (w.ts_caret_line(lo).x - x).abs();
    let dh = (w.ts_caret_line(hi).x - x).abs();
    if dl <= dh {
        lo
    } else {
        hi
    }
}

/// A finger held on the field: select the word (all of a password), place
/// the caret past the text, or pick up a handle.
fn long_press<W: TouchSelectable>(w: &mut W, pos: Point) -> EventResult {
    if let Some(kind) = handle_at(w, pos) {
        begin_handle_drag(w, kind, pos);
        crate::touch_emulation::claim_long_press_drag();
        return EventResult::Consumed;
    }
    w.touch_selection_mut().press = Press::None;
    let toolbar = w.ts_toolbar_allowed();
    let text = w.ts_text();
    if w.ts_secret() && !text.is_empty() {
        // Android selects a whole password: its words would show its shape.
        w.ts_select(0, text.len());
        w.touch_selection_mut().show_selection(toolbar);
        return EventResult::Consumed;
    }
    let (start, end) = if w.ts_on_text(pos) {
        text_units::word_at(&text, cluster_under(w, &text, pos))
    } else {
        (0, 0)
    };
    if start == end {
        // Empty space past the text: the caret and Paste.
        let off = w.ts_offset_at(pos);
        w.ts_select(off, off);
        let handle = HandleStyle::current().has_caret_handle() && w.ts_editable();
        w.touch_selection_mut().show_caret(handle, toolbar);
        return EventResult::Consumed;
    }
    w.ts_select(start, end);
    let ts = w.touch_selection_mut();
    ts.show_selection(toolbar);
    ts.drag = Some(Drag::Words {
        pivot: (start, end),
        lift: lift(),
    });
    crate::touch_emulation::claim_long_press_drag();
    EventResult::Consumed
}

/// A finger's tap landing: the toolbar's buttons, the handles and the
/// selection are this controller's; anywhere else is the host's.
fn tap_down<W: TouchSelectable>(w: &mut W, pos: Point) -> Option<EventResult> {
    if let Some(layout) = toolbar_layout(w) {
        if layout.contains(pos) {
            w.touch_selection_mut().press = match layout.button_at(pos) {
                Some(action) => Press::Button(action),
                None => Press::Swallowed,
            };
            return Some(EventResult::Consumed);
        }
    }
    if let Some(kind) = handle_at(w, pos) {
        let allowed = w.ts_toolbar_allowed();
        let ts = w.touch_selection_mut();
        if kind == HandleKind::Caret {
            let shown = ts.toolbar;
            ts.set_toolbar(allowed && !shown);
            ts.caret_handle_until = Some(crate::clock::now() + CARET_HANDLE_TIME);
        }
        ts.press = Press::Swallowed;
        return Some(EventResult::Consumed);
    }
    if w.touch_selection().selection_handles && w.ts_on_text(pos) {
        let (a, c) = w.ts_anchor_cursor();
        let off = w.ts_offset_at(pos);
        if a != c && a.min(c) <= off && off <= a.max(c) {
            let allowed = w.ts_toolbar_allowed();
            let ts = w.touch_selection_mut();
            let shown = ts.toolbar;
            ts.set_toolbar(allowed && !shown);
            ts.press = Press::Swallowed;
            return Some(EventResult::Consumed);
        }
    }
    let ts = w.touch_selection_mut();
    ts.hide();
    ts.press = Press::Passed;
    None
}

/// Run a toolbar button, then leave handles and toolbar as the platform
/// does: Select all keeps both, Copy on iOS keeps the selection, Copy on
/// Android collapses it to its end, and Cut and Paste leave a caret.
fn apply_action<W: TouchSelectable>(w: &mut W, action: TextMenuAction) {
    let (a, c) = w.ts_anchor_cursor();
    w.ts_apply(action);
    match action {
        TextMenuAction::SelectAll => {
            let toolbar = w.ts_toolbar_allowed();
            w.touch_selection_mut().show_selection(toolbar);
        }
        TextMenuAction::Copy if HandleStyle::current() == HandleStyle::Lollipop => {
            w.touch_selection_mut().toolbar = false;
        }
        TextMenuAction::Copy => {
            let end = a.max(c);
            w.ts_select(end, end);
            w.touch_selection_mut().hide();
        }
        TextMenuAction::Cut | TextMenuAction::Paste => w.touch_selection_mut().hide(),
    }
    crate::animation::request_draw();
}

fn begin_handle_drag<W: TouchSelectable>(w: &mut W, kind: HandleKind, finger: Point) {
    let (a, c) = w.ts_anchor_cursor();
    let off = match kind {
        HandleKind::Start => a.min(c),
        HandleKind::End => a.max(c),
        HandleKind::Caret => c,
    };
    let mid = w.ts_caret_line(off).middle();
    let ts = w.touch_selection_mut();
    ts.drag = Some(Drag::Handle {
        kind,
        grab: (mid.x - finger.x, mid.y - finger.y),
        lift: lift(),
    });
    ts.finger = Some(finger);
    ts.press = Press::None;
    crate::animation::request_draw();
}

fn drag_to<W: TouchSelectable>(w: &mut W, pos: Point) {
    let Some(drag) = w.touch_selection().drag else {
        return;
    };
    let text = w.ts_text();
    match drag {
        Drag::Words { pivot, lift: then } => {
            let at = steady(pos, then);
            let off = w.ts_offset_at(at);
            let (ws, we) = text_units::word_at(&text, cluster_under(w, &text, at));
            if off >= pivot.1 {
                w.ts_select(pivot.0, we.max(pivot.1));
            } else {
                w.ts_select(pivot.1, ws.min(pivot.0));
            }
        }
        Drag::Handle {
            grab, lift: then, ..
        } => {
            w.touch_selection_mut().finger = Some(pos);
            aim_handle(w, steady(Point::new(pos.x + grab.0, pos.y + grab.1), then));
        }
    }
    crate::animation::request_draw();
}

/// Move the dragged handle's end of the selection to the cluster boundary
/// nearest `target` (host-local).
fn aim_handle<W: TouchSelectable>(w: &mut W, target: Point) {
    let Some(Drag::Handle { kind, .. }) = w.touch_selection().drag else {
        return;
    };
    let text = w.ts_text();
    let off = nearest_boundary(w, &text, w.ts_offset_at(target), target.x);
    let (a, c) = w.ts_anchor_cursor();
    let (lo, hi) = (a.min(c), a.max(c));
    match kind {
        HandleKind::Caret => w.ts_select(off, off),
        // An end never reaches the other: at least one character stays
        // selected, as Android clamps its handles.
        HandleKind::Start => w.ts_select(hi, off.min(text_units::prev(&text, hi))),
        HandleKind::End => w.ts_select(lo, off.max(text_units::next(&text, lo))),
    }
}

fn end_drag<W: TouchSelectable>(w: &mut W) {
    let toolbar = w.ts_toolbar_allowed();
    let ts = w.touch_selection_mut();
    ts.finger = None;
    ts.scrolled_at = None;
    match ts.drag.take() {
        Some(Drag::Handle {
            kind: HandleKind::Caret,
            ..
        }) => {
            ts.caret_handle_until = Some(crate::clock::now() + CARET_HANDLE_TIME);
        }
        Some(_) => ts.set_toolbar(toolbar),
        None => {}
    }
    crate::animation::request_draw();
}
