//! Widget-tree traversal helpers: dirty marking and hit-test path lookup.
//!
//! [`mark_subtree_dirty`] is called by the frame loop after off-event-path
//! async work finishes (font load, image decode). The hit-test family —
//! [`hit_test_subtree`], [`active_modal_path`], [`global_overlay_hit_path`]
//! — returns a `Vec<usize>` path of child indices identifying the deepest
//! widget that claims a position; the App event router uses that path to
//! dispatch mouse events to the right subtree.
//!
//! # Coordinate system
//!
//! All positions in this module are **logical Y-up**, origin at the
//! bottom-left, expressed in the receiving widget's own local space (the
//! caller has already subtracted ancestor `bounds().x/y`). The App
//! pre-converts platform Y-down input coordinates via `App::flip_y`
//! before calling in — do not flip Y here.

use super::*;

/// Recursively call `mark_dirty` on `widget` and every visible
/// descendant.  Used by the host frame loop after an async data
/// source (image fetch + decode, font load, etc.) finishes outside
/// the normal event-dispatch path that would otherwise mark widgets
/// dirty as the event bubbles.  Called explicitly at the top of the
/// frame so the user-visible "freshly-decoded data lands in stale
/// FBO contents" bug never opens a one-frame race window.
pub fn mark_subtree_dirty(widget: &mut dyn Widget) {
    widget.mark_dirty();
    for child in widget.children_mut().iter_mut() {
        mark_subtree_dirty(child.as_mut());
    }
}

/// Walk the subtree rooted at `widget` and return the path (list of child
/// indices) to the deepest widget that passes `hit_test` at `local_pos`.
///
/// `local_pos` is expressed in `widget`'s coordinate space (not including
/// `widget.bounds().x/y` — the caller has already accounted for that).
///
/// Returns `Some(vec![])` if `widget` itself is hit but no child is.
/// Returns `None` if nothing is hit.
pub fn hit_test_subtree(widget: &dyn Widget, local_pos: Point) -> Option<Vec<usize>> {
    if !widget.is_visible() || !widget.hit_test(local_pos) {
        return None;
    }
    // Let overlays (e.g. a floating scrollbar) claim the pointer before any
    // child that happens to cover the same pixels, and let a disabled scope
    // swallow the pointer so clicks never reach its (non-interactive) children.
    if widget.claims_pointer_exclusively(local_pos) || widget.blocks_child_interaction() {
        return Some(vec![]);
    }
    // Check children in reverse order (last drawn = topmost = highest priority).
    for (i, child) in widget.children().iter().enumerate().rev() {
        let child_local = Point::new(
            local_pos.x - child.bounds().x,
            local_pos.y - child.bounds().y,
        );
        if let Some(mut sub_path) = hit_test_subtree(child.as_ref(), child_local) {
            sub_path.insert(0, i);
            return Some(sub_path);
        }
    }
    Some(vec![]) // hit this widget, no child claimed it
}

/// Return the path to the topmost active modal subtree, ignoring normal
/// hit-testing bounds. Modal overlays paint at app level, so their event
/// routing must also bypass regular child clipping/window hit regions.
pub fn active_modal_path(widget: &dyn Widget) -> Option<Vec<usize>> {
    if !widget.is_visible() {
        return None;
    }
    for (i, child) in widget.children().iter().enumerate().rev() {
        if let Some(mut sub_path) = active_modal_path(child.as_ref()) {
            sub_path.insert(0, i);
            return Some(sub_path);
        }
    }
    if widget.has_active_modal() {
        Some(vec![])
    } else {
        None
    }
}

/// Return the topmost widget whose app-level overlay contains `local_pos`.
///
/// This intentionally ignores ancestor `hit_test` bounds while descending:
/// global overlays such as ComboBox popups are painted outside their normal
/// parent clip/bounds, so their event routing must escape those bounds too.
pub fn global_overlay_hit_path(widget: &dyn Widget, local_pos: Point) -> Option<Vec<usize>> {
    if !widget.is_visible() {
        return None;
    }
    for (i, child) in widget.children().iter().enumerate().rev() {
        let child_local = Point::new(
            local_pos.x - child.bounds().x,
            local_pos.y - child.bounds().y,
        );
        if let Some(mut sub_path) = global_overlay_hit_path(child.as_ref(), child_local) {
            sub_path.insert(0, i);
            return Some(sub_path);
        }
    }
    if widget.hit_test_global_overlay(local_pos) {
        Some(vec![])
    } else {
        None
    }
}

