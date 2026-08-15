//! Inspector + reflection support for the widget tree: flat tree
//! snapshots for the F12-style inspector panel, reflection-driven field
//! dumps, id/type widget lookup, and inspector-originated edits.
//!
//! Split out of `tree.rs` (traversal + event dispatch) to keep both
//! files under the 800-line cap.

use super::*;

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
    let mut child_to_screen = *parent_to_screen;
    child_to_screen.translate(b.x, b.y);
    child_to_screen.premultiply(&widget.inspector_child_transform());
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

/// Walk the subtree rooted at `widget` and collect an `InspectorNode` per
/// widget in DFS paint order (root first).
///
/// `screen_origin` is the accumulated parent offset in screen Y-up coords.
/// Widgets that apply additional transforms between themselves and their
/// children (NodeEditor's pan/zoom, etc.) opt in via
/// [`Widget::inspector_child_transform`]; the traversal composes those
/// transforms so descendant `screen_bounds` reflect what the user sees.
pub fn collect_inspector_nodes(
    widget: &dyn Widget,
    depth: usize,
    screen_origin: Point,
    out: &mut Vec<InspectorNode>,
) {
    let mut parent_to_screen = crate::TransAffine::new();
    parent_to_screen.translate(screen_origin.x, screen_origin.y);
    collect_inspector_nodes_with_path(widget, depth, &parent_to_screen, &[], out);
}

