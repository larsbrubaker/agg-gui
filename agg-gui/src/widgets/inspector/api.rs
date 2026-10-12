//! Programmatic API of `InspectorPanel`: selection, expansion, the
//! tree/properties split, and the geometry hosts and tests read back
//! (agg-sharp's `InspectorModel.Select` / `SetExpanded` / `IsExpanded`,
//! `InspectorPanel.PropertiesHeight` / `ListAreaHeight` and
//! `InspectorPropertiesView.ValueBounds`).
//!
//! Node ids are positions in the snapshot the panel was last laid out
//! with (the host's `collect_inspector_nodes` order).  State and layout
//! live in `mod.rs`; event routing and painting in `widget_impl.rs`.

use crate::geometry::Rect;

use super::{InspectorPanel, MIN_PROPS_H};

impl InspectorPanel {
    /// Select snapshot node `idx` (or clear the selection with `None`),
    /// expanding its ancestors so its row shows, as agg-gui's tree does
    /// when a row is selected from outside.
    pub fn select(&mut self, idx: Option<usize>) {
        match idx.filter(|&i| i < self.tree_view.nodes.len()) {
            Some(i) => {
                self.tree_view.select_single(i);
                self.tree_view.expand_path_to(i);
            }
            None => self.tree_view.clear_selection(),
        }
        self.selected = self.tree_view.nodes.iter().position(|n| n.is_selected);
    }

    /// The selected snapshot node, if any.
    pub fn selected(&self) -> Option<usize> {
        self.tree_view.nodes.iter().position(|n| n.is_selected)
    }

    /// Whether snapshot node `idx` is expanded.
    pub fn is_expanded(&self, idx: usize) -> bool {
        self.tree_view.is_expanded(idx)
    }

    /// Expand or collapse snapshot node `idx`.
    pub fn set_expanded(&mut self, idx: usize, expanded: bool) {
        self.tree_view.set_expanded(idx, expanded);
    }

    /// Whether the tree holds the keyboard (a row was clicked and focus
    /// has not moved on).
    pub fn tree_has_focus(&self) -> bool {
        self.tree_focused
    }

    /// Height asked for the properties pane: the split line's distance
    /// from the panel bottom.  Layout keeps each pane at least 60 tall.
    pub fn properties_height(&self) -> f64 {
        self.props_h
    }

    /// Move the split line to `height` above the panel bottom (at least
    /// the properties pane's 60 minimum).
    pub fn set_properties_height(&mut self, height: f64) {
        self.props_h = height.max(MIN_PROPS_H);
        crate::animation::request_draw();
    }

    /// Height of the area below the header that the tree and the
    /// properties pane share.
    pub fn list_area_height(&self) -> f64 {
        self.list_area_h()
    }

    /// Where the split line sits after clamping, from the panel bottom.
    pub fn split_line_y(&self) -> f64 {
        self.split_y()
    }

    /// Panel-local bounds of the clickable value strip of the editable
    /// property row `field` (`"margin.left"`, `"h_anchor"`, ...), as the
    /// last paint laid it out.  `None` for a read-only or unpainted row.
    pub fn value_bounds(&self, field: &str) -> Option<Rect> {
        self.prop_hits
            .iter()
            .find(|h| h.field == field)
            .map(|h| h.rect)
    }

    /// Panel-local rectangle of the box-model preview (the selected
    /// widget's shape below the rows) as last painted; `None` when the
    /// properties pane has no room for it.
    pub fn box_preview_bounds(&self) -> Option<Rect> {
        self.box_preview
    }
}
