//! Inspector + reflection support for the widget tree: the flat
//! [`InspectorNode`] snapshot the F12-style inspector panel lists, the
//! reflection-driven field dump, and the transform math that places each
//! node on screen.
//!
//! Neighbours: `lookup` (id/type widget search, the world-mouse and
//! viewport cells), `edit` (path walking and inspector-originated edits),
//! `draw_report` (the runaway-repaint diagnostic).  All are re-exported here
//! so `tree_inspector::X` paths stay stable; `tree.rs` holds traversal and
//! event dispatch.

use super::*;

mod draw_report;
mod edit;
mod lookup;

pub use draw_report::debug_draw_report;
#[cfg(feature = "reflect")]
pub use edit::{apply_inspector_edit, InspectorEdit};
pub use edit::{apply_widget_base_edit, walk_path, walk_path_mut, WidgetBaseEdit, WidgetBaseField};
pub use lookup::{
    current_mouse_world, current_viewport, find_widget_by_id, find_widget_by_id_mut,
    find_widget_by_type, find_widget_screen_rect, set_current_mouse_world, set_current_viewport,
};

/// Flat snapshot of one widget for the inspector panel.
#[derive(Clone)]
pub struct InspectorNode {
    pub type_name: &'static str,
    /// Absolute screen bounds (Y-up), accumulated as the tree is walked.
    pub screen_bounds: Rect,
    /// Outer margin (per-side) in on-screen units, like `screen_bounds`: the
    /// widget's logical margin times `scale`.  Drawn as the orange band
    /// outside `screen_bounds` in the Chrome F12-style hover overlay.
    pub margin: crate::layout_props::Insets,
    /// Inner padding (per-side) in on-screen units — only nonzero on
    /// container widgets that override [`Widget::padding`].  Drawn as the
    /// green band inset from `screen_bounds`.
    pub padding: crate::layout_props::Insets,
    /// On-screen units per logical unit of this widget: the scale its
    /// ancestors apply (a zoomed node canvas), `1.0` with none.  Dividing
    /// `screen_bounds`, `margin` or `padding` by it gives the widget's own
    /// logical values, the units its edits are made in.
    pub scale: f64,
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
    /// The widget's own [`Widget::is_visible`] flag (an ancestor may still
    /// hide it), C#'s `InspectorNode.Visible`.  Hidden widgets are collected
    /// too, so the properties pane can show them again.
    pub visible: bool,
    /// The widget's own [`Widget::is_enabled`] flag (an ancestor may still
    /// disable it), C#'s `InspectorNode.Enabled`.
    pub enabled: bool,
    /// How many children the widget holds, hidden ones included (C#'s
    /// `InspectorNode.ChildCount`, `widget.Children.Count`).
    pub child_count: usize,
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

/// The transform from a widget's children's space to the screen: a point in
/// child space goes through the widget's `child_transform` first, then the
/// widget's bounds offset (its place in its parent's space), then the
/// parent's chain. The offset must be applied *before* the parent's chain,
/// not added to it afterwards: under a scaling ancestor (NodeEditor's zoomed
/// canvas) the widget's offset is in the ancestor's scaled space too.
pub(crate) fn compose_child_to_screen(
    parent_to_screen: &crate::TransAffine,
    bounds: Rect,
    child_transform: &crate::TransAffine,
) -> crate::TransAffine {
    let mut m = *child_transform;
    m.translate(bounds.x, bounds.y);
    m.multiply(parent_to_screen);
    m
}

/// AABB of `rect`'s four corners after passing through `t` — equals
/// `t(rect)` exactly for translation + uniform scale (no rotation), and
/// is the right conservative bound otherwise.
pub(crate) fn transform_rect_aabb(t: &crate::TransAffine, rect: Rect) -> Rect {
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

impl InspectorNode {
    /// `v`, an on-screen length of this node, in the widget's logical units.
    pub fn to_logical(&self, v: f64) -> f64 {
        if self.scale > 0.0 {
            v / self.scale
        } else {
            v
        }
    }
}

/// Whether `nodes[idx]` is on screen: it and every ancestor before it in the
/// depth-first list report visible.  A hidden widget keeps its row (see
/// [`collect_inspector_nodes`]) but occupies no pixels, so hover outlines
/// nothing for it.
pub(crate) fn inspector_node_on_screen(nodes: &[InspectorNode], idx: usize) -> bool {
    let Some(node) = nodes.get(idx) else {
        return false;
    };
    if !node.visible {
        return false;
    }
    let mut depth = node.depth;
    for ancestor in nodes[..idx].iter().rev() {
        if depth == 0 {
            break;
        }
        if ancestor.depth < depth {
            if !ancestor.visible {
                return false;
            }
            depth = ancestor.depth;
        }
    }
    true
}

fn collect_inspector_nodes_with_path(
    widget: &dyn Widget,
    depth: usize,
    parent_to_screen: &crate::TransAffine,
    path_prefix: &[usize],
    out: &mut Vec<InspectorNode>,
) {
    // Hidden widgets stay in the list, subtrees included, as agg-sharp's
    // `InspectorModel.Collect` walks every child: the `visible` row is how a
    // widget hidden from the inspector is shown again.  The panel draws
    // their rows dimmed and outlines nothing for them on the app.
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
        scale,
        h_anchor,
        v_anchor,
        depth,
        path: path_prefix.to_vec(),
        properties: props,
        visible: widget.is_visible(),
        enabled: widget.is_enabled(),
        child_count: widget.children().len(),
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
    let child_to_screen =
        compose_child_to_screen(parent_to_screen, b, &widget.inspector_child_transform());

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
