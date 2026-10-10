//! Public programmatic API of `TreeView`: selection and keyboard cursor,
//! expansion, scrolling a node into view, per-row trailing content, node
//! removal, and change notification ([`TreeViewEvent`]).
//!
//! Change notification follows the other agg-gui widgets' callback style
//! ([`TreeView::on_tree_event`]) and adds a drain
//! ([`TreeView::take_events`]) for owners that must edit the tree in
//! response — a lazily populated tree adds a folder's children on
//! [`TreeViewEvent::Expanded`], which a callback cannot do while the
//! `TreeView` is borrowed.  Events report what the *user* did (mouse and
//! keyboard); the programmatic calls here never emit them.
//!
//! Split out of `mod.rs` (800-line cap); see `mod.rs` for the module map.

use std::collections::HashSet;

use crate::geometry::Rect;

use super::node::NodeGlyph;
use super::{PendingScroll, TreeView, SCROLLBAR_W};

/// Most events kept for [`TreeView::take_events`]; the oldest are dropped
/// first when nobody drains them.
const MAX_QUEUED_EVENTS: usize = 1024;

/// A change the user made in a [`TreeView`].  Node ids are indices into
/// `TreeView::nodes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeViewEvent {
    /// The set of selected nodes changed (click, arrow keys, ctrl / shift
    /// click).  `cursor` is the keyboard cursor node after the change; read
    /// the full selection with [`TreeView::selected_nodes`].
    SelectionChanged { cursor: Option<usize> },
    /// A node was expanded (toggle arrow, row click, arrow key, Space/Enter).
    /// Lazily populated trees add the node's children now.
    Expanded(usize),
    /// A node was collapsed.
    Collapsed(usize),
    /// A row was activated: double-clicked, or Enter on the cursor row.
    Activated(usize),
}

/// Where [`TreeView::scroll_node_into_view`] puts the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAlign {
    /// Scroll the least amount that shows the whole row; no scroll when it
    /// is already in view.
    Minimal,
    /// Centre the row in the viewport when it is not already in view.
    Center,
}

/// Scroll offset that shows visible row `row` per `align`, given the current
/// `offset`, viewport height and content height.
pub(super) fn scroll_for_row(
    row: usize,
    align: ScrollAlign,
    offset: f64,
    viewport_h: f64,
    row_h: f64,
    content_h: f64,
) -> f64 {
    let max = (content_h - viewport_h).max(0.0);
    // Distance of the row's top edge from the content's top.
    let top = row as f64 * row_h;
    let in_view = top >= offset && top + row_h <= offset + viewport_h;
    let wanted = match align {
        _ if in_view => offset,
        ScrollAlign::Minimal if top < offset => top,
        ScrollAlign::Minimal => top + row_h - viewport_h,
        ScrollAlign::Center => top + row_h * 0.5 - viewport_h * 0.5,
    };
    wanted.clamp(0.0, max)
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

impl TreeView {
    /// Call `cb` for every [`TreeViewEvent`] as it happens.  The events are
    /// also queued for [`take_events`](Self::take_events).
    pub fn on_tree_event(mut self, cb: impl FnMut(&TreeViewEvent) + 'static) -> Self {
        self.event_cb = Some(Box::new(cb));
        self
    }

    /// The events since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<TreeViewEvent> {
        std::mem::take(&mut self.events)
    }

    pub(super) fn emit(&mut self, event: TreeViewEvent) {
        if let Some(cb) = self.event_cb.as_mut() {
            cb(&event);
        }
        if self.events.len() >= MAX_QUEUED_EVENTS {
            self.events.remove(0);
        }
        self.events.push(event);
    }
}

// ---------------------------------------------------------------------------
// Selection and cursor
// ---------------------------------------------------------------------------

impl TreeView {
    /// Select only `idx` and put the keyboard cursor on it (so arrow keys
    /// continue from there).  Out-of-range ids are ignored.  Does not scroll;
    /// pair with [`scroll_node_into_view`](Self::scroll_node_into_view).
    pub fn select_single(&mut self, idx: usize) {
        if idx < self.nodes.len() {
            self.set_single_selection(idx);
            crate::animation::request_draw();
        }
    }

    /// Deselect every node (the cursor stays where it is).
    pub fn clear_selection(&mut self) {
        for n in &mut self.nodes {
            n.is_selected = false;
        }
        crate::animation::request_draw();
    }

    /// Ids of the selected nodes, in index order.
    pub fn selected_nodes(&self) -> Vec<usize> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.is_selected)
            .map(|(i, _)| i)
            .collect()
    }

    /// The keyboard cursor (also the shift-click anchor).
    pub fn cursor_node(&self) -> Option<usize> {
        self.cursor_node
    }

    /// Move the keyboard cursor without changing the selection.
    pub fn set_cursor_node(&mut self, idx: Option<usize>) {
        self.cursor_node = idx.filter(|&i| i < self.nodes.len());
    }
}

