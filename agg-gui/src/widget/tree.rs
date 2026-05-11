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
    for child in widget.children_mut().iter_mut().rev() {
        if dispatch_unconsumed_key(child.as_mut(), key, modifiers) == EventResult::Consumed {
            widget.mark_dirty();
            return EventResult::Consumed;
        }
    }
    let before = crate::animation::invalidation_epoch();
    let result = widget.on_unconsumed_key(key, modifiers);
    if result == EventResult::Consumed || before != crate::animation::invalidation_epoch() {
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
    /// Outer margin in logical units (per-side).  Drawn as the orange band
    /// outside `screen_bounds` in the Chrome F12-style hover overlay.
    pub margin: crate::layout_props::Insets,
    /// Inner padding in logical units (per-side) — only nonzero on container
    /// widgets that override [`Widget::padding`].  Drawn as the green band
    /// inset from `screen_bounds`.
    pub padding: crate::layout_props::Insets,
    /// Horizontal anchor from the widget's `WidgetBase`, if present.
    pub h_anchor: crate::layout_props::HAnchor,
    /// Vertical anchor from the widget's `WidgetBase`, if present.
    pub v_anchor: crate::layout_props::VAnchor,
    pub depth: usize,
    /// Path of child indices from the App root to this widget.  Used by the
    /// inspector's live-editing pipeline to walk back to the live widget and
    /// apply a reflected edit.  Empty for the root.
    pub path: Vec<usize>,
    /// Type-specific display properties from [`Widget::properties`].
    pub properties: Vec<(&'static str, String)>,
}

/// Walk a reflected struct's fields and produce `(name, display)` pairs
/// suitable for the inspector's property pane.  Public so callers can build
/// the same typed dump for ad-hoc reflectable values (e.g. a debug hover
/// inspector outside the widget tree).
#[cfg(feature = "reflect")]
pub fn reflect_fields(reflected: &dyn bevy_reflect::Reflect) -> Vec<(&'static str, String)> {
    use bevy_reflect::{ReflectRef, TypeInfo};
    let mut out = Vec::new();
    if let ReflectRef::Struct(s) = reflected.reflect_ref() {
        // The TypeInfo of the struct gives us field NAMES with `'static`
        // lifetime — required because `InspectorNode::properties` is
        // `Vec<(&'static str, String)>`.  Falling back to indexed names
        // ("field_0") for unrepresented info keeps the dump alive even on
        // tuple structs that don't carry named fields.
        let names: Vec<&'static str> =
            if let Some(TypeInfo::Struct(info)) = reflected.get_represented_type_info() {
                (0..s.field_len())
                    .map(|i| info.field_at(i).map(|f| f.name()).unwrap_or(""))
                    .collect()
            } else {
                vec![""; s.field_len()]
            };
        for i in 0..s.field_len() {
            let name = names.get(i).copied().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if let Some(field) = s.field_at(i) {
                out.push((name, format_reflect_value(field)));
            }
        }
    }
    out
}

#[cfg(feature = "reflect")]
fn format_reflect_value(value: &dyn bevy_reflect::PartialReflect) -> String {
    // Try common primitive types first for clean output, then fall back to
    // `Debug` via `reflect_short_type_path`.  bevy_reflect's `Debug` impl
    // for arbitrary reflected values produces verbose "Reflected(..)" style
    // output — bypass it for the types the inspector sees on a typical frame.
    if let Some(v) = value.try_downcast_ref::<bool>() {
        return v.to_string();
    }
    if let Some(v) = value.try_downcast_ref::<f64>() {
        return format!("{v:.3}");
    }
    if let Some(v) = value.try_downcast_ref::<f32>() {
        return format!("{v:.3}");
    }
    if let Some(v) = value.try_downcast_ref::<i32>() {
        return v.to_string();
    }
    if let Some(v) = value.try_downcast_ref::<u32>() {
        return v.to_string();
    }
    if let Some(v) = value.try_downcast_ref::<usize>() {
        return v.to_string();
    }
    if let Some(v) = value.try_downcast_ref::<String>() {
        return format!("\"{v}\"");
    }
    if let Some(v) = value.try_downcast_ref::<crate::color::Color>() {
        return format!("rgba({:.2}, {:.2}, {:.2}, {:.2})", v.r, v.g, v.b, v.a);
    }
    // Generic fallback: `Debug`-print the reflected value.
    format!("{value:?}")
}

/// Snapshot pushed to the platform render loop so the host can draw a
/// Chrome F12-style three-band overlay (margin + bounds + padding) around
/// the widget the inspector is hovering.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InspectorOverlay {
    pub bounds: Rect,
    pub margin: crate::layout_props::Insets,
    pub padding: crate::layout_props::Insets,
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
    collect_inspector_nodes_with_path(widget, depth, screen_origin, &[], out);
}

