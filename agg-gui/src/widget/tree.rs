use super::*;

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
    // child that happens to cover the same pixels.
    if widget.claims_pointer_exclusively(local_pos) {
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

/// Dispatch `event` through a path (list of child indices from the root).
/// The event bubbles leaf → root; returns `Consumed` if any widget consumed it.
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
        let result = root.on_event(event);
        if result == EventResult::Consumed || before != crate::animation::invalidation_epoch() {
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
        return root.on_event(event);
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
    if child_result == EventResult::Consumed {
        root.mark_dirty();
        return EventResult::Consumed;
    }
    if before_child != crate::animation::invalidation_epoch() {
        root.mark_dirty();
    }
    // Bubble: deliver to this widget too (with original pos_in_root coords).
    let before_self = crate::animation::invalidation_epoch();
    let result = root.on_event(event);
    if result == EventResult::Consumed || before_self != crate::animation::invalidation_epoch() {
        root.mark_dirty();
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
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Inspector support
// ---------------------------------------------------------------------------

/// Flat snapshot of one widget for the inspector panel.
#[derive(Clone)]
pub struct InspectorNode {
    pub type_name: &'static str,
    /// Absolute screen bounds (Y-up), accumulated as the tree is walked.
    pub screen_bounds: Rect,
    pub depth: usize,
    /// Type-specific display properties from [`Widget::properties`].
    pub properties: Vec<(&'static str, String)>,
}

// ── Global mouse-world-pos (for nested drags that can't use widget-
//    local coords because ancestor layout shifts under them each frame) ─────

thread_local! {
    static CURRENT_MOUSE_WORLD: std::cell::Cell<Option<Point>> =
        std::cell::Cell::new(None);
    static CURRENT_VIEWPORT: std::cell::Cell<Size> =
        std::cell::Cell::new(Size::new(1.0, 1.0));
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

/// Walk the subtree rooted at `widget` and collect an `InspectorNode` per
/// widget in DFS paint order (root first).
///
/// `screen_origin` is the accumulated parent offset in screen Y-up coords.
pub fn collect_inspector_nodes(
    widget: &dyn Widget,
    depth: usize,
    screen_origin: Point,
    out: &mut Vec<InspectorNode>,
) {
    // Invisible widgets (and their entire subtrees) are excluded from the
    // inspector — they are not part of the live rendered scene.
    if !widget.is_visible() {
        return;
    }
    // Utility widgets opt out of the inspector entirely.
    if !widget.show_in_inspector() {
        return;
    }

    let b = widget.bounds();
    let abs = Rect::new(
        screen_origin.x + b.x,
        screen_origin.y + b.y,
        b.width,
        b.height,
    );
    // Build the properties vec — include the universal `backbuffer` flag
    // first (so every widget shows it in a consistent location), then the
    // widget-specific properties.
    let mut props = vec![(
        "backbuffer",
        if widget.has_backbuffer() {
            "true".to_string()
        } else {
            "false".to_string()
        },
    )];
    props.extend(widget.properties());
    out.push(InspectorNode {
        type_name: widget.type_name(),
        screen_bounds: abs,
        depth,
        properties: props,
    });

    // Widgets that are part of the inspector infrastructure opt out of child
    // recursion to prevent the inspector from growing its own node list every
    // frame (exponential growth).  Their sub-trees are still visible in the
    // inspector on the next frame through the normal layout snapshot.
    if !widget.contributes_children_to_inspector() {
        return;
    }

    let child_origin = Point::new(abs.x, abs.y);
    for child in widget.children() {
        collect_inspector_nodes(child.as_ref(), depth + 1, child_origin, out);
    }
}
