//! `TreeView` — compositional tree widget with expand/collapse, multi-select,
//! keyboard navigation, and drag-and-drop reordering.
//!
//! Layout is virtualised: only the rows inside the viewport get a `TreeRow`
//! child widget (`row_widgets`), and a row widget is kept across layouts as
//! long as what it shows is unchanged, so its labels keep their cached
//! rasters.  The list of visible rows comes from `flat::FlatCache`, rebuilt
//! only when the structure or expansion changes.  Selection, hover and
//! focus are painted by `TreeView::paint` underneath the rows, so changing
//! them never rebuilds a row widget.
//!
//! The framework recurses into `row_widgets` after `paint()` returns, so the
//! `clip_rect` set at the end of `paint()` is active during child painting.
//! The rows are display-only: `TreeView` claims the pointer for its whole
//! area (`claims_pointer_exclusively`), so a click on a row focuses the tree
//! and selects the row.
//!
//! Module map: `node.rs` (data types), `flat.rs` (row cache), `row.rs`
//! (row widgets), `input.rs` (mouse / keyboard handling), `api.rs` (public
//! programmatic API and change events), `drag.rs` (drag and drop),
//! `widget_impl.rs` (the `Widget` impl: layout, paint, dispatch).

mod api;
mod drag;
mod flat;
mod input;
mod node;
pub mod row;
mod row_trailing;
mod widget_impl;

pub use api::{ScrollAlign, TreeViewEvent};
use flat::FlatCache;
use node::{DragState, DropPosition, FlatRow};
pub use node::{NodeGlyph, NodeIcon, TreeNode};
pub use row::{ExpandToggle, NodeIconWidget, TreeRow};

use std::sync::Arc;

use crate::geometry::{Point, Rect, Size};
use crate::icon_image::IconImage;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

/// Node ids of the visible rows by the uncached reference walk, for tests
/// of the cached rows.
#[cfg(test)]
pub(crate) fn reference_rows(nodes: &[TreeNode]) -> Vec<usize> {
    node::flatten_visible(nodes)
        .into_iter()
        .map(|r| r.node_idx)
        .collect()
}

const SCROLLBAR_W: f64 = 10.0;
const DRAG_THRESHOLD: f64 = 4.0;

// ---------------------------------------------------------------------------
// RowMeta
// ---------------------------------------------------------------------------

/// Metadata for one built row widget; parallel to `row_widgets`.
struct RowMeta {
    /// Index into `self.nodes` for this row.
    node_idx: usize,
    /// Hash of everything the row widget shows (see
    /// `widget_impl::row_signature`); a widget is reused while it matches.
    sig: u64,
}

/// Callback registered with [`TreeView::on_tree_event`].
type EventCallback = Box<dyn FnMut(&TreeViewEvent)>;

/// A scroll request resolved at the next `layout()`, once the visible rows
/// and the viewport height are current.
#[derive(Clone, Copy, Debug)]
enum PendingScroll {
    Node(usize, ScrollAlign),
    Row(usize, ScrollAlign),
}

// ---------------------------------------------------------------------------
// TreeView struct
// ---------------------------------------------------------------------------

pub struct TreeView {
    bounds: Rect,
    /// One `TreeRow` per visible row inside the viewport.
    row_widgets: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    /// Parallel to `row_widgets`.
    row_metas: Vec<RowMeta>,

    pub nodes: Vec<TreeNode>,

    // Scroll state
    scroll_offset: f64,
    content_height: f64,
    /// Viewport height of the last `layout()`; row positions derive from it.
    viewport_h: f64,

    // Row metrics
    pub row_height: f64,
    pub indent_width: f64,
    pub font: Arc<Font>,
    pub font_size: f64,
    /// Font for [`NodeGlyph`] icons; `None` uses `font`.
    pub icon_font: Option<Arc<Font>>,

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
    /// When `true` (the default), Enter toggles the cursor row's expansion as
    /// well as reporting [`TreeViewEvent::Activated`].  Set `false` when
    /// activation means something else (open, zoom).
    pub enter_toggles_expansion: bool,
    hover_repaint: bool,
    focused: bool,
    /// Display-row index of the row under the cursor.
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

