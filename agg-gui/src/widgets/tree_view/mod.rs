//! `TreeView` — compositional tree widget with expand/collapse, multi-select,
//! keyboard navigation, and drag-and-drop reordering.
//!
//! Each visible row is represented by a `TreeRow` child widget stored in
//! `row_widgets`.  The framework recurses into these children after `paint()`
//! returns, so the `clip_rect` set at the end of `paint()` is active during
//! child painting.

mod drag;
mod node;
pub mod row;
mod widget_impl;

use drag::{apply_drop, compute_drop_target};
use node::{flatten_visible, DragState, DropPosition, FlatRow};
pub use node::{NodeIcon, TreeNode};
pub use row::{ExpandToggle, NodeIconWidget, TreeRow};

use std::sync::Arc;

use crate::event::{EventResult, Key, Modifiers};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

const SCROLLBAR_W: f64 = 10.0;
const DRAG_THRESHOLD: f64 = 4.0;

// ---------------------------------------------------------------------------
// RowMeta
// ---------------------------------------------------------------------------

/// Metadata for one visible row; parallel to `row_widgets` after `layout()`.
struct RowMeta {
    /// Index into `self.nodes` for this row.
    node_idx: usize,
    /// Bounds of the `ExpandToggle` in **TreeView-local** coordinates.
    /// `None` if the node has no children.
    toggle_rect: Option<Rect>,
}

// ---------------------------------------------------------------------------
// TreeView struct
// ---------------------------------------------------------------------------

pub struct TreeView {
    bounds: Rect,
    /// One `TreeRow` per currently-visible node; rebuilt each `layout()` call.
    row_widgets: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    /// Parallel to `row_widgets` — metadata for hit-testing in `on_event()`.
    row_metas: Vec<RowMeta>,

    pub nodes: Vec<TreeNode>,

    // Scroll state
    scroll_offset: f64,
    content_height: f64,

    // Row metrics
    pub row_height: f64,
    pub indent_width: f64,
    pub font: Arc<Font>,
    pub font_size: f64,

    // Interaction
    pub drag_enabled: bool,
    /// When `true`, clicking anywhere on a row that has children also toggles
    /// its expansion state.  When `false` (the default), only the expand-toggle
    /// arrow collapses/expands; clicks elsewhere only select.
    ///
    /// Set to `true` for file-explorer-style trees (the demo Tree tab).
    /// Leave `false` for the inspector tree, where clicking selects without
    /// accidentally collapsing an expanded branch.
    pub toggle_on_row_click: bool,
    hover_repaint: bool,
    focused: bool,
    /// Flat-row index of the row under the cursor.
    hovered_row: Option<usize>,
    /// Node index used as the keyboard cursor / shift-click anchor.
    cursor_node: Option<usize>,
    /// Active drag gesture.
    drag: Option<DragState>,
    /// Current computed drop target.
    drop_target: Option<DropPosition>,

    // Scrollbar drag
    hovered_scrollbar: bool,
    dragging_scrollbar: bool,
    sb_drag_start_y: f64,
    sb_drag_start_offset: f64,

