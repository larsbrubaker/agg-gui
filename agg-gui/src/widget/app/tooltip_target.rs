//! Which widget's tooltip the central tooltip controller shows: the deepest
//! tipped widget on the hover path, or — when the hovered widget has no tip
//! of its own — a tipped non-hittable child under the pointer (a `Label`
//! never takes the pointer, yet an ellipsized one tips its full text, as
//! agg-sharp's `TextWidget.ToolTipText` does).
//!
//! Used by `App::begin_tooltip_controller_frame` (`app.rs`); the controller
//! itself lives in `crate::widgets::tooltip::controller`.

use crate::geometry::Point;
use crate::widget::tree::child_local_pos;
use crate::widget::Widget;

/// Walk the hover `path` from `root` and return the **deepest** widget along it
/// whose [`Widget::tooltip_text`] is `Some`, as `(identity_path, text)`. The
/// identity is the path prefix down to that widget, which the controller uses
/// to detect target changes (moving to a different tipped control ⇒ reshow).
/// A shallower tipped ancestor is used only when no deeper descendant has a tip.
///
/// When the path's last widget has no tip and `pointer` (root coordinates) is
/// known, its non-hittable children under the pointer are searched too
/// ([`probe_untipped_leaf`]); their tip beats the ancestors'.
pub(super) fn deepest_tipped(
    root: &dyn Widget,
    path: &[usize],
    pointer: Option<Point>,
) -> Option<(Vec<usize>, String)> {
    let mut widget: &dyn Widget = root;
    let mut pos = pointer;
    let mut best: Option<(usize, String)> = None;
    if let Some(t) = widget.tooltip_text() {
        best = Some((0, t.to_string()));
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
            best = Some((i + 1, t.to_string()));
        }
    }
    let leaf_tipped = best.as_ref().is_some_and(|(d, _)| *d == depth);
    if !leaf_tipped {
        if let Some(local) = pos {
            if let Some((tail, text)) = probe_untipped_leaf(widget, local) {
                let mut identity = path[..depth].to_vec();
                identity.extend(tail);
                return Some((identity, text));
            }
        }
    }
    best.map(|(depth, text)| (path[..depth].to_vec(), text))
}

/// Search `widget`'s visible children that contain `local` but don't take the
/// pointer (`hit_test` is `false`) — topmost first, depth first — for one with
/// a tooltip. Returns the child-index path below `widget` and the tip.
fn probe_untipped_leaf(widget: &dyn Widget, local: Point) -> Option<(Vec<usize>, String)> {
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
            return Some((vec![i], t.to_string()));
        }
        if let Some((mut tail, text)) = probe_untipped_leaf(child.as_ref(), p) {
            tail.insert(0, i);
            return Some((tail, text));
        }
    }
    None
}