    /// Cached child index and visible rows.
    flat: FlatCache,
    pending_scroll: Option<PendingScroll>,
    /// Changes the user made, for [`TreeView::take_events`].
    events: Vec<TreeViewEvent>,
    event_cb: Option<EventCallback>,
    /// Children counted per parent (`[0]` = roots, `[p + 1]` = children of
    /// `p`) so `add_root` / `add_child` are O(1); valid while
    /// `nodes.len() == counted_len`.
    sibling_counts: Vec<u32>,
    counted_len: usize,
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
            viewport_h: 0.0,
            row_height: 24.0,
            indent_width: 16.0,
            font,
            font_size: crate::font_settings::default_font_size_or(13.0),
            icon_font: None,
            drag_enabled: false,
            toggle_on_row_click: false,
            enter_toggles_expansion: true,
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
            flat: FlatCache::default(),
            pending_scroll: None,
            events: Vec::new(),
            event_cb: None,
            sibling_counts: Vec::new(),
            counted_len: usize::MAX,
        }
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
    /// Font for [`NodeGlyph`] icons (e.g. a Font Awesome face).
    pub fn with_icon_font(mut self, font: Arc<Font>) -> Self {
        self.icon_font = Some(font);
        self
    }
    /// See [`TreeView::enter_toggles_expansion`].
    pub fn with_enter_toggles_expansion(mut self, toggles: bool) -> Self {
        self.enter_toggles_expansion = toggles;
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
        let order = self.next_sibling_order(None);
        let idx = self.nodes.len();
        self.nodes.push(TreeNode::new(label, icon, None, order));
        self.counted_len = self.nodes.len();
        idx
    }

    /// Add a child of `parent_idx`; returns its index.
    pub fn add_child(
        &mut self,
        parent_idx: usize,
        label: impl Into<String>,
        icon: NodeIcon,
    ) -> usize {
        let order = self.next_sibling_order(Some(parent_idx));
        let idx = self.nodes.len();
        self.nodes
            .push(TreeNode::new(label, icon, Some(parent_idx), order));
        self.counted_len = self.nodes.len();
        idx
    }

    /// The `order` for a new last child of `parent` — the number of children
    /// it has.  Recounts (O(n)) only when `nodes` changed length behind the
    /// count's back (a direct `nodes` edit); O(1) for runs of `add_*` calls.
    fn next_sibling_order(&mut self, parent: Option<usize>) -> u32 {
        let len = self.nodes.len();
        let slot = match parent {
            None => 0,
            Some(p) if p < len => p + 1,
            // A parent that doesn't exist (yet): count the slow way.
            Some(p) => return self.nodes.iter().filter(|n| n.parent == Some(p)).count() as u32,
        };
        if self.counted_len != len {
            self.sibling_counts.clear();
            self.sibling_counts.resize(len + 1, 0);
            for n in &self.nodes {
                match n.parent {
                    None => self.sibling_counts[0] += 1,
                    Some(p) if p < len => self.sibling_counts[p + 1] += 1,
                    Some(_) => {}
                }
            }
        }
        if self.sibling_counts.len() < len + 2 {
            self.sibling_counts.resize(len + 2, 0);
        }
        let order = self.sibling_counts[slot];
        self.sibling_counts[slot] += 1;
        order
    }

    /// Show `image` instead of the procedural icon for the node at `idx`
    /// (`None` restores the procedural icon).  Out-of-range indices are
    /// ignored.
    pub fn set_node_icon_image(&mut self, idx: usize, image: Option<IconImage>) {
        if let Some(node) = self.nodes.get_mut(idx) {
            node.icon_image = image;
        }
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

    /// Bring the visible-row cache up to date with `nodes`.
    fn refresh_flat(&mut self) {
        self.flat.refresh(&self.nodes);
    }

    /// Position of the node being dragged (live) in `flat.rows` — that row
    /// is hidden while it follows the cursor, so display rows after it shift
    /// up by one.
    fn drag_skip(&self) -> Option<usize> {
        let d = self.drag.as_ref().filter(|d| d.live)?;
        self.flat.position_of(d.node_idx)
    }

    /// Number of display rows (visible rows minus a live-dragged one).
    fn display_len(&self) -> usize {
        self.flat.rows.len() - usize::from(self.drag_skip().is_some())
    }

    /// The display row at `i` (see [`Self::drag_skip`]).
    fn display_row(&self, i: usize) -> Option<FlatRow> {
        let skip = self.drag_skip();
        let flat_i = match skip {
            Some(s) if i >= s => i + 1,
            _ => i,
        };
        self.flat.rows.get(flat_i).copied()
    }

    /// Bottom edge (Y-up, TreeView-local) of display row `i`.
    fn row_y(&self, i: usize) -> f64 {
        self.viewport_h - (i as f64 + 1.0) * self.row_height + self.scroll_offset
    }

    /// The display-row index under `pos` (TreeView-local), or `None`.
    fn row_index_at(&self, pos: Point) -> Option<usize> {
        // Rows scrolled partly off-screen only count where they are visible.
        if pos.x < 0.0
            || pos.x >= self.bounds.width - SCROLLBAR_W
            || pos.y < 0.0
            || pos.y >= self.bounds.height.min(self.viewport_h)
            || self.row_height <= 0.0
        {
            return None;
        }
        let raw = (self.viewport_h - pos.y + self.scroll_offset) / self.row_height;
        if raw < 0.0 {
            return None;
        }
        let i = raw.floor() as usize;
        (i < self.display_len()).then_some(i)
    }
}