fn collect_inspector_nodes_with_path(
    widget: &dyn Widget,
    depth: usize,
    screen_origin: Point,
    path_prefix: &[usize],
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
    // Reflection-driven property dump.  Widgets that opt into the
    // companion-props pattern (`Widget::as_reflect`) get their reflected
    // struct fields surfaced here as `(name, formatted)` pairs — typed,
    // accurate, and free of the hand-maintained `properties()` strings
    // they would otherwise need.  Fields that aren't a struct, or that
    // can't be displayed, are silently skipped.
    #[cfg(feature = "reflect")]
    if let Some(reflected) = widget.as_reflect() {
        props.extend(reflect_fields(reflected));
    }
    let (h_anchor, v_anchor) = widget
        .widget_base()
        .map(|b| (b.h_anchor, b.v_anchor))
        .unwrap_or((
            crate::layout_props::HAnchor::FIT,
            crate::layout_props::VAnchor::FIT,
        ));
    out.push(InspectorNode {
        type_name: widget.type_name(),
        screen_bounds: abs,
        margin: widget.margin(),
        padding: widget.padding(),
        h_anchor,
        v_anchor,
        depth,
        path: path_prefix.to_vec(),
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
    let mut child_path: Vec<usize> = Vec::with_capacity(path_prefix.len() + 1);
    child_path.extend_from_slice(path_prefix);
    child_path.push(0);
    for (i, child) in widget.children().iter().enumerate() {
        *child_path.last_mut().unwrap() = i;
        collect_inspector_nodes_with_path(
            child.as_ref(),
            depth + 1,
            child_origin,
            &child_path,
            out,
        );
    }
}

/// Walk the widget tree from `root` along `path` and return the deepest
/// reachable widget as a mutable reference.  Returns `None` if the path
/// indexes past the available children at any level — useful when the path
/// is stale (e.g. the tree shape changed since the inspector snapshot).
pub fn walk_path_mut<'a>(root: &'a mut dyn Widget, path: &[usize]) -> Option<&'a mut dyn Widget> {
    let mut node: &mut dyn Widget = root;
    for &idx in path {
        let children = node.children_mut();
        if idx >= children.len() {
            return None;
        }
        node = children[idx].as_mut();
    }
    Some(node)
}

/// A pending inspector edit: navigate to the widget at `path`, look up
/// `field_path` via reflection, and apply `new_value`.
///
/// Edits are queued by the inspector and drained by the host frame loop —
/// applying them mid-paint or mid-event-dispatch could violate borrow rules
/// or layout invariants.
#[cfg(feature = "reflect")]
pub struct InspectorEdit {
    pub path: Vec<usize>,
    /// Reflection path inside the target widget's `as_reflect` value, e.g.
    /// `"checked"` or `"value"` or `"margin.left"`.
    pub field_path: String,
    /// Replacement value, already type-correct for the target field.
    pub new_value: Box<dyn bevy_reflect::PartialReflect>,
}

#[cfg(feature = "reflect")]
impl std::fmt::Debug for InspectorEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectorEdit")
            .field("path", &self.path)
            .field("field_path", &self.field_path)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// WidgetBase live editing (no reflect feature required)
// ---------------------------------------------------------------------------

/// One field in a widget's [`crate::layout_props::WidgetBase`] that the
/// inspector can change at runtime.
#[derive(Clone, Debug)]
pub enum WidgetBaseField {
    MarginLeft(f64),
    MarginRight(f64),
    MarginTop(f64),
    MarginBottom(f64),
    HAnchor(crate::layout_props::HAnchor),
    VAnchor(crate::layout_props::VAnchor),
    MinWidth(f64),
    MinHeight(f64),
    MaxWidth(f64),
    MaxHeight(f64),
}

/// Queued mutation for a widget's `WidgetBase`.  The inspector pushes these;
/// the host frame loop drains and applies via [`apply_widget_base_edit`].
#[derive(Clone, Debug)]
pub struct WidgetBaseEdit {
    /// Path of child indices from the App root to the target widget.
    pub path: Vec<usize>,
    pub field: WidgetBaseField,
}

/// Apply a single queued `WidgetBaseEdit` against the live widget tree.
/// Returns `true` when the edit landed, `false` if the path was stale or the
/// widget does not expose a `WidgetBase`.
pub fn apply_widget_base_edit(root: &mut dyn Widget, edit: &WidgetBaseEdit) -> bool {
    let Some(target) = walk_path_mut(root, &edit.path) else {
        return false;
    };
    let Some(base) = target.widget_base_mut() else {
        return false;
    };
    match &edit.field {
        WidgetBaseField::MarginLeft(v) => base.margin.left = *v,
        WidgetBaseField::MarginRight(v) => base.margin.right = *v,
        WidgetBaseField::MarginTop(v) => base.margin.top = *v,
        WidgetBaseField::MarginBottom(v) => base.margin.bottom = *v,
        WidgetBaseField::HAnchor(a) => base.h_anchor = *a,
        WidgetBaseField::VAnchor(a) => base.v_anchor = *a,
        WidgetBaseField::MinWidth(v) => base.min_size.width = v.max(0.0),
        WidgetBaseField::MinHeight(v) => base.min_size.height = v.max(0.0),
        WidgetBaseField::MaxWidth(v) => base.max_size.width = v.max(0.0),
        WidgetBaseField::MaxHeight(v) => base.max_size.height = v.max(0.0),
    }
    target.mark_dirty();
    crate::animation::request_draw();
    true
}

/// Apply a single queued inspector edit against the live widget tree.
/// Returns `true` if the edit landed; `false` if the path was stale or the
/// field path didn't resolve.
#[cfg(feature = "reflect")]
pub fn apply_inspector_edit(root: &mut dyn Widget, edit: &InspectorEdit) -> bool {
    use bevy_reflect::{GetPath, PartialReflect};
    let Some(target) = walk_path_mut(root, &edit.path) else {
        return false;
    };
    let applied;
    {
        let Some(reflected) = target.as_reflect_mut() else {
            return false;
        };
        let Ok(field) = reflected.reflect_path_mut(edit.field_path.as_str()) else {
            return false;
        };
        let field: &mut dyn PartialReflect = field;
        applied = field.try_apply(edit.new_value.as_ref()).is_ok();
    }
    // Reflection bypasses the widget's setters, which is where cache
    // invalidation normally happens (e.g. Label::set_text).  Hand the
    // widget a single-shot dirty signal so the next paint re-rasterises.
    if applied {
        target.mark_dirty();
        crate::animation::request_draw();
    }
    applied
}