/// Centralized event delivery: call the widget's `on_event` and apply the
/// framework's automatic-invalidation policy in ONE place, so no dispatch
/// path can silently forget it.  A [`EventResult::Consumed`] result schedules
/// a repaint via [`crate::animation::request_draw`];
/// [`EventResult::ConsumedQuiet`] and [`EventResult::Ignored`] do not.
///
/// This single hook makes "consume ⇒ repaint" the default, fixing the
/// recurring bug class where a widget mutates paint-affecting state on an
/// event but forgets to request a draw, leaving part of itself stale.
fn deliver(widget: &mut dyn Widget, event: &Event) -> EventResult {
    auto_request_draw(widget.on_event(event))
}

/// Apply the auto-invalidation policy to an already-computed [`EventResult`].
/// Shared by [`deliver`] (for `on_event`) and the unconsumed-key path (for
/// `on_unconsumed_key`) so every delivery route honours the same rule.
fn auto_request_draw(result: EventResult) -> EventResult {
    if result.requests_redraw() {
        crate::animation::request_draw();
    }
    result
}

/// Dispatch `event` through a path (list of child indices from the root).
/// The event bubbles leaf → root; returns a consuming result if any widget
/// consumed it (preserving the `Consumed` vs `ConsumedQuiet` distinction).
///
/// `pos_in_root` is the event position in the root widget's coordinate space.
/// The function translates it down through each level of the path.
pub fn dispatch_event(
    root: &mut Box<dyn Widget>,
    path: &[usize],
    event: &Event,
    pos_in_root: Point,
) -> EventResult {
    if path.is_empty() {
        let before = crate::animation::invalidation_epoch();
        let result = deliver(root.as_mut(), event);
        if result.requests_redraw() || before != crate::animation::invalidation_epoch() {
            root.mark_dirty();
        }
        return result;
    }
    let idx = path[0];
    // Path can become stale between when it was captured (hit-test or
    // previous-frame hovered/focus) and when it is dispatched — e.g. a
    // CollapsingHeader collapsed since then and dropped its child.  Rather
    // than panic, just stop descending and deliver the event at this level.
    if idx >= root.children().len() {
        return deliver(root.as_mut(), event);
    }
    let child_bounds = root.children()[idx].bounds();
    let child_pos = Point::new(
        pos_in_root.x - child_bounds.x,
        pos_in_root.y - child_bounds.y,
    );
    let translated_event = translate_event(event, child_pos);

    let before_child = crate::animation::invalidation_epoch();
    let child_result = dispatch_event(
        &mut root.children_mut()[idx],
        &path[1..],
        &translated_event,
        child_pos,
    );
    // A child (or descendant) that requested a draw bumped the epoch —
    // invalidate our own cache so the retained backbuffer re-rasters. A
    // quiet consume bumps nothing, so it correctly leaves the cache alone.
    if before_child != crate::animation::invalidation_epoch() {
        root.mark_dirty();
    }
    if child_result.is_consumed() {
        return child_result;
    }
    // Bubble: deliver to this widget too (with original pos_in_root coords).
    let before_self = crate::animation::invalidation_epoch();
    let result = deliver(root.as_mut(), event);
    if result.requests_redraw() || before_self != crate::animation::invalidation_epoch() {
        root.mark_dirty();
    }
    result
}