// ---------------------------------------------------------------------------
// Expansion
// ---------------------------------------------------------------------------

impl TreeView {
    /// Expand or collapse the node at `idx`.
    pub fn set_expanded(&mut self, idx: usize, expanded: bool) {
        if let Some(n) = self.nodes.get_mut(idx) {
            n.is_expanded = expanded;
            crate::animation::request_draw();
        }
    }

    /// Collapse the node at `idx`.
    pub fn collapse(&mut self, idx: usize) {
        self.set_expanded(idx, false);
    }

    pub fn is_expanded(&self, idx: usize) -> bool {
        self.nodes.get(idx).is_some_and(|n| n.is_expanded)
    }

    /// Expand every ancestor of `idx`, so its row becomes visible (`idx`
    /// itself keeps its state).  Stops at a parent cycle.
    pub fn expand_path_to(&mut self, idx: usize) {
        let mut seen = HashSet::new();
        let mut cur = self.nodes.get(idx).and_then(|n| n.parent);
        while let Some(p) = cur.filter(|&p| p < self.nodes.len() && seen.insert(p)) {
            self.nodes[p].is_expanded = true;
            cur = self.nodes[p].parent;
        }
        crate::animation::request_draw();
    }

    /// Show the expand arrow on `idx` before it has children (lazy loading;
    /// see [`TreeNode::may_have_children`](super::TreeNode::may_have_children)).
    pub fn set_node_may_have_children(&mut self, idx: usize, may: bool) {
        if let Some(n) = self.nodes.get_mut(idx) {
            n.may_have_children = may;
        }
    }
}

// ---------------------------------------------------------------------------
// Scrolling and row geometry
// ---------------------------------------------------------------------------

impl TreeView {
    /// Scroll so `idx`'s row is in view, at the next layout (when the rows
    /// reflect any expansion done just before).  Expand its ancestors first
    /// ([`expand_path_to`](Self::expand_path_to)); a hidden node is ignored.
    pub fn scroll_node_into_view(&mut self, idx: usize, align: ScrollAlign) {
        self.pending_scroll = Some(PendingScroll::Node(idx, align));
        crate::animation::request_draw();
    }

    /// Scroll so visible row `row` (0 = top row) is in view, at the next
    /// layout.
    pub fn scroll_to_row(&mut self, row: usize, align: ScrollAlign) {
        self.pending_scroll = Some(PendingScroll::Row(row, align));
        crate::animation::request_draw();
    }

    /// Pixels the content is scrolled down from the top.
    pub fn scroll_offset(&self) -> f64 {
        self.scroll_offset
    }

    /// Set the scroll offset (clamped at the next layout).
    pub fn set_scroll_offset(&mut self, offset: f64) {
        self.scroll_offset = offset.max(0.0);
        crate::animation::request_draw();
    }

    /// Resolve a pending scroll request against the current rows; called by
    /// `layout()` once `viewport_h` and `content_height` are current.
    pub(super) fn apply_pending_scroll(&mut self) {
        let Some(req) = self.pending_scroll.take() else {
            return;
        };
        let (row, align) = match req {
            PendingScroll::Row(r, a) => (Some(r), a),
            PendingScroll::Node(n, a) => (self.flat.position_of(n), a),
        };
        if let Some(row) = row.filter(|&r| r < self.flat.rows.len()) {
            // The hovered row index refers to the old scroll position.
            self.hovered_row = None;
            self.scroll_offset = scroll_for_row(
                row,
                align,
                self.scroll_offset,
                self.viewport_h,
                self.row_height,
                self.content_height,
            );
        }
    }

    /// Number of visible rows (all expanded rows, not just those on screen).
    pub fn visible_row_count(&mut self) -> usize {
        self.refresh_flat();
        self.flat.rows.len()
    }

    /// Visible-row position of `idx` (0 = top row), or `None` when a
    /// collapsed ancestor hides it.
    pub fn visible_row_of(&mut self, idx: usize) -> Option<usize> {
        self.refresh_flat();
        self.flat.position_of(idx)
    }

    /// The node shown at visible row `row`.
    pub fn node_at_row(&mut self, row: usize) -> Option<usize> {
        self.refresh_flat();
        self.flat.rows.get(row).map(|r| r.node_idx)
    }

