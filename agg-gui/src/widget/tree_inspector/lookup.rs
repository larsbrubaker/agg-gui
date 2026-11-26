//! Widget lookup for the inspector and tests: depth-first search by id or
//! type name, a widget's on-screen rectangle and the part of it that shows
//! (through `widget/paint_geometry.rs`, painting's clip chain), and the
//! thread-local world mouse position and viewport the shells record each
//! frame.
//!
//! Part of `tree_inspector`; shares its transform helpers from `mod.rs`.

use super::super::*;
use super::{compose_child_to_screen, transform_rect_aabb};

// ── Global mouse-world-pos (for nested drags that can't use widget-
//    local coords because ancestor layout shifts under them each frame) ─────

thread_local! {
    static CURRENT_MOUSE_WORLD: std::cell::Cell<Option<Point>> =
        const { std::cell::Cell::new(None) };
    static CURRENT_VIEWPORT: std::cell::Cell<Size> =
        const { std::cell::Cell::new(Size::new(1.0, 1.0)) };
}

/// Record the current mouse cursor position in app-level (world / Y-up
/// logical) coordinates.  Called by `App`'s mouse entry points.
pub fn set_current_mouse_world(p: Point) {
    CURRENT_MOUSE_WORLD.with(|c| c.set(Some(p)));
}

/// Retrieve the latest world-space mouse position.  Widgets doing a
/// drag gesture that needs invariance against ancestor-layout shifts
/// (e.g. a nested `Resize` inside an auto-sized `Window`, where the
/// window grows/shrinks as the user drags and moves the widget's
/// ancestor frame) should prefer this over the widget-local `pos`
/// carried in `Event::Mouse*`.
pub fn current_mouse_world() -> Option<Point> {
    CURRENT_MOUSE_WORLD.with(|c| c.get())
}

/// Record the current app-level viewport in logical Y-up coordinates.
pub fn set_current_viewport(s: Size) {
    CURRENT_VIEWPORT.with(|c| c.set(s));
}

/// Retrieve the latest app-level viewport in logical coordinates.
pub fn current_viewport() -> Size {
    CURRENT_VIEWPORT.with(|c| c.get())
}

/// Depth-first search the subtree rooted at `widget` for one whose
/// [`Widget::id`] matches `id`.  Returns the first match in paint order,
/// including `widget` itself.  Used primarily by tests to locate a
/// specific `Window` by its title without knowing the tree shape.
pub fn find_widget_by_id<'a>(widget: &'a dyn Widget, id: &str) -> Option<&'a dyn Widget> {
    if widget.id() == Some(id) {
        return Some(widget);
    }
    for child in widget.children() {
        if let Some(found) = find_widget_by_id(child.as_ref(), id) {
            return Some(found);
        }
    }
    None
}

/// Mutable counterpart to [`find_widget_by_id`].  Required when a test
/// needs to poke at a sub-widget's mutable state (e.g. calling a
/// `ScrollView::set_scroll_offset`) after finding it by id.
pub fn find_widget_by_id_mut<'a>(
    widget: &'a mut dyn Widget,
    id: &str,
) -> Option<&'a mut dyn Widget> {
    if widget.id() == Some(id) {
        return Some(widget);
    }
    for child in widget.children_mut().iter_mut() {
        if let Some(found) = find_widget_by_id_mut(child.as_mut(), id) {
            return Some(found);
        }
    }
    None
}

/// Absolute (screen-space, Y-up) rectangle of the first widget in the
/// subtree whose [`Widget::id`] matches `id`.
///
/// [`find_widget_by_id`] returns the widget itself, and
/// [`Widget::bounds`] is **parent-local** — many widgets even reset
/// their own origin to (0, 0) in `layout`, so a caller that wants "where
/// on the window is this widget" cannot read `bounds()` directly. This
/// walks the same accumulated transform chain
/// [`collect_inspector_nodes`] does (translation by each ancestor's
/// bounds origin, composed with any [`Widget::inspector_child_transform`]),
/// so the answer matches the inspector overlay's *transform math*.
///
/// It differs from the inspector in which subtrees it walks: the
/// inspector's `show_in_inspector` / `contributes_children_to_inspector`
/// opt-outs are presentation concerns and are ignored here, but a subtree
/// whose root reports `!is_visible()` is skipped entirely — a hidden
/// widget occupies no pixels, so "where on the window is it" has no
/// answer.
///
/// Useful to any host that has to line window-space pixels up with a
/// widget: screenshot / thumbnail crops, platform overlays (IME, native
/// video surfaces), and tests asserting on real placement. Returns
/// `None` when no *visible* widget carries `id`.
pub fn find_widget_screen_rect(root: &dyn Widget, id: &str) -> Option<Rect> {
    // The root's own bounds origin is already screen-absolute, so the
    // walk starts from the identity transform.
    let root_to_screen = crate::TransAffine::new();
    find_widget_screen_rect_inner(root, id, &root_to_screen)
}

