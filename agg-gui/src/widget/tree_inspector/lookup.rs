//! Widget lookup for the inspector and tests: depth-first search by id or
//! type name, a widget's on-screen rectangle, and the thread-local world
//! mouse position and viewport the shells record each frame.
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
