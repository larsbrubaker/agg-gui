//! `TreeView` — virtualized tree widget with expand/collapse, multi-select,
//! keyboard navigation, and drag-and-drop reordering.
//!
//! The widget is **monolithic**: it draws all visible rows itself without
//! any child widgets, enabling O(visible_rows) painting regardless of total
//! tree size.

mod node;
mod drag;

pub use node::{NodeIcon, TreeNode};
use node::{DragState, DropPosition, FlatRow, flatten_visible};
use drag::{apply_drop, compute_drop_target, icon_color, paint_drop_child_highlight,
           paint_drop_line, paint_ghost};

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;

const SCROLLBAR_W: f64 = 10.0;
const EXPAND_W: f64 = 18.0;  // space reserved for expand arrow
const ICON_W: f64 = 14.0;
const ICON_GAP: f64 = 4.0;
const DRAG_THRESHOLD: f64 = 4.0;

// ---------------------------------------------------------------------------
// TreeView struct
// ---------------------------------------------------------------------------

pub struct TreeView {
    bounds: Rect,
    // Always empty — TreeView is a monolithic (leaf) widget.
    _children: Vec<Box<dyn Widget>>,

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
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl TreeView {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            _children: Vec::new(),
            nodes: Vec::new(),
            scroll_offset: 0.0,
            content_height: 0.0,
            row_height: 24.0,
            indent_width: 16.0,
            font,
            font_size: 13.0,
            focused: false,
            hovered_row: None,
            cursor_node: None,
            drag: None,
            drop_target: None,
            hovered_scrollbar: false,
            dragging_scrollbar: false,
            sb_drag_start_y: 0.0,
            sb_drag_start_offset: 0.0,
        }
    }

    pub fn with_row_height(mut self, h: f64) -> Self { self.row_height = h; self }
    pub fn with_indent_width(mut self, w: f64) -> Self { self.indent_width = w; self }
    pub fn with_font_size(mut self, s: f64) -> Self { self.font_size = s; self }

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
        let order = self.nodes
            .iter()
            .filter(|n| n.parent == Some(parent_idx))
            .count() as u32;
        let idx = self.nodes.len();
        self.nodes.push(TreeNode::new(label, icon, Some(parent_idx), order));
        idx
    }

    /// Expand the node at `idx`.
    pub fn expand(&mut self, idx: usize) {
        if idx < self.nodes.len() { self.nodes[idx].is_expanded = true; }
    }
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

impl TreeView {
    /// Y coordinate of the bottom edge of flat row `i` in local (Y-up) space.
    fn row_y_bottom(&self, i: usize) -> f64 {
        self.bounds.height - (i as f64 + 1.0) * self.row_height + self.scroll_offset
    }

    /// Flat-row index under `local_y`, or `None`.
    fn row_at_y(&self, local_y: f64, n_rows: usize) -> Option<usize> {
        if n_rows == 0 { return None; }
        let raw = (self.bounds.height - local_y + self.scroll_offset) / self.row_height;
        if raw < 0.0 { return None; }
        let i = raw as usize;
        if i >= n_rows { None } else { Some(i) }
    }

    /// First and last flat-row indices that overlap the viewport.
    fn visible_range(&self, n_rows: usize) -> (usize, usize) {
        if n_rows == 0 { return (0, 0); }
        let h = self.bounds.height;
        let rh = self.row_height;
        let off = self.scroll_offset;
        let first = ((off / rh) as usize).min(n_rows - 1);
        let last = (((h + off) / rh) as usize + 1).min(n_rows - 1);
        (first, last)
    }

    fn scrollbar_x(&self) -> f64 { self.bounds.width - SCROLLBAR_W }

    fn max_scroll(&self) -> f64 {
        (self.content_height - self.bounds.height).max(0.0)
    }