// ---------------------------------------------------------------------------
// Selection helpers
// ---------------------------------------------------------------------------

impl TreeView {
    /// Select only `node_idx` and move the cursor to it.  Returns whether
    /// any node's selection changed.
    fn set_single_selection(&mut self, node_idx: usize) -> bool {
        let mut changed = false;
        for (i, n) in self.nodes.iter_mut().enumerate() {
            let sel = i == node_idx;
            changed |= n.is_selected != sel;
            n.is_selected = sel;
        }
        self.cursor_node = Some(node_idx);
        changed
    }

    fn toggle_select(&mut self, node_idx: usize) {
        self.nodes[node_idx].is_selected = !self.nodes[node_idx].is_selected;
        self.cursor_node = Some(node_idx);
    }

    /// Select the visible rows from `anchor_node` to `target_node`.  Returns
    /// whether any node's selection changed.
    fn range_select(&mut self, anchor_node: usize, target_node: usize) -> bool {
        let a = self.flat.position_of(anchor_node);
        let b = self.flat.position_of(target_node);
        let mut changed = false;
        if let (Some(a), Some(b)) = (a, b) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let mut want = vec![false; self.nodes.len()];
            for r in &self.flat.rows[lo..=hi] {
                want[r.node_idx] = true;
            }
            for (n, sel) in self.nodes.iter_mut().zip(want) {
                changed |= n.is_selected != sel;
                n.is_selected = sel;
            }
        }
        self.cursor_node = Some(target_node);
        changed
    }

    /// Move the cursor `delta` visible rows, selecting that row.  Returns
    /// whether the selection changed.
    fn move_cursor(&mut self, delta: i32) -> bool {
        let rows = &self.flat.rows;
        if rows.is_empty() {
            return false;
        }
        let cur_flat = self
            .cursor_node
            .and_then(|ni| self.flat.position_of(ni))
            .unwrap_or(0);
        let new_flat = (cur_flat as i32 + delta).clamp(0, rows.len() as i32 - 1) as usize;
        let ni = rows[new_flat].node_idx;
        let changed = self.set_single_selection(ni);
        // Scroll to keep the new row visible.
        self.reveal_row_now(new_flat);
        changed
    }

    /// Returns the node index currently under the cursor, or `None`.
    pub fn hovered_node_idx(&self) -> Option<usize> {
        self.hovered_row
            .and_then(|ri| self.display_row(ri))
            .map(|r| r.node_idx)
    }

    /// Clear the hover state — useful when the mouse leaves the area the
    /// parent considers part of the tree (e.g. into the InspectorPanel's
    /// header or property pane).  Bumps the invalidation epoch so the
    /// previously-hovered row's background re-rasterises.
    pub fn clear_hover(&mut self) {
        if self.hovered_row.is_some() || self.hovered_scrollbar {
            self.hovered_row = None;
            self.hovered_scrollbar = false;
            crate::animation::request_draw();
        }
    }

    /// Scroll the least amount that shows visible row `flat_idx`, using the
    /// current viewport (event handlers run after a layout).
    fn reveal_row_now(&mut self, flat_idx: usize) {
        self.scroll_offset = api::scroll_for_row(
            flat_idx,
            ScrollAlign::Minimal,
            self.scroll_offset,
            self.viewport_h,
            self.row_height,
            self.content_height,
        );
    }
}
