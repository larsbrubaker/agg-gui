//! Data types for `TreeView`: the node record, its icon kinds, the flat-row
//! record and the drag/drop state.
//!
//! The visible-row list itself is built and cached by `flat.rs`;
//! `flatten_visible` is the uncached reference walk, kept for tests.

use crate::color::Color;
use crate::geometry::Point;
use crate::icon_image::IconImage;

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// A node in the tree.
pub struct TreeNode {
    pub label: String,
    pub icon: NodeIcon,
    /// Image drawn instead of the procedural `icon` when `Some` (set with
    /// `TreeView::set_node_icon_image`).  `icon` still decides drop
    /// behaviour (folders and packages accept children).
    pub icon_image: Option<IconImage>,
    /// Font glyph drawn instead of the procedural `icon` (and of
    /// `icon_image`) when `Some` — e.g. a Font Awesome code point.  Drawn as
    /// text at paint time, so it stays sharp when the display scale changes.
    /// Set with `TreeView::set_node_icon_glyph`.
    pub icon_glyph: Option<NodeGlyph>,
    /// Right-aligned secondary text at the row's trailing edge (a size, a
    /// count), drawn dimmed.
    pub secondary_text: Option<String>,
    /// Small horizontal bar at the row's trailing edge, filled to this
    /// fraction (clamped to `0..=1`) — e.g. a share of the parent's size.
    pub fraction: Option<f32>,
    /// Show the expand arrow even though no children have been added yet —
    /// for lazily populated trees, which add the children on
    /// [`TreeViewEvent::Expanded`](super::TreeViewEvent::Expanded).
    pub may_have_children: bool,
    /// Index of the parent node; `None` means root-level.
    pub parent: Option<usize>,
    pub is_expanded: bool,
    pub is_selected: bool,
    /// Sibling ordering key. Lower values appear first (visually higher).
    pub order: u32,
}

impl TreeNode {
    pub fn new(
        label: impl Into<String>,
        icon: NodeIcon,
        parent: Option<usize>,
        order: u32,
    ) -> Self {
        Self {
            label: label.into(),
            icon,
            icon_image: None,
            icon_glyph: None,
            secondary_text: None,
            fraction: None,
            may_have_children: false,
            parent,
            is_expanded: false,
            is_selected: false,
            order,
        }
    }
}

/// Procedurally-drawn icon discriminant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeIcon {
    Folder,
    File,
    Package,
}

/// A font glyph used as a node icon (see [`TreeNode::icon_glyph`]).  The
/// glyph is drawn with the tree's icon font (`TreeView::with_icon_font`,
/// falling back to the row font, whose standard fallback chain includes
/// Font Awesome) in `color`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeGlyph {
    pub glyph: char,
    pub color: Color,
}

impl NodeGlyph {
    pub fn new(glyph: char, color: Color) -> Self {
        Self { glyph, color }
    }
}

// ---------------------------------------------------------------------------
// Flat-row representation (cached by `flat.rs`)
// ---------------------------------------------------------------------------

/// One visible row after DFS expansion of the tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatRow {
    /// Index into `TreeView::nodes`.
    pub node_idx: usize,
    pub depth: u32,
    pub has_children: bool,
}

/// Produce an ordered list of visible rows by DFS traversal, respecting
/// `is_expanded`.  Nodes at each level are sorted by `order`.
///
/// O(nodes × visible rows): the reference the cached `flat::FlatCache` is
/// tested against.  `TreeView` itself never calls it.
#[cfg(test)]
pub fn flatten_visible(nodes: &[TreeNode]) -> Vec<FlatRow> {
    if nodes.is_empty() {
        return Vec::new();
    }

    // Root-level nodes sorted by order.
    let mut roots: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.parent.is_none())
        .map(|(i, _)| i)
        .collect();
    roots.sort_by_key(|&i| nodes[i].order);

    let mut result = Vec::new();
    // Stack of (node_idx, depth); push in reverse so pop gives DFS order.
    let mut stack: Vec<(usize, u32)> = roots.into_iter().rev().map(|i| (i, 0)).collect();

    while let Some((node_idx, depth)) = stack.pop() {
        let mut children: Vec<usize> = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.parent == Some(node_idx))
            .map(|(i, _)| i)
            .collect();
        children.sort_by_key(|&i| nodes[i].order);

        let has_children = !children.is_empty() || nodes[node_idx].may_have_children;
        result.push(FlatRow {
            node_idx,
            depth,
            has_children,
        });

        if nodes[node_idx].is_expanded && has_children {
            for &child in children.iter().rev() {
                stack.push((child, depth + 1));
            }
        }
    }

    result
}

/// Returns `true` if `node_idx` is a descendant of `ancestor_idx`.
pub fn is_descendant(nodes: &[TreeNode], ancestor_idx: usize, mut node_idx: usize) -> bool {
    loop {
        match nodes[node_idx].parent {
            None => return false,
            Some(p) if p == ancestor_idx => return true,
            Some(p) => node_idx = p,
        }
    }
}

// ---------------------------------------------------------------------------
// Interaction state
// ---------------------------------------------------------------------------

/// Drop position relative to a reference node (by node index).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropPosition {
    Before(usize),  // insert before this node_idx in the parent's child list
    After(usize),   // insert after this node_idx in the parent's child list
    AsChild(usize), // make a child of this node_idx
}

/// Active drag-and-drop gesture.
pub struct DragState {
    /// Node being dragged.
    pub node_idx: usize,
    /// Where in the row the cursor was when the drag started (offset from row bottom).
    pub _cursor_row_offset: f64,
    /// Current cursor position in TreeView local coordinates.
    pub current_pos: Point,
    /// `true` once the drag threshold (4 px) has been exceeded.
    pub live: bool,
}