    fn thumb_metrics(&self) -> Option<(f64, f64)> {
        let h = self.bounds.height;
        if self.content_height <= h { return None; }
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
}

// ---------------------------------------------------------------------------
// Selection helpers
// ---------------------------------------------------------------------------

impl TreeView {
    fn select_single(&mut self, node_idx: usize) {
        for n in &mut self.nodes { n.is_selected = false; }
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
            for n in &mut self.nodes { n.is_selected = false; }
            for r in &rows[lo..=hi] {
                self.nodes[r.node_idx].is_selected = true;
            }
        }
        self.cursor_node = Some(target_node);
    }

    fn move_cursor(&mut self, delta: i32, rows: &[FlatRow]) {
        if rows.is_empty() { return; }
        let cur_flat = self.cursor_node
            .and_then(|ni| rows.iter().position(|r| r.node_idx == ni))
            .unwrap_or(0);
        let new_flat = (cur_flat as i32 + delta)
            .clamp(0, rows.len() as i32 - 1) as usize;
        let ni = rows[new_flat].node_idx;
        self.select_single(ni);
        // Scroll to keep the new row visible.
        self.scroll_to_row(new_flat);
    }

    fn scroll_to_row(&mut self, flat_idx: usize) {
        let y_bottom = self.bounds.height
            - (flat_idx as f64 + 1.0) * self.row_height
            + self.scroll_offset;
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

impl Widget for TreeView {
    fn type_name(&self) -> &'static str { "TreeView" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self._children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self._children }
    fn is_focusable(&self) -> bool { true }

    fn hit_test(&self, local_pos: Point) -> bool {
        // Capture all events during drags even if cursor leaves bounds.
        if self.drag.is_some() || self.dragging_scrollbar { return true; }
        let b = self.bounds();
        local_pos.x >= 0.0 && local_pos.x <= b.width
            && local_pos.y >= 0.0 && local_pos.y <= b.height
    }

    fn layout(&mut self, available: Size) -> Size {
        let rows = flatten_visible(&self.nodes);
        self.content_height = rows.len() as f64 * self.row_height;
        self.scroll_offset = self.scroll_offset.clamp(0.0, self.max_scroll());
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        let w = self.bounds.width;
        let content_w = w - SCROLLBAR_W;
        let rh = self.row_height;
        let ind = self.indent_width;
        let font_size = self.font_size;

        // --- Background ---
        ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // --- Scrollbar (drawn before content clip) ---
        let sb_x = self.scrollbar_x();
        if self.content_height > h {
            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.04));
            ctx.begin_path();
            ctx.rect(sb_x, 0.0, SCROLLBAR_W, h);
            ctx.fill();
            if let Some((thumb_y, thumb_h)) = self.thumb_metrics() {
                let thumb_color = if self.dragging_scrollbar {
                    Color::rgba(0.0, 0.0, 0.0, 0.45)
                } else if self.hovered_scrollbar {
                    Color::rgba(0.0, 0.0, 0.0, 0.32)
                } else {
                    Color::rgba(0.0, 0.0, 0.0, 0.18)
                };
                ctx.set_fill_color(thumb_color);
                ctx.begin_path();
                ctx.rounded_rect(sb_x + 2.0, thumb_y, SCROLLBAR_W - 4.0, thumb_h, 3.0);
                ctx.fill();
            }
        }

        // --- Content clip ---
        ctx.clip_rect(0.0, 0.0, content_w, h);

        // --- Compute visible rows ---
        let rows = flatten_visible(&self.nodes);
        if rows.is_empty() { return; }
        let (first, last) = self.visible_range(rows.len());
        let scroll_off = self.scroll_offset;
        let hovered = self.hovered_row;
        let focused = self.focused;
        let drag_node = self.drag.as_ref().map(|d| d.node_idx);
        let drop_target = self.drop_target;

        // Collect data needed per row to avoid borrow issues.
        let font = Arc::clone(&self.font);
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(font_size);

        for i in first..=last {
            let row = &rows[i];
            let node = &self.nodes[row.node_idx];
            let y_bot = h - (i as f64 + 1.0) * rh + scroll_off;
            let y_top = y_bot + rh;
            let is_dragged = drag_node == Some(row.node_idx);

            if is_dragged { continue; } // draw ghost on top instead

            // Selection / hover background
            if node.is_selected {
                let c = if focused {
                    Color::rgba(0.22, 0.45, 0.88, 0.15)
                } else {
                    Color::rgba(0.0, 0.0, 0.0, 0.07)
                };
                ctx.set_fill_color(c);
                ctx.begin_path();
                ctx.rect(0.0, y_bot, content_w, rh);
                ctx.fill();
            } else if hovered == Some(i) {
                ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.04));
                ctx.begin_path();
                ctx.rect(0.0, y_bot, content_w, rh);
                ctx.fill();
            }