fn find_widget_screen_rect_inner(
    widget: &dyn Widget,
    id: &str,
    parent_to_screen: &crate::TransAffine,
) -> Option<Rect> {
    if !widget.is_visible() {
        return None;
    }
    let b = widget.bounds();
    if widget.id() == Some(id) {
        return Some(transform_rect_aabb(parent_to_screen, b));
    }
    let child_to_screen =
        compose_child_to_screen(parent_to_screen, b, &widget.inspector_child_transform());
    for child in widget.children() {
        if let Some(found) = find_widget_screen_rect_inner(child.as_ref(), id, &child_to_screen) {
            return Some(found);
        }
    }
    None
}

/// The part of the first painted widget carrying `id` that shows on screen:
/// its rectangle cut by every clip painting puts it under, as
/// [`paint_geometry`](crate::widget::paint_geometry) replays them: each
/// ancestor's `clip_children_rect`, a row's per-column `child_paint_clip`,
/// child transforms (pan/zoom), and the window alone for a subtree painted in
/// the overlay pass. A widget scrolled half out of a `ScrollView` gives the
/// half that shows.
///
/// The rectangle is in root logical space (Y-up, the space of the root's
/// `bounds()`), the space [`find_widget_screen_rect`] and the overlay queues
/// use. The window is the root's bounds (`App` lays the root out to fill it)
/// cut to the part left on screen while the on-screen keyboard lifts the
/// tree ([`visible_root_rect`](crate::widget::visible_root_rect)), so a widget
/// the lift has pushed off the top shows only below it. The rectangle is
/// where painting puts the widget, so a widget that snaps to whole pixels
/// gives its snapped place; it matches [`find_widget_screen_rect`] wherever
/// that place is already whole.
///
/// `None` when no painted widget carries `id` (it or an ancestor reports
/// `!is_visible()` or [`Widget::suppresses_paint`], as an `Opacity` faded to
/// 0 does, or it is not in the tree) or when nothing of it shows.
pub fn find_widget_visible_rect(root: &dyn Widget, id: &str) -> Option<Rect> {
    let mut path = Vec::new();
    if !painted_path_to_id(root, id, &mut path) {
        return None;
    }
    let widget = super::walk_path(root, &path)?;
    let window = crate::widget::paint_walk::painted_window(root);
    let painted = crate::widget::paint_geometry::paint_placement_in(root, &path, window)?;
    let own = widget.bounds();
    painted.visible_rect(Size::new(own.width, own.height))
}

/// Fill `path` with the child indices from `widget` down to the first widget
/// carrying `id`, in the order [`find_widget_screen_rect`] searches, skipping
/// the subtrees painting skips: hidden ones, as that search does, and ones
/// whose root [`Widget::suppresses_paint`] (neither painted in place nor in
/// the overlay pass). False when there is none.
fn painted_path_to_id(widget: &dyn Widget, id: &str, path: &mut Vec<usize>) -> bool {
    if !widget.is_visible() || widget.suppresses_paint() {
        return false;
    }
    if widget.id() == Some(id) {
        return true;
    }
    for (index, child) in widget.children().iter().enumerate() {
        path.push(index);
        if painted_path_to_id(child.as_ref(), id, path) {
            return true;
        }
        path.pop();
    }
    false
}

/// Depth-first search for a widget by its [`Widget::type_name`].  Returns
/// the first match in paint order.  Used by tests that want to assert on
/// a specific widget kind inside an opaque content subtree (e.g.
/// "find the ScrollView inside this window").
pub fn find_widget_by_type<'a>(widget: &'a dyn Widget, type_name: &str) -> Option<&'a dyn Widget> {
    if widget.type_name() == type_name {
        return Some(widget);
    }
    for child in widget.children() {
        if let Some(found) = find_widget_by_type(child.as_ref(), type_name) {
            return Some(found);
        }
    }
    None
}