    /// `idx`'s row rectangle in TreeView-local (Y-up) coordinates at the
    /// current scroll offset, excluding the scrollbar strip; `None` when the
    /// row is hidden.  It may lie partly or wholly outside the viewport.
    pub fn node_row_rect(&mut self, idx: usize) -> Option<Rect> {
        let row = self.visible_row_of(idx)?;
        let y = self.viewport_h - (row as f64 + 1.0) * self.row_height + self.scroll_offset;
        let w = (self.bounds.width - SCROLLBAR_W).max(0.0);
        Some(Rect::new(0.0, y, w, self.row_height))
    }
}

// ---------------------------------------------------------------------------
// Per-row content
// ---------------------------------------------------------------------------

impl TreeView {
    /// Draw `glyph` as `idx`'s icon (`None` restores the image / procedural
    /// icon).
    pub fn set_node_icon_glyph(&mut self, idx: usize, glyph: Option<NodeGlyph>) {
        if let Some(n) = self.nodes.get_mut(idx) {
            n.icon_glyph = glyph;
        }
    }

    /// Right-aligned, dimmed secondary text for `idx`'s row.
    pub fn set_node_secondary_text(&mut self, idx: usize, text: Option<String>) {
        if let Some(n) = self.nodes.get_mut(idx) {
            n.secondary_text = text;
        }
    }

    /// Trailing fraction bar (`0..=1`) for `idx`'s row.
    pub fn set_node_fraction(&mut self, idx: usize, fraction: Option<f32>) {
        if let Some(n) = self.nodes.get_mut(idx) {
            n.fraction = fraction;
        }
    }

    /// Tooltip shown while the pointer is over `idx`'s row (`None` clears
    /// it).  Keyed by node, so moving to another row re-arms the tooltip;
    /// while the row's name is elided the tip is the full name with this
    /// text on the next line.
    pub fn set_node_tooltip(&mut self, idx: usize, tooltip: Option<String>) {
        if let Some(n) = self.nodes.get_mut(idx) {
            n.tooltip = tooltip;
        }
    }
}

// ---------------------------------------------------------------------------
// Removal
// ---------------------------------------------------------------------------

impl TreeView {
    /// Remove every descendant of `idx` (it stays, childless and collapsed).
    ///
    /// Node ids are indices, so removal shifts the ids of later nodes down.
    /// Returns the old-id → new-id map (`None` for removed nodes) so owners
    /// can remap ids they keep.  The cursor, hover and selection follow
    /// their nodes; a drag in progress is cancelled.  O(nodes).
    pub fn remove_children(&mut self, idx: usize) -> Vec<Option<usize>> {
        let remap = self.remove_where(idx, false);
        if let Some(n) = self
            .nodes
            .get_mut(remap.get(idx).copied().flatten().unwrap_or(usize::MAX))
        {
            n.is_expanded = false;
        }
        remap
    }

    /// Remove `idx` and all its descendants.  See
    /// [`remove_children`](Self::remove_children) for the returned id map.
    pub fn remove_node(&mut self, idx: usize) -> Vec<Option<usize>> {
        self.remove_where(idx, true)
    }

    /// Remove `idx`'s subtree (`include_root`: `idx` too); returns the id map.
    fn remove_where(&mut self, idx: usize, include_root: bool) -> Vec<Option<usize>> {
        let n = self.nodes.len();
        if idx >= n {
            return (0..n).map(Some).collect();
        }
        self.refresh_flat();
        let mut remove = vec![false; n];
        let mut stack: Vec<usize> = self.flat.children(idx).to_vec();
        while let Some(i) = stack.pop() {
            if !remove[i] {
                remove[i] = true;
                stack.extend_from_slice(self.flat.children(i));
            }
        }
        remove[idx] = include_root;

        let mut remap = vec![None; n];
        let mut next = 0;
        for (i, slot) in remap.iter_mut().enumerate() {
            if !remove[i] {
                *slot = Some(next);
                next += 1;
            }
        }
        let old = std::mem::take(&mut self.nodes);
        self.nodes = old
            .into_iter()
            .zip(&remove)
            .filter(|(_, &gone)| !gone)
            .map(|(mut node, _)| {
                // A kept node's parent is kept (subtrees go whole); a parent
                // id past the end stays dangling, as before.
                node.parent = node
                    .parent
                    .map(|p| remap.get(p).copied().flatten().unwrap_or(p));
                node
            })
            .collect();

        let map = |id: Option<usize>| id.and_then(|i| remap.get(i).copied().flatten());
        self.cursor_node = map(self.cursor_node);
        self.pending_scroll = match self.pending_scroll {
            Some(PendingScroll::Node(i, a)) => map(Some(i)).map(|i| PendingScroll::Node(i, a)),
            other => other,
        };
        self.hovered_row = None;
        self.drag = None;
        self.drop_target = None;
        self.counted_len = usize::MAX;
        self.flat.invalidate();
        crate::animation::request_draw();
        remap
    }
}