            // Expand arrow
            let ax = row.depth as f64 * ind + 2.0;
            if row.has_children {
                let cx = ax + 7.0;
                let cy = y_bot + rh * 0.5;
                ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.45));
                ctx.begin_path();
                if node.is_expanded {
                    // Down-pointing ▼
                    ctx.move_to(cx - 4.5, cy + 2.0);
                    ctx.line_to(cx + 4.5, cy + 2.0);
                    ctx.line_to(cx, cy - 3.0);
                    ctx.close_path();
                } else {
                    // Right-pointing ▶
                    ctx.move_to(cx - 2.5, cy - 4.5);
                    ctx.line_to(cx - 2.5, cy + 4.5);
                    ctx.line_to(cx + 3.5, cy);
                    ctx.close_path();
                }
                ctx.fill();
            }

            // Icon
            let ix = ax + EXPAND_W;
            let iy = y_bot + (rh - ICON_W) * 0.5;
            ctx.set_fill_color(icon_color(node.icon));
            ctx.begin_path();
            ctx.rounded_rect(ix, iy, ICON_W, ICON_W, 2.0);
            ctx.fill();
            // folder tab nub
            if matches!(node.icon, NodeIcon::Folder) {
                ctx.begin_path();
                ctx.rounded_rect(ix, iy + ICON_W * 0.55, ICON_W * 0.45, ICON_W * 0.5, 1.0);
                ctx.fill();
            }

            // Label
            let lx = ix + ICON_W + ICON_GAP;
            let label = &node.label;
            if let Some(m) = ctx.measure_text(label) {
                let ty = y_bot + (rh - m.ascent - m.descent) * 0.5 + m.descent;
                ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.87));
                ctx.fill_text(label, lx, ty);
            }

            let _ = y_top;
        }

        // --- Drop indicator ---
        if let Some(dt) = drop_target {
            if self.drag.as_ref().map_or(false, |d| d.live) {
                let ref_node = match dt {
                    DropPosition::Before(ni) | DropPosition::After(ni) | DropPosition::AsChild(ni) => ni,
                };
                if let Some(ri) = rows.iter().position(|r| r.node_idx == ref_node) {
                    let y_bot = h - (ri as f64 + 1.0) * rh + scroll_off;
                    let indent = rows[ri].depth as f64 * ind + EXPAND_W;
                    match dt {
                        DropPosition::Before(_) => paint_drop_line(ctx, indent, y_bot + rh, content_w - indent),
                        DropPosition::After(_)  => paint_drop_line(ctx, indent, y_bot, content_w - indent),
                        DropPosition::AsChild(_) => paint_drop_child_highlight(ctx, y_bot, content_w, rh),
                    }
                }
            }
        }

        // --- Ghost ---
        if let Some(drag) = &self.drag {
            if drag.live {
                let label = self.nodes[drag.node_idx].label.clone();
                let ic = icon_color(self.nodes[drag.node_idx].icon);
                let pos = drag.current_pos;
                paint_ghost(ctx, &label, pos, content_w, rh, &font, font_size, ic);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::FocusGained => { self.focused = true;  EventResult::Consumed }
            Event::FocusLost   => { self.focused = false; EventResult::Consumed }

            Event::MouseWheel { delta_y, .. } => {
                // Convention: delta_y > 0 = user scrolled DOWN (wants to see content below).
                // Increasing scroll_offset shifts content UP → reveals lower rows. ✓
                self.scroll_offset =
                    (self.scroll_offset + delta_y * 40.0).clamp(0.0, self.max_scroll());
                EventResult::Consumed
            }

            Event::MouseMove { pos } => self.handle_mouse_move(*pos),
            Event::MouseDown { pos, button: MouseButton::Left, modifiers } => {
                self.handle_mouse_down(*pos, *modifiers)
            }
            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                self.handle_mouse_up(*pos)
            }
            Event::KeyDown { key, modifiers } => self.handle_key_down(key, *modifiers),
            _ => EventResult::Ignored,
        }
    }
}

