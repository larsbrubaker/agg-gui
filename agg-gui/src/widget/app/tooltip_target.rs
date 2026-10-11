//! Which widget's tooltip the central tooltip controller shows: the deepest
//! tipped widget on the hover path, or — when the hovered widget has no tip
//! of its own — a tipped non-hittable child under the pointer (a `Label`
//! never takes the pointer, yet an ellipsized one tips its full text, as
//! agg-sharp's `TextWidget.ToolTipText` does).
//!
//! A widget that shows many items under one bounds reports the hovered one
//! through [`Widget::tooltip_key`]; the key is appended to the identity so a
//! key change reads to the controller as entering a different widget.
//!
//! Used by `App::begin_tooltip_controller_frame` (`app.rs`); the controller
//! itself lives in `crate::widgets::tooltip::controller`.

use crate::geometry::{Point, Rect, Size};
use crate::widget::tree::child_local_pos;
use crate::widget::Widget;

/// Walk the hover `path` from `root` and return the **deepest** widget along it
/// whose [`Widget::tooltip_text`] is `Some`, as `(identity_path, text)`. The
/// identity is the path prefix down to that widget — plus its
/// [`Widget::tooltip_key`], when it reports one ([`push_key`]) — which the
/// controller uses to detect target changes (moving to a different tipped
/// control or item ⇒ reshow).
/// A shallower tipped ancestor is used only when no deeper descendant has a tip.
///
/// When the path's last widget has no tip and `pointer` (root coordinates) is
/// known, its non-hittable children under the pointer are searched too
/// ([`probe_untipped_leaf`]); their tip beats the ancestors'.
///
/// The third element is the tipped widget's bounds in root coordinates, when
/// the pointer is known (the tooltip controller's
/// [`TooltipPlacement::Widget`](crate::widgets::TooltipPlacement) centres
/// the tip on it). It is found from the pointer: the widget's origin is the
/// pointer less the pointer's widget-local position, which holds through
/// scroll offsets and nested translations.
pub(super) fn deepest_tipped(
    root: &dyn Widget,
    path: &[usize],
    pointer: Option<Point>,
) -> Option<(Vec<usize>, String, Option<Rect>)> {
    let mut widget: &dyn Widget = root;
    let mut pos = pointer;
    let mut best: Option<(usize, String, Option<u64>, Option<Rect>)> = None;
    if let Some(t) = widget.tooltip_text() {
        let owner = owner_rect(pointer, pos, rect_size(widget.bounds()));
        best = Some((0, t.to_string(), widget.tooltip_key(), owner));
    }
    let mut depth = 0;
    for (i, &idx) in path.iter().enumerate() {
        let Some(child) = widget.children().get(idx) else {
            break;
        };
        pos = pos.map(|p| child_local_pos(widget, child.bounds(), p));
        widget = child.as_ref();
        depth = i + 1;
        if let Some(t) = widget.tooltip_text() {
            let owner = owner_rect(pointer, pos, rect_size(widget.bounds()));
            best = Some((i + 1, t.to_string(), widget.tooltip_key(), owner));
        }
    }
    let leaf_tipped = best.as_ref().is_some_and(|(d, ..)| *d == depth);
    if !leaf_tipped {
        if let Some(local) = pos {
            if let Some((tail, text, child_local, size)) = probe_untipped_leaf(widget, local) {
                let mut identity = path[..depth].to_vec();
                identity.extend(tail);
                return Some((identity, text, owner_rect(pointer, Some(child_local), size)));
            }
        }
    }
    best.map(|(depth, text, key, owner)| {
        let mut identity = path[..depth].to_vec();
        push_key(&mut identity, key);
        (identity, text, owner)
    })
}

/// A widget of `size` whose local pointer position is `local`, given the
/// pointer at `pointer` in root coordinates: its root-space bounds.
fn owner_rect(pointer: Option<Point>, local: Option<Point>, size: Size) -> Option<Rect> {
    let (p, l) = (pointer?, local?);
    Some(Rect::new(p.x - l.x, p.y - l.y, size.width, size.height))
}

/// Marks the start of a [`Widget::tooltip_key`] suffix in an identity path.
/// No widget has `usize::MAX - 1` children, so a keyed identity never equals
/// a deeper widget's index path (`usize::MAX` is the controller's overlay
/// marker).
const TOOLTIP_KEY_MARKER: usize = usize::MAX - 1;

/// Append `key` to `identity` as the marker plus its low and high 32-bit
/// halves (each fits a `usize` on 32-bit targets such as wasm, so distinct
/// keys never alias). `None` appends nothing, leaving unkeyed identities as
/// plain index paths.
fn push_key(identity: &mut Vec<usize>, key: Option<u64>) {
    if let Some(k) = key {
        identity.push(TOOLTIP_KEY_MARKER);
        identity.push((k & 0xFFFF_FFFF) as usize);
        identity.push((k >> 32) as usize);
    }
}

/// Search `widget`'s visible children that contain `local` but don't take the
/// pointer (`hit_test` is `false`) — topmost first, depth first — for one with
/// a tooltip. Returns the child-index path below `widget`, the tip, and the
/// pointer's position local to the tipped child with that child's size.
fn probe_untipped_leaf(
    widget: &dyn Widget,
    local: Point,
) -> Option<(Vec<usize>, String, Point, Size)> {
    for (i, child) in widget.children().iter().enumerate().rev() {
        let b = child.bounds();
        if !child.is_visible() || b.width <= 0.0 || b.height <= 0.0 {
            continue;
        }
        let p = child_local_pos(widget, b, local);
        let inside = p.x >= 0.0 && p.y >= 0.0 && p.x <= b.width && p.y <= b.height;
        if !inside || child.hit_test(p) {
            continue;
        }
        if let Some(t) = child.tooltip_text() {
            let mut tail = vec![i];
            push_key(&mut tail, child.tooltip_key());
            return Some((tail, t.to_string(), p, rect_size(b)));
        }
        if let Some((mut tail, text, at, size)) = probe_untipped_leaf(child.as_ref(), p) {
            tail.insert(0, i);
            return Some((tail, text, at, size));
        }
    }
    None
}

fn rect_size(r: Rect) -> Size {
    Size::new(r.width, r.height)
}