/// AABB of `rect`'s four corners after passing through `t` — equals
/// `t(rect)` exactly for translation + uniform scale (no rotation), and
/// is the right conservative bound otherwise.
fn transform_rect_aabb(t: &crate::TransAffine, rect: Rect) -> Rect {
    let corners = [
        (rect.x, rect.y),
        (rect.x + rect.width, rect.y),
        (rect.x, rect.y + rect.height),
        (rect.x + rect.width, rect.y + rect.height),
    ];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (mut x, mut y) in corners {
        t.transform(&mut x, &mut y);
        if x < min_x {
            min_x = x;
        }
        if x > max_x {
            max_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if y > max_y {
            max_y = y;
        }
    }
    Rect::new(
        min_x,
        min_y,
        (max_x - min_x).max(0.0),
        (max_y - min_y).max(0.0),
    )
}

/// Effective uniform scale extracted from `t` — used to scale logical
/// margin / padding insets so the F12-style overlay bands stay
/// visually proportional to the (scaled) widget bounds.  For pure
/// translation + uniform scale this returns the scale exactly; under
/// non-uniform scale it returns the geometric mean of the axes, which
/// is the right "single number" for matching the visible band width.
fn effective_scale(t: &crate::TransAffine) -> f64 {
    let sx = (t.sx * t.sx + t.shy * t.shy).sqrt();
    let sy = (t.shx * t.shx + t.sy * t.sy).sqrt();
    if sx > 0.0 && sy > 0.0 {
        (sx * sy).sqrt()
    } else {
        1.0
    }
}

fn collect_inspector_nodes_with_path(
    widget: &dyn Widget,
    depth: usize,
    parent_to_screen: &crate::TransAffine,
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
    // Transform the widget's parent-local bounds rect through the
    // accumulated parent_to_screen transform.  For the common case
    // (pure translation, no scale) this collapses to the old
    // `screen_origin + b.x/y` math; under scale it now correctly
    // reports the on-screen size.
    let abs = transform_rect_aabb(parent_to_screen, b);
    let scale = effective_scale(parent_to_screen);
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
    let margin_logical = widget.margin();
    let padding_logical = widget.padding();
    let scaled_margin = crate::layout_props::Insets {
        left: margin_logical.left * scale,
        right: margin_logical.right * scale,
        top: margin_logical.top * scale,
        bottom: margin_logical.bottom * scale,
    };
    let scaled_padding = crate::layout_props::Insets {
        left: padding_logical.left * scale,
        right: padding_logical.right * scale,
        top: padding_logical.top * scale,
        bottom: padding_logical.bottom * scale,
    };
    out.push(InspectorNode {
        type_name: widget.type_name(),
        screen_bounds: abs,
        margin: scaled_margin,
        padding: scaled_padding,
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

    // Compose the transform for child traversal.  Order matches the
    // paint pipeline: parent_to_screen ∘ translate(b.x, b.y) ∘
    // widget.inspector_child_transform().  That is, applied to a point
    // in child-local space, we first run the widget's own child
    // transform (e.g. NodeEditor's pan/zoom), then translate by the
    // widget's bounds offset (its position in its parent), then through
    // the parent_to_screen chain.
    let mut child_to_screen = *parent_to_screen;
    child_to_screen.translate(b.x, b.y);
    let extra = widget.inspector_child_transform();
    child_to_screen.premultiply(&extra);

    let mut child_path: Vec<usize> = Vec::with_capacity(path_prefix.len() + 1);
    child_path.extend_from_slice(path_prefix);
    child_path.push(0);
    for (i, child) in widget.children().iter().enumerate() {
        *child_path.last_mut().unwrap() = i;
        collect_inspector_nodes_with_path(
            child.as_ref(),
            depth + 1,
            &child_to_screen,
            &child_path,
            out,
        );
    }
}

// ---------------------------------------------------------------------------
// Runaway-repaint diagnostic report
// ---------------------------------------------------------------------------

/// Recursively collect every *visible* widget whose [`Widget::needs_draw`]
/// currently returns `true`, recording its child-index path, type name, and
/// whether it is "self-hot" (no visible child is independently asking for a
/// draw, so this widget's own state is the driver).
///
/// The walk descends into **all** visible children — it does not prune on a
/// parent whose `needs_draw()` is `false`.  That deliberately catches
/// propagation gaps (a hot child under a container that forgot to OR its
/// children into `needs_draw`, the exact class of bug behind the intermittent
/// runaway), which a prune-on-false walk would hide.
fn collect_needs_draw(
    widget: &dyn Widget,
    path: &mut Vec<usize>,
    out: &mut Vec<(Vec<usize>, &'static str, bool)>,
) {
    // Same visibility gate the host loop uses: invisible subtrees never keep
    // the app awake, so they are irrelevant to a runaway and are skipped.
    if !widget.is_visible() {
        return;
    }
    if widget.needs_draw() {
        let child_hot = widget.children().iter().any(|c| c.needs_draw());
        out.push((path.clone(), widget.type_name(), !child_hot));
    }
    for (i, child) in widget.children().iter().enumerate() {
        path.push(i);
        collect_needs_draw(child.as_ref(), path, out);
        path.pop();
    }
}

/// Build a human-readable diagnostic naming everything currently keeping the
/// reactive host awake, for capturing the intermittent "continuous rendering
/// never quiesces" runaway on real hardware.
///
/// The report contains, in order:
/// 1. the raw immediate-draw flag (read side-effect-free — see
///    [`crate::animation::peek_draw_signals`], which unlike `wants_draw`
///    neither pumps async wakeups nor clears a due deadline),
/// 2. the next scheduled-draw deadline with remaining time (and a `DUE`
///    marker when already past),
/// 3. the drained draw-request provenance tags, deduplicated with counts
///    (always empty in release builds — the trace is debug-only), and
/// 4. every visible widget whose `needs_draw()` is `true`, as `[path] Type`,
///    with a `<- self` marker on the ones whose own state (not a child's)
///    is the driver.
///
/// Takes the tree `root` as a parameter so it can be invoked from a host
/// shell that holds `&App` (`app.root()`), matching the introspection style
/// of [`collect_inspector_nodes`].
#[doc(hidden)]
pub fn debug_draw_report(root: &dyn Widget) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let (needs_flag, deadline) = crate::animation::peek_draw_signals();
    let now = web_time::Instant::now();

    let _ = writeln!(s, "== agg-gui draw report ==");
    let _ = writeln!(s, "immediate needs_draw flag: {needs_flag}");
    match deadline {
        Some(when) => {
            let remaining_ms = when.saturating_duration_since(now).as_secs_f64() * 1000.0;
            let due = now >= when;
            let _ = writeln!(
                s,
                "next scheduled deadline: {remaining_ms:.1} ms{}",
                if due { " (DUE)" } else { "" }
            );
        }
        None => {
            let _ = writeln!(s, "next scheduled deadline: none");
        }
    }

    // (c) Drained provenance tags, deduplicated with counts (most-frequent
    // first).  Draining is what the quiescence guard already does; doing it
    // here means a subsequent report starts from a clean trace.
    let tags = crate::animation::drain_draw_trace();
    if tags.is_empty() {
        let _ = writeln!(
            s,
            "draw-trace tags: none (empty in release builds — trace is debug-only)"
        );
    } else {
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for t in tags {
            if let Some(entry) = counts.iter_mut().find(|(name, _)| *name == t) {
                entry.1 += 1;
            } else {
                counts.push((t, 1));
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let _ = writeln!(s, "draw-trace tags ({} distinct):", counts.len());
        for (name, count) in counts {
            let _ = writeln!(s, "  {count:>4} x {name}");
        }
    }

    // (d) Every visible widget whose needs_draw() is true.
    let mut hot: Vec<(Vec<usize>, &'static str, bool)> = Vec::new();
    collect_needs_draw(root, &mut Vec::new(), &mut hot);
    if hot.is_empty() {
        let _ = writeln!(s, "widgets wanting draw: none");
    } else {
        let _ = writeln!(s, "widgets wanting draw ({}):", hot.len());
        for (path, type_name, self_hot) in hot {
            let path_str = if path.is_empty() {
                "root".to_string()
            } else {
                path.iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            };
            let _ = writeln!(
                s,
                "  [{path_str}] {type_name}{}",
                if self_hot { "  <- self" } else { "" }
            );
        }
    }

    s
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