// ---------------------------------------------------------------------------
// Mouse event handlers
// ---------------------------------------------------------------------------

impl TreeView {
    fn handle_mouse_move(&mut self, pos: Point) -> EventResult {
        self.hovered_scrollbar = self.in_scrollbar(pos);

        // Scrollbar drag
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

        // Node drag
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
                    pos, &rows, &self.nodes,
                    self.bounds.height, self.row_height,
                    self.scroll_offset, self.drag.as_ref().unwrap(),
                );
                let _ = node_idx;
            }
            return EventResult::Consumed;
        }

        // Update hover row
        let rows = flatten_visible(&self.nodes);
        self.hovered_row = self.row_at_y(pos.y, rows.len());
        EventResult::Ignored
    }

    fn handle_mouse_down(&mut self, pos: Point, mods: Modifiers) -> EventResult {
        // Scrollbar
        if self.in_scrollbar(pos) {
            self.dragging_scrollbar = true;
            self.sb_drag_start_y = pos.y;
            self.sb_drag_start_offset = self.scroll_offset;
            return EventResult::Consumed;
        }

        let rows = flatten_visible(&self.nodes);
        let Some(flat_i) = self.row_at_y(pos.y, rows.len()) else {
            return EventResult::Ignored;
        };
        let row = &rows[flat_i];
        let node_idx = row.node_idx;

        // Click on expand arrow?
        let arrow_x = row.depth as f64 * self.indent_width;
        if pos.x >= arrow_x && pos.x < arrow_x + EXPAND_W && row.has_children {
            self.nodes[node_idx].is_expanded = !self.nodes[node_idx].is_expanded;
            return EventResult::Consumed;
        }

        // Selection
        if mods.ctrl {
            self.toggle_select(node_idx);
        } else if mods.shift {
            let anchor = self.cursor_node;
            if let Some(a) = anchor {
                let rows2 = flatten_visible(&self.nodes);
                self.range_select(a, node_idx, &rows2);
            } else {
                self.select_single(node_idx);
            }
        } else {
            if !self.nodes[node_idx].is_selected {
                self.select_single(node_idx);
            }
            // Start drag potential
            let y_bot = self.row_y_bottom(flat_i);
            self.drag = Some(DragState {
                node_idx,
                _cursor_row_offset: pos.y - y_bot,
                current_pos: pos,
                live: false,
            });
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
            Key::ArrowDown => { self.move_cursor(1, &rows);  EventResult::Consumed }
            Key::ArrowUp   => { self.move_cursor(-1, &rows); EventResult::Consumed }
            Key::ArrowRight => {
                if let Some(ni) = self.cursor_node {
                    if !self.nodes[ni].is_expanded
                        && rows.iter().any(|r| r.node_idx == ni && r.has_children)
                    {
                        self.nodes[ni].is_expanded = true;
                    } else {
                        // Move to first child
                        if let Some(flat_i) = rows.iter().position(|r| r.node_idx == ni) {
                            self.move_cursor(1, &rows);
                            let _ = flat_i;
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