    /// Hash of the row-content state at the last `layout()` rebuild —
    /// covers everything that affects WHICH `TreeRow` widgets exist
    /// (node order, label text, expand / select / hover / focus state)
    /// but NOT what affects only their bounds (viewport size, scroll
    /// offset).  When the next `layout()` finds an unchanged signature,
    /// it reuses the cached `row_widgets` Vec — preserving each Label's
    /// backbuffer cache — and only repositions them.  Without this,
    /// resizing a window with a 250+-row tree (the inspector) re-rasterised
    /// every label every frame.
    last_row_content_sig: Option<u64>,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl TreeView {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            row_widgets: Vec::new(),
            base: WidgetBase::new(),
            row_metas: Vec::new(),
            nodes: Vec::new(),
            scroll_offset: 0.0,
            content_height: 0.0,
            row_height: 24.0,
            indent_width: 16.0,
            font,
            font_size: 13.0,
            drag_enabled: false,
            toggle_on_row_click: false,
            hover_repaint: true,
            focused: false,
            hovered_row: None,
            cursor_node: None,
            drag: None,
            drop_target: None,
            hovered_scrollbar: false,
            dragging_scrollbar: false,
            sb_drag_start_y: 0.0,
            sb_drag_start_offset: 0.0,
            last_row_content_sig: None,
        }
    }

    /// Hash of everything that affects WHICH `TreeRow` widgets we'd build
    /// — but not their bounds.  Used by `layout()` to skip rebuilding the
    /// row widget vec when the user is just resizing the parent (window
    /// resize, scroll, etc.) and the underlying node list hasn't moved.
    fn row_content_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.nodes.len().hash(&mut h);
        for n in &self.nodes {
            n.label.hash(&mut h);
            n.parent.hash(&mut h);
            n.order.hash(&mut h);
            n.is_expanded.hash(&mut h);
            n.is_selected.hash(&mut h);
            (n.icon as u8).hash(&mut h);
        }
        self.hovered_row.hash(&mut h);
        self.focused.hash(&mut h);
        // Drag state affects which row to skip in the build.
        self.drag
            .as_ref()
            .map(|d| (d.live, d.node_idx))
            .hash(&mut h);
        self.font_size.to_bits().hash(&mut h);
        self.row_height.to_bits().hash(&mut h);
        self.indent_width.to_bits().hash(&mut h);
        h.finish()
    }

    pub fn with_row_height(mut self, h: f64) -> Self {
        self.row_height = h;
        self
    }
    pub fn with_indent_width(mut self, w: f64) -> Self {
        self.indent_width = w;
        self
    }
    pub fn with_font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }
    pub fn with_drag_enabled(mut self) -> Self {
        self.drag_enabled = true;
        self
    }
    pub fn with_toggle_on_row_click(mut self) -> Self {
        self.toggle_on_row_click = true;
        self
    }
    pub fn with_hover_repaint(mut self, repaint: bool) -> Self {
        self.hover_repaint = repaint;
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    /// Add a root-level node; returns its index.
    pub fn add_root(&mut self, label: impl Into<String>, icon: NodeIcon) -> usize {
        let order = self.nodes.iter().filter(|n| n.parent.is_none()).count() as u32;
        let idx = self.nodes.len();
        self.nodes.push(TreeNode::new(label, icon, None, order));
        idx
    }

    /// Add a child of `parent_idx`; returns its index.
    pub fn add_child(
        &mut self,
        parent_idx: usize,
        label: impl Into<String>,
        icon: NodeIcon,
    ) -> usize {
        let order = self
            .nodes
            .iter()
            .filter(|n| n.parent == Some(parent_idx))
            .count() as u32;
        let idx = self.nodes.len();
        self.nodes
            .push(TreeNode::new(label, icon, Some(parent_idx), order));
        idx
    }

    /// Expand the node at `idx`.
    pub fn expand(&mut self, idx: usize) {
        if idx < self.nodes.len() {
            self.nodes[idx].is_expanded = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

impl TreeView {
    fn scrollbar_x(&self) -> f64 {
        self.bounds.width - SCROLLBAR_W
    }

    fn max_scroll(&self) -> f64 {
        (self.content_height - self.bounds.height).max(0.0)
    }

    fn thumb_metrics(&self) -> Option<(f64, f64)> {
        let h = self.bounds.height;
        if self.content_height <= h {
            return None;
        }
        let ratio = h / self.content_height;
        let thumb_h = (h * ratio).max(20.0);
        let track_h = h - thumb_h;
        let thumb_y = track_h * (1.0 - self.scroll_offset / self.max_scroll());
        Some((thumb_y, thumb_h))
    }

    /// Is `local_pos` in the scrollbar strip?
    fn in_scrollbar(&self, local_pos: Point) -> bool {
        local_pos.x >= self.scrollbar_x()
    }

    /// Returns the flat-row index (into `row_metas`/`row_widgets`) for the row
    /// under `pos` in TreeView-local coordinates, or `None`.
    fn row_index_at(&self, pos: Point) -> Option<usize> {
        for (i, widget) in self.row_widgets.iter().enumerate() {
            let b = widget.bounds();
            // Clamp to visible content area — rows scrolled off-screen have b.y < 0
            // or b.y + b.height > self.bounds.height; exclude those slivers.
            if pos.y >= b.y.max(0.0)
                && pos.y < (b.y + b.height).min(self.bounds.height)
                && pos.x >= 0.0
                && pos.x < self.bounds.width - SCROLLBAR_W
            {
                return Some(i);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Selection helpers
// ---------------------------------------------------------------------------

impl TreeView {
    fn select_single(&mut self, node_idx: usize) {
        for n in &mut self.nodes {
            n.is_selected = false;
        }
        self.nodes[node_idx].is_selected = true;
        self.cursor_node = Some(node_idx);
    }

    fn toggle_select(&mut self, node_idx: usize) {
        self.nodes[node_idx].is_selected = !self.nodes[node_idx].is_selected;
        self.cursor_node = Some(node_idx);
    }

    fn range_select(&mut self, anchor_node: usize, target_node: usize, rows: &[FlatRow]) {
        let a = rows.iter().position(|r| r.node_idx == anchor_node);
        let b = rows.iter().position(|r| r.node_idx == target_node);
        if let (Some(a), Some(b)) = (a, b) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            for n in &mut self.nodes {
                n.is_selected = false;
            }
            for r in &rows[lo..=hi] {
                self.nodes[r.node_idx].is_selected = true;
            }
        }
        self.cursor_node = Some(target_node);
    }

    fn move_cursor(&mut self, delta: i32, rows: &[FlatRow]) {
        if rows.is_empty() {
            return;
        }
        let cur_flat = self
            .cursor_node
            .and_then(|ni| rows.iter().position(|r| r.node_idx == ni))
            .unwrap_or(0);
        let new_flat = (cur_flat as i32 + delta).clamp(0, rows.len() as i32 - 1) as usize;
        let ni = rows[new_flat].node_idx;
        self.select_single(ni);
        // Scroll to keep the new row visible.
        self.scroll_to_row(new_flat);
    }

    /// Returns the node index currently under the cursor, or `None`.
    pub fn hovered_node_idx(&self) -> Option<usize> {
        self.hovered_row
            .and_then(|ri| self.row_metas.get(ri).map(|m| m.node_idx))
    }

    fn scroll_to_row(&mut self, flat_idx: usize) {
        // `row_widgets` bounds reflect the `scroll_offset` from the last `layout()` call.
        // The framework calls `layout()` every frame before rendering, so `scroll_offset`
        // changes here will be reflected before the next mouse hit-test.
        // Y-up coordinates: y_bottom is the lower edge (smaller Y) and y_top is the upper edge (larger Y).
        let y_bottom =
            self.bounds.height - (flat_idx as f64 + 1.0) * self.row_height + self.scroll_offset;
        let y_top = y_bottom + self.row_height;
        if y_bottom < 0.0 {
            self.scroll_offset = (self.scroll_offset - y_bottom).min(self.max_scroll());
        } else if y_top > self.bounds.height {
            self.scroll_offset = (self.scroll_offset - (y_top - self.bounds.height)).max(0.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Widget impl
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Mouse event handlers
// ---------------------------------------------------------------------------

impl TreeView {
    fn handle_mouse_move(&mut self, pos: Point) -> EventResult {
        let old_hovered_scrollbar = self.hovered_scrollbar;
        let old_hovered_row = self.hovered_row;
        self.hovered_scrollbar = self.in_scrollbar(pos);

        if self.dragging_scrollbar {
            if let Some((_, thumb_h)) = self.thumb_metrics() {
                let h = self.bounds.height;
                let track_h = (h - thumb_h).max(1.0);
                let delta_y = self.sb_drag_start_y - pos.y;
                let spp = self.max_scroll() / track_h;
                self.scroll_offset =
                    (self.sb_drag_start_offset + delta_y * spp).clamp(0.0, self.max_scroll());
            }
            return EventResult::Consumed;
        }

        if let Some(drag) = &mut self.drag {
            let dx = pos.x - drag.current_pos.x;
            let dy = pos.y - drag.current_pos.y;
            drag.current_pos = pos;
            if !drag.live && (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD {
                drag.live = true;
            }
            if drag.live {
                let node_idx = drag.node_idx;
                let rows = flatten_visible(&self.nodes);
                self.drop_target = compute_drop_target(
                    pos,
                    &rows,
                    &self.nodes,
                    self.bounds.height,
                    self.row_height,
                    self.scroll_offset,
                    self.drag.as_ref().unwrap(),
                );
                let _ = node_idx;
            }
            return EventResult::Consumed;
        }

        self.hovered_row = self.row_index_at(pos);
        if self.hover_repaint
            && (self.hovered_scrollbar != old_hovered_scrollbar
                || self.hovered_row != old_hovered_row)
        {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn handle_mouse_down(&mut self, pos: Point, mods: Modifiers) -> EventResult {
        if self.in_scrollbar(pos) {
            self.dragging_scrollbar = true;
            self.sb_drag_start_y = pos.y;
            self.sb_drag_start_offset = self.scroll_offset;
            return EventResult::Consumed;
        }

        let Some(flat_i) = self.row_index_at(pos) else {
            return EventResult::Ignored;
        };
        let meta = &self.row_metas[flat_i];
        let node_idx = meta.node_idx;

        // Expand/collapse: any click on a row with children toggles it when
        // `toggle_on_row_click` is enabled (file-explorer style).  Otherwise
        // only the expand-toggle arrow triggers expansion so that clicking a
        // row in the inspector tree selects it without accidentally collapsing
        // a branch the user was browsing.
        if self.toggle_on_row_click {
            if meta.toggle_rect.is_some() {
                self.nodes[node_idx].is_expanded = !self.nodes[node_idx].is_expanded;
            }
        } else if let Some(tr) = meta.toggle_rect {
            if pos.x >= tr.x && pos.x < tr.x + tr.width && pos.y >= tr.y && pos.y < tr.y + tr.height
            {
                self.nodes[node_idx].is_expanded = !self.nodes[node_idx].is_expanded;
            }
        }

        // Selection
        if mods.ctrl {
            self.toggle_select(node_idx);
        } else if mods.shift {
            if let Some(a) = self.cursor_node {
                let rows2 = flatten_visible(&self.nodes);
                self.range_select(a, node_idx, &rows2);
            } else {
                self.select_single(node_idx);
            }
        } else {
            self.select_single(node_idx);
            if self.drag_enabled {
                let y_bot = self.row_widgets[flat_i].bounds().y;
                self.drag = Some(DragState {
                    node_idx,
                    _cursor_row_offset: pos.y - y_bot,
                    current_pos: pos,
                    live: false,
                });
            }
        }

        EventResult::Consumed
    }

    fn handle_mouse_up(&mut self, pos: Point) -> EventResult {
        // Scrollbar drag end
        if self.dragging_scrollbar {
            self.dragging_scrollbar = false;
            return EventResult::Consumed;
        }

        // Node drag end
        if let Some(drag) = self.drag.take() {
            if drag.live {
                if let Some(target) = self.drop_target.take() {
                    apply_drop(&mut self.nodes, drag.node_idx, target);
                }
            } else {
                // Was a click, not a drag — finalize single-select.
                self.select_single(drag.node_idx);
            }
            self.drop_target = None;
            return EventResult::Consumed;
        }

        let _ = pos;
        EventResult::Ignored
    }

    fn handle_key_down(&mut self, key: &Key, mods: Modifiers) -> EventResult {
        let rows = flatten_visible(&self.nodes);
        match key {
            Key::ArrowDown => {
                self.move_cursor(1, &rows);
                EventResult::Consumed
            }
            Key::ArrowUp => {
                self.move_cursor(-1, &rows);
                EventResult::Consumed
            }
            Key::ArrowRight => {
                if let Some(ni) = self.cursor_node {
                    if !self.nodes[ni].is_expanded
                        && rows.iter().any(|r| r.node_idx == ni && r.has_children)
                    {
                        self.nodes[ni].is_expanded = true;
                    } else {
                        // Move to first child
                        if rows.iter().any(|r| r.node_idx == ni) {
                            self.move_cursor(1, &rows);
                        }
                    }
                }
                EventResult::Consumed
            }
            Key::ArrowLeft => {
                if let Some(ni) = self.cursor_node {
                    if self.nodes[ni].is_expanded {
                        self.nodes[ni].is_expanded = false;
                    } else if let Some(parent_idx) = self.nodes[ni].parent {
                        self.select_single(parent_idx);
                        if let Some(fi) = rows.iter().position(|r| r.node_idx == parent_idx) {
                            self.scroll_to_row(fi);
                        }
                    }
                }
                EventResult::Consumed
            }
            Key::Char(' ') | Key::Enter => {
                if let Some(ni) = self.cursor_node {
                    if rows.iter().any(|r| r.node_idx == ni && r.has_children) {
                        self.nodes[ni].is_expanded = !self.nodes[ni].is_expanded;
                    }
                }
                EventResult::Consumed
            }
            Key::Tab => EventResult::Ignored, // let App handle focus advancement
            _ => {
                let _ = mods;
                EventResult::Ignored
            }
        }
    }
}