/// Variant of [`dispatch_event`] that accepts `&mut dyn Widget` as the
/// root. Useful when a parent owns a sub-tree by concrete type (e.g.
/// `Window`'s `title_bar: WindowTitleBar`) and wants to route an event
/// into it via the framework's standard hit-test + bubble dispatch,
/// instead of running coordinate hit-tests inline.
pub fn dispatch_event_dyn(
    root: &mut dyn Widget,
    path: &[usize],
    event: &Event,
    pos_in_root: Point,
) -> EventResult {
    if path.is_empty() {
        let before = crate::animation::invalidation_epoch();
        let result = deliver(root, event);
        if result.requests_redraw() || before != crate::animation::invalidation_epoch() {
            root.mark_dirty();
        }
        return result;
    }
    let idx = path[0];
    if idx >= root.children().len() {
        return deliver(root, event);
    }
    let child_bounds = root.children()[idx].bounds();
    let child_pos = Point::new(
        pos_in_root.x - child_bounds.x,
        pos_in_root.y - child_bounds.y,
    );
    let translated_event = translate_event(event, child_pos);

    let before_child = crate::animation::invalidation_epoch();
    // After the first hop we're inside the Vec<Box<dyn Widget>>, so we
    // can fall back to the regular Box-based dispatcher.
    let child_result = dispatch_event(
        &mut root.children_mut()[idx],
        &path[1..],
        &translated_event,
        child_pos,
    );
    if before_child != crate::animation::invalidation_epoch() {
        root.mark_dirty();
    }
    if child_result.is_consumed() {
        return child_result;
    }
    let before_self = crate::animation::invalidation_epoch();
    let result = deliver(root, event);
    if result.requests_redraw() || before_self != crate::animation::invalidation_epoch() {
        root.mark_dirty();
    }
    result
}

/// Give visible widgets a chance to handle a key ignored by the focused path.
///
/// Traverses in reverse paint order so topmost windows/menu bars win.
pub fn dispatch_unconsumed_key(
    widget: &mut dyn Widget,
    key: &Key,
    modifiers: Modifiers,
) -> EventResult {
    if !widget.is_visible() {
        return EventResult::Ignored;
    }
    let mut consumed = None;
    for child in widget.children_mut().iter_mut().rev() {
        let r = dispatch_unconsumed_key(child.as_mut(), key, modifiers);
        if r.is_consumed() {
            consumed = Some(r);
            break;
        }
    }
    if let Some(r) = consumed {
        widget.mark_dirty();
        return r;
    }
    let before = crate::animation::invalidation_epoch();
    let result = auto_request_draw(widget.on_unconsumed_key(key, modifiers));
    if result.requests_redraw() || before != crate::animation::invalidation_epoch() {
        widget.mark_dirty();
    }
    result
}

/// Produce a version of `event` with mouse positions replaced by `new_pos`.
/// Non-mouse events (key, focus) are returned unchanged.
fn translate_event(event: &Event, new_pos: Point) -> Event {
    match event {
        Event::MouseMove { .. } => Event::MouseMove { pos: new_pos },
        Event::MouseDown {
            button, modifiers, ..
        } => Event::MouseDown {
            pos: new_pos,
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseUp {
            button, modifiers, ..
        } => Event::MouseUp {
            pos: new_pos,
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseWheel {
            delta_y,
            delta_x,
            modifiers,
            ..
        } => Event::MouseWheel {
            pos: new_pos,
            delta_y: *delta_y,
            delta_x: *delta_x,
            modifiers: *modifiers,
        },
        Event::FileDropped { paths, .. } => Event::FileDropped {
            pos: new_pos,
            paths: paths.clone(),
        },
        other => other.clone(),
    }
}

/// Offer `event` to every visible widget in the subtree, depth-first with
/// the same topmost-last-child priority as [`hit_test_subtree`], stopping
/// at the first consumer. Positions are translated into each widget's
/// local space on the way down, exactly like [`dispatch_event`].
///
/// This exists for events that must not be lost when the widget under
/// their position ignores them — file drops foremost: native shells can't
/// always produce an accurate drop position (winit's Windows backend
/// discards the OLE drop point and emits no CursorMoved during the drag),
/// so the position may point at chrome while the only interested handler
/// (a canvas) sits in a sibling subtree the bubble path never reaches.
pub fn dispatch_event_broadcast(
    root: &mut Box<dyn Widget>,
    event: &Event,
    pos_in_root: Point,
) -> EventResult {
    if !root.is_visible() {
        return EventResult::Ignored;
    }
    for i in (0..root.children().len()).rev() {
        let child_bounds = root.children()[i].bounds();
        let child_pos = Point::new(
            pos_in_root.x - child_bounds.x,
            pos_in_root.y - child_bounds.y,
        );
        let translated = translate_event(event, child_pos);
        let r = dispatch_event_broadcast(&mut root.children_mut()[i], &translated, child_pos);
        if r.is_consumed() {
            root.mark_dirty();
            return r;
        }
    }
    let result = deliver(root.as_mut(), event);
    if result.is_consumed() {
        root.mark_dirty();
    }
    result
}
