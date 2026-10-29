//! Inspector-originated edits: walking a child-index path back to a live
//! widget, the queued [`WidgetBaseEdit`] (margin, anchors, sizes, padding,
//! visibility, enabled) and, with the `reflect` feature, reflected
//! [`InspectorEdit`]s (agg-sharp's `InspectorEdit.Apply`).
//!
//! Part of `tree_inspector`; the inspector panel queues these and the host
//! frame loop drains them.
//!
//! A reflected edit travels in four steps:
//! 1. Each [`InspectorNode`] carries `path`, its child-index path from the
//!    root, recorded while the snapshot is collected.
//! 2. While painting the properties pane, the inspector panel
//!    (`widgets/inspector_props.rs`) records a hit rectangle for each row:
//!    a `true`/`false` row toggles, and a row that parses as a number steps
//!    down on its left half and up on its right, by 5% of the value's
//!    magnitude (at least 0.1). The step is sent as an `f64`, so it lands
//!    only on `f64` fields. A click on a row pushes an [`InspectorEdit`]
//!    (the node's path, the row's name as the field path, the new value)
//!    onto the queue bound with `InspectorPanel::with_edit_queue`.
//! 3. The host frame loop drains the queue before layout and paint (the
//!    demo's `render_app_frame` in `demo-wgpu/src/frame.rs`), since applying
//!    an edit during event dispatch or paint would change a tree that is
//!    being walked. [`apply_inspector_edit`] resolves the path with
//!    [`walk_path_mut`], the field with `reflect_path_mut` on the widget's
//!    `as_reflect_mut` value, and writes it with `try_apply`. A stale path,
//!    an unknown field or a value of the wrong type leaves the tree as it
//!    was and returns `false`.
//! 4. A landed edit calls [`Widget::mark_dirty`], which invalidates both the
//!    widget's backbuffer and its cached bitmap (a `Label`'s rasterised
//!    text), because reflection writes the field directly and skips the
//!    setters that would invalidate them. The host then refreshes the
//!    inspector snapshot so the new value shows at once.
//!
//! Which widgets have reflected values, and how a widget opts in, is on
//! `Widget::as_reflect` (`widget/widget_trait.rs`).

use super::super::*;

/// The widget at `path` under `root` (an empty path is `root`); `None` when
/// the path indexes past the children at any level.
pub fn walk_path<'a>(root: &'a dyn Widget, path: &[usize]) -> Option<&'a dyn Widget> {
    let mut node = root;
    for &idx in path {
        node = node.children().get(idx)?.as_ref();
    }
    Some(node)
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

/// One field of a widget that the inspector can change at runtime: its
/// [`crate::layout_props::WidgetBase`] (margin, anchors, size constraints,
/// width and height) or, through the [`Widget`] trait's `try_set_*` hooks,
/// its padding, visibility and enabled flag (agg-sharp's `InspectorEditField`).
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
    /// One side of the padding, through [`Widget::try_set_padding`].
    PaddingLeft(f64),
    PaddingRight(f64),
    PaddingTop(f64),
    PaddingBottom(f64),
    /// An explicit width (C# `GuiWidget.Width = …`): agg-gui has no width
    /// property, so this pins `min_size.width` and `max_size.width` to the
    /// value and the parent assigns exactly that width.
    Width(f64),
    /// An explicit height, pinning `min_size.height` and `max_size.height`.
    Height(f64),
    /// Show or hide the widget, through [`Widget::try_set_visible`].
    Visible(bool),
    /// Enable or disable the widget, through [`Widget::try_set_enabled`].
    Enabled(bool),
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
/// widget does not carry the field (no `WidgetBase`, or a `try_set_*` hook it
/// does not implement).  Sizes, margins and paddings never go below zero.
pub fn apply_widget_base_edit(root: &mut dyn Widget, edit: &WidgetBaseEdit) -> bool {
    let Some(target) = walk_path_mut(root, &edit.path) else {
        return false;
    };
    if !apply_trait_field(target, &edit.field) {
        let Some(base) = target.widget_base_mut() else {
            return false;
        };
        if !apply_base_field(base, &edit.field) {
            return false;
        }
    }
    target.mark_dirty();
    crate::animation::request_draw();
    true
}

/// The fields set through the `Widget` trait's `try_set_*` hooks.  `false`
/// for a `WidgetBase` field or a hook the widget leaves at its default.
fn apply_trait_field(target: &mut dyn Widget, field: &WidgetBaseField) -> bool {
    let mut padding = target.padding();
    match field {
        WidgetBaseField::PaddingLeft(v) => padding.left = v.max(0.0),
        WidgetBaseField::PaddingRight(v) => padding.right = v.max(0.0),
        WidgetBaseField::PaddingTop(v) => padding.top = v.max(0.0),
        WidgetBaseField::PaddingBottom(v) => padding.bottom = v.max(0.0),
        WidgetBaseField::Visible(on) => return target.try_set_visible(*on),
        WidgetBaseField::Enabled(on) => return target.try_set_enabled(*on),
        _ => return false,
    }
    target.try_set_padding(padding)
}

/// The fields stored in the widget's `WidgetBase`; `false` for the others.
fn apply_base_field(base: &mut crate::layout_props::WidgetBase, field: &WidgetBaseField) -> bool {
    match field {
        // A size never goes below zero (agg-sharp's `InspectorEdit.Apply`).
        WidgetBaseField::MarginLeft(v) => base.margin.left = v.max(0.0),
        WidgetBaseField::MarginRight(v) => base.margin.right = v.max(0.0),
        WidgetBaseField::MarginTop(v) => base.margin.top = v.max(0.0),
        WidgetBaseField::MarginBottom(v) => base.margin.bottom = v.max(0.0),
        WidgetBaseField::HAnchor(a) => base.h_anchor = *a,
        WidgetBaseField::VAnchor(a) => base.v_anchor = *a,
        WidgetBaseField::MinWidth(v) => base.min_size.width = v.max(0.0),
        WidgetBaseField::MinHeight(v) => base.min_size.height = v.max(0.0),
        WidgetBaseField::MaxWidth(v) => base.max_size.width = v.max(0.0),
        WidgetBaseField::MaxHeight(v) => base.max_size.height = v.max(0.0),
        WidgetBaseField::Width(v) => {
            base.min_size.width = v.max(0.0);
            base.max_size.width = v.max(0.0);
        }
        WidgetBaseField::Height(v) => {
            base.min_size.height = v.max(0.0);
            base.max_size.height = v.max(0.0);
        }
        _ => return false,
    }
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
