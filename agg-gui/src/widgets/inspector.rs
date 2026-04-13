//! Widget inspector panel — composition-based, light theme.
//!
//! Each tree row is a real `InspectorRow` child widget, itself composed of
//! `Label` children. This makes the inspector's own structure visible when
//! it is inspected.
//!
//! Layout (Y-up, panel fills its full bounds):
//! ```text
//! ┌─────────────────────┐ ← top (HEADER_H)   header (painted in panel's paint)
//! ├─────────────────────┤
//! │   InspectorRow …    │ ← tree area (children clipped here)
//! ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤ ← draggable h-split
//! │   Properties        │ ← props area (painted in panel's paint)
//! └─────────────────────┘ ← bottom (y=0)
//! ```
//!
//! Clipping: `paint()` calls `ctx.clip_rect(tree_area)` as its last
//! operation. The framework then paints InspectorRow children inside that clip.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{InspectorNode, Widget};
use crate::widgets::label::{Label, LabelAlign};

// ── geometry constants ────────────────────────────────────────────────────────
const DEFAULT_PROPS_H: f64 = 180.0;
const ROW_H:           f64 = 20.0;
const INDENT_W:        f64 = 14.0;
const HEADER_H:        f64 = 30.0;
const FONT_SIZE:       f64 = 12.0;
const SPLIT_HIT:       f64 = 5.0;
const MIN_PROPS_H:     f64 = 60.0;
const MIN_TREE_H:      f64 = 60.0;
const SIZE_LABEL_W:    f64 = 72.0;

// ── light theme colors ────────────────────────────────────────────────────────
fn c_panel_bg()  -> Color { Color::rgb(0.965, 0.968, 0.975) }
fn c_header_bg() -> Color { Color::rgb(0.910, 0.915, 0.925) }
fn c_props_bg()  -> Color { Color::rgb(0.950, 0.952, 0.960) }
fn c_split_bg()  -> Color { Color::rgba(0.0, 0.0, 0.0, 0.08) }
fn c_border()    -> Color { Color::rgba(0.0, 0.0, 0.0, 0.12) }
fn c_text()      -> Color { Color::rgb(0.12, 0.12, 0.15) }
fn c_dim_text()  -> Color { Color::rgba(0.0, 0.0, 0.0, 0.42) }
fn c_guide()     -> Color { Color::rgba(0.0, 0.0, 0.0, 0.10) }
fn c_row_hover() -> Color { Color::rgba(0.10, 0.40, 0.90, 0.08) }
fn c_row_sel()   -> Color { Color::rgba(0.10, 0.40, 0.90, 0.14) }
fn c_row_alt()   -> Color { Color::rgba(0.0, 0.0, 0.0, 0.025) }
fn c_sel_text()  -> Color { Color::rgb(0.10, 0.35, 0.80) }

// ── tree helpers ──────────────────────────────────────────────────────────────

/// For each node index, whether the immediately following node is a child.
fn compute_has_subtree(nodes: &[InspectorNode]) -> Vec<bool> {
    let n = nodes.len();
    let mut out = vec![false; n];
    for i in 0..n.saturating_sub(1) {
        if nodes[i + 1].depth > nodes[i].depth {
            out[i] = true;
        }
    }
    out
}

/// Visible node indices (DFS order), skipping subtrees of collapsed nodes.
fn build_visible_map(
    nodes: &[InspectorNode],
    collapsed: &HashSet<usize>,
) -> Vec<usize> {
    let mut visible = Vec::new();
    let mut skip_depth: Option<usize> = None;
    for (i, node) in nodes.iter().enumerate() {
        if let Some(min_d) = skip_depth {
            if node.depth > min_d { continue; }
            skip_depth = None;
        }
        visible.push(i);
        if collapsed.contains(&i) {
            skip_depth = Some(node.depth);
        }
    }
    visible
}

// ── InspectorRow ──────────────────────────────────────────────────────────────

/// One row in the inspector tree.
///
/// Children (Label widgets) are rebuilt every `layout()` call:
///   0: expand/collapse toggle label ("▶" / "▼" / "")
///   1: type-name label
///   2: size label (right-aligned)
///
/// `paint()` draws only the row background and tree guide line; children
/// draw their own text.
struct InspectorRow {
    bounds:      Rect,
    children:    Vec<Box<dyn Widget>>,
    font:        Arc<Font>,
    depth:       usize,
    has_subtree: bool,
    is_expanded: bool,
    is_selected: bool,
    is_hovered:  bool,
    even_row:    bool,
    type_name:   &'static str,
    size_str:    String,
}

impl InspectorRow {
    #[allow(clippy::too_many_arguments)]
    fn new(
        font:        Arc<Font>,
        depth:       usize,
        has_subtree: bool,
        is_expanded: bool,
        is_selected: bool,
        is_hovered:  bool,
        even_row:    bool,
        type_name:   &'static str,
        size_str:    String,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            depth,
            has_subtree,
            is_expanded,
            is_selected,
            is_hovered,
            even_row,
            type_name,
            size_str,
        }
    }
}

impl Widget for InspectorRow {
    fn type_name(&self) -> &'static str { "InspectorRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w          = available.width;
        let text_color = if self.is_selected { c_sel_text() } else { c_text() };
        let indent_x   = self.depth as f64 * INDENT_W;

        // ── expand toggle (▶ / ▼ / empty) ────────────────────────────────────
        let expand_text = if self.has_subtree {
            if self.is_expanded { "▼" } else { "▶" }
        } else {
            ""
        };
        let mut expand_lbl = Label::new(expand_text, Arc::clone(&self.font))
            .with_font_size(8.0)
            .with_color(c_dim_text())
            .with_align(LabelAlign::Center);
        expand_lbl.layout(Size::new(INDENT_W, ROW_H));
        expand_lbl.set_bounds(Rect::new(indent_x, 0.0, INDENT_W, ROW_H));

        // ── type-name label ───────────────────────────────────────────────────
        let type_x = indent_x + INDENT_W + 2.0;
        let type_w  = (w - type_x - SIZE_LABEL_W - 4.0).max(10.0);
        let mut type_lbl = Label::new(self.type_name, Arc::clone(&self.font))
            .with_font_size(FONT_SIZE)
            .with_color(text_color);
        type_lbl.layout(Size::new(type_w, ROW_H));
        type_lbl.set_bounds(Rect::new(type_x, 0.0, type_w, ROW_H));

        // ── size label (right-aligned) ────────────────────────────────────────
        let size_x = (w - SIZE_LABEL_W - 4.0).max(0.0);
        let mut size_lbl = Label::new(self.size_str.clone(), Arc::clone(&self.font))
            .with_font_size(10.0)
            .with_color(c_dim_text())
            .with_align(LabelAlign::Right);
        size_lbl.layout(Size::new(SIZE_LABEL_W, ROW_H));
        size_lbl.set_bounds(Rect::new(size_x, 0.0, SIZE_LABEL_W, ROW_H));

        self.children.clear();
        self.children.push(Box::new(expand_lbl));
        self.children.push(Box::new(type_lbl));
        self.children.push(Box::new(size_lbl));

        Size::new(w, ROW_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;

        // ── row background ────────────────────────────────────────────────────
        let bg = if self.is_selected     { c_row_sel() }
                 else if self.is_hovered { c_row_hover() }
                 else if self.even_row   { c_row_alt() }
                 else                   { Color::transparent() };
        if bg.a > 0.0 {
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, ROW_H);
            ctx.fill();
        }

        // ── tree guide line (L-shape from parent level to text) ───────────────
        // Local coords: y=ROW_H is top of row, y=0 is bottom.
        if self.depth > 0 {
            let guide_x = (self.depth as f64 - 1.0) * INDENT_W + INDENT_W * 0.5;
            let type_x  = self.depth as f64 * INDENT_W + INDENT_W;
            let mid_y   = ROW_H * 0.5;
            ctx.set_stroke_color(c_guide());
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(guide_x, ROW_H);  // top edge — connects up to parent
            ctx.line_to(guide_x, mid_y);
            ctx.line_to(type_x,  mid_y);
            ctx.stroke();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── InspectorPanel ────────────────────────────────────────────────────────────

pub struct InspectorPanel {
    bounds:         Rect,
    /// InspectorRow widgets, rebuilt every layout() call.
    children:       Vec<Box<dyn Widget>>,
    font:           Arc<Font>,
    nodes:          Rc<RefCell<Vec<InspectorNode>>>,
    scroll_offset:  f64,
    /// Selected: original node index.
    selected:       Option<usize>,
    /// Hovered: visible row index.
    hovered_row:    Option<usize>,
    props_h:        f64,
    split_dragging: bool,
    /// Original node indices whose subtrees are collapsed.
    collapsed:      HashSet<usize>,
    /// visible_row_idx → original_node_idx (updated in layout).
    visible_map:    Vec<usize>,
    /// Written by layout(); read by the render loop to draw an overlay.
    pub hovered_bounds: Rc<RefCell<Option<Rect>>>,
}

impl InspectorPanel {
    pub fn new(
        font:           Arc<Font>,
        nodes:          Rc<RefCell<Vec<InspectorNode>>>,
        hovered_bounds: Rc<RefCell<Option<Rect>>>,
    ) -> Self {
        Self {
            bounds:         Rect::default(),
            children:       Vec::new(),
            font,
            nodes,
            scroll_offset:  0.0,
            selected:       None,
            hovered_row:    None,
            props_h:        DEFAULT_PROPS_H,
            split_dragging: false,
            collapsed:      HashSet::new(),
            visible_map:    Vec::new(),
            hovered_bounds,
        }
    }

    // ── geometry helpers ──────────────────────────────────────────────────────

    /// Height of the area below the header (tree + props).
    fn list_area_h(&self) -> f64 { (self.bounds.height - HEADER_H).max(0.0) }

    /// Y position of the tree/props split line (from panel bottom).
    fn split_y(&self) -> f64 {
        self.props_h.clamp(
            MIN_PROPS_H,
            (self.list_area_h() - MIN_TREE_H).max(MIN_PROPS_H),
        )
    }

    /// Bottom Y of the tree area (just above the split handle).
    fn tree_origin_y(&self) -> f64 { self.split_y() + 4.0 }

    /// Visible height of the tree area.
    fn tree_area_h(&self) -> f64 {
        (self.list_area_h() - self.tree_origin_y()).max(0.0)
    }

    fn max_scroll(&self) -> f64 {
        let n = self.visible_map.len() as f64;
        (n * ROW_H - self.tree_area_h()).max(0.0)
    }

    /// Convert a panel-local Y coordinate into a visible row index.
    fn vis_row_at(&self, pos: Point) -> Option<usize> {
        let list_top = self.list_area_h();
        let tree_bot = self.tree_origin_y();
        if pos.y < tree_bot || pos.y > list_top { return None; }
        let row = ((list_top + self.scroll_offset - pos.y) / ROW_H) as usize;
        if row < self.visible_map.len() { Some(row) } else { None }
    }

    fn on_split_handle(&self, pos: Point) -> bool {
        let sy = self.split_y();
        pos.y >= sy - SPLIT_HIT && pos.y <= sy + SPLIT_HIT
    }
}

// ── Widget impl ───────────────────────────────────────────────────────────────

impl Widget for InspectorPanel {
    fn type_name(&self) -> &'static str { "InspectorPanel" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds.width  = available.width;
        self.bounds.height = available.height;

        let w        = available.width;
        let nodes    = self.nodes.borrow();
        let has_sub  = compute_has_subtree(&nodes);
        let vis_map  = build_visible_map(&nodes, &self.collapsed);
        let n        = vis_map.len();
        let list_h   = self.list_area_h();

        // Clamp scroll to valid range.
        self.scroll_offset = self.scroll_offset
            .clamp(0.0, (n as f64 * ROW_H - self.tree_area_h()).max(0.0));

        // Update hovered_bounds for the overlay in the render loop.
        *self.hovered_bounds.borrow_mut() = self.hovered_row
            .and_then(|vi| vis_map.get(vi))
            .map(|&oi| nodes[oi].screen_bounds);

        // Rebuild children (one InspectorRow per visible node).
        // Row i bottom Y = list_h - (i+1)*ROW_H + scroll_offset  (Y-up, top-down list)
        self.children.clear();
        for (vis_idx, &orig_idx) in vis_map.iter().enumerate() {
            let node     = &nodes[orig_idx];
            let is_exp   = !self.collapsed.contains(&orig_idx);
            let b        = &node.screen_bounds;
            let size_str = format!("{:.0}×{:.0}", b.width, b.height);
            let row_y    = list_h - (vis_idx as f64 + 1.0) * ROW_H + self.scroll_offset;

            let mut row = InspectorRow::new(
                Arc::clone(&self.font),
                node.depth,
                has_sub[orig_idx],
                is_exp,
                self.selected == Some(orig_idx),
                self.hovered_row == Some(vis_idx),
                vis_idx % 2 == 0,
                node.type_name,
                size_str,
            );
            row.layout(Size::new(w, ROW_H));
            row.set_bounds(Rect::new(0.0, row_y, w, ROW_H));
            self.children.push(Box::new(row));
        }

        self.visible_map = vis_map;
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w     = self.bounds.width;
        let h     = self.bounds.height;
        let sy    = self.split_y();
        let hdr_y = h - HEADER_H;

        // ── panel background ────────────────────────────────────────────────
        ctx.set_fill_color(c_panel_bg());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Left border
        ctx.set_stroke_color(c_border());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, 0.0);
        ctx.line_to(0.0, h);
        ctx.stroke();

        // ── header ──────────────────────────────────────────────────────────
        ctx.set_fill_color(c_header_bg());
        ctx.begin_path();
        ctx.rect(0.0, hdr_y, w, HEADER_H);
        ctx.fill();

        ctx.set_stroke_color(c_border());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, hdr_y);
        ctx.line_to(w, hdr_y);
        ctx.stroke();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(13.0);
        ctx.set_fill_color(c_text());
        let title = "Widget Inspector";
        if let Some(m) = ctx.measure_text(title) {
            ctx.fill_text(
                title,
                12.0,
                hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent,
            );
        }

        let count_txt = format!("{} widgets", self.nodes.borrow().len());
        ctx.set_font_size(11.0);
        ctx.set_fill_color(c_dim_text());
        if let Some(m) = ctx.measure_text(&count_txt) {
            ctx.fill_text(
                &count_txt,
                w - m.width - 10.0,
                hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent,
            );
        }

        // ── properties pane ──────────────────────────────────────────────────
        ctx.set_fill_color(c_props_bg());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, sy - 2.0);
        ctx.fill();

        self.paint_properties(ctx, sy - 2.0);

        // ── split handle ─────────────────────────────────────────────────────
        ctx.set_fill_color(c_split_bg());
        ctx.begin_path();
        ctx.rect(0.0, sy - 2.0, w, 4.0);
        ctx.fill();
        ctx.set_stroke_color(c_border());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, sy);
        ctx.line_to(w, sy);
        ctx.stroke();

        // ── clip children to tree area ───────────────────────────────────────
        // This clip is installed as the LAST operation in paint(). The
        // framework then paints InspectorRow children inside this region,
        // preventing them from bleeding into the header or properties pane.
        let tree_h = self.tree_area_h();
        if tree_h > 0.0 {
            ctx.clip_rect(0.0, self.tree_origin_y(), w, tree_h);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if self.on_split_handle(*pos) {
                    self.split_dragging = true;
                    return EventResult::Consumed;
                }
                if let Some(vis_idx) = self.vis_row_at(*pos) {
                    let orig_idx = self.visible_map[vis_idx];
                    // Check for expand/collapse toggle click.
                    let (depth, node_has_sub) = {
                        let nodes = self.nodes.borrow();
                        let d = nodes.get(orig_idx).map_or(0, |n| n.depth);
                        let has = orig_idx + 1 < nodes.len()
                            && nodes[orig_idx + 1].depth > nodes[orig_idx].depth;
                        (d, has)
                    };
                    let toggle_start = depth as f64 * INDENT_W;
                    let toggle_end   = toggle_start + INDENT_W;
                    if node_has_sub && pos.x >= toggle_start && pos.x <= toggle_end {
                        if self.collapsed.contains(&orig_idx) {
                            self.collapsed.remove(&orig_idx);
                        } else {
                            self.collapsed.insert(orig_idx);
                        }
                    } else {
                        self.selected = Some(orig_idx);
                    }
                } else {
                    self.selected = None;
                }
                EventResult::Consumed
            }
            Event::MouseMove { pos } => {
                if self.split_dragging {
                    self.props_h = pos.y.clamp(
                        MIN_PROPS_H,
                        (self.list_area_h() - MIN_TREE_H).max(MIN_PROPS_H),
                    );
                    return EventResult::Consumed;
                }
                self.hovered_row = self.vis_row_at(*pos);
                EventResult::Ignored
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                if self.split_dragging {
                    self.split_dragging = false;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseWheel { delta_y, .. } => {
                // delta_y > 0 = scroll down = increase offset (content moves up).
                self.scroll_offset = (self.scroll_offset + delta_y * 30.0)
                    .clamp(0.0, self.max_scroll());
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── properties pane (monolithic) ─────────────────────────────────────────────

impl InspectorPanel {
    fn paint_properties(&self, ctx: &mut dyn DrawCtx, available_h: f64) {
        if available_h < 4.0 { return; }
        let w = self.bounds.width;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(10.0);
        ctx.set_fill_color(c_dim_text());
        ctx.fill_text("PROPERTIES", 10.0, available_h - 14.0);

        ctx.set_stroke_color(c_border());
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(10.0 + 70.0, available_h - 10.0);
        ctx.line_to(w - 8.0, available_h - 10.0);
        ctx.stroke();

        let Some(sel_idx) = self.selected else {
            ctx.set_font_size(FONT_SIZE);
            ctx.set_fill_color(c_dim_text());
            ctx.fill_text("(select a widget)", 10.0, available_h - 36.0);
            return;
        };

        let nodes = self.nodes.borrow();
        let Some(node) = nodes.get(sel_idx) else { return; };

        ctx.set_font_size(14.0);
        ctx.set_fill_color(c_text());
        ctx.fill_text(node.type_name, 10.0, available_h - 36.0);

        let b = &node.screen_bounds;
        let rows: &[(&str, String)] = &[
            ("x",      format!("{:.1}", b.x)),
            ("y",      format!("{:.1}", b.y)),
            ("width",  format!("{:.1}", b.width)),
            ("height", format!("{:.1}", b.height)),
            ("depth",  format!("{}", node.depth)),
        ];

        ctx.set_font_size(FONT_SIZE);
        let row_start_y = available_h - 56.0;
        for (i, (label, value)) in rows.iter().enumerate() {
            let ry = row_start_y - i as f64 * 18.0;
            if ry < 4.0 { break; }
            ctx.set_fill_color(c_dim_text());
            ctx.fill_text(label, 12.0, ry);
            ctx.set_fill_color(c_text());
            if let Some(m) = ctx.measure_text(value) {
                ctx.fill_text(value, w - m.width - 10.0, ry);
            }
            ctx.set_stroke_color(c_border());
            ctx.set_line_width(0.5);
            ctx.begin_path();
            ctx.move_to(8.0, ry - 4.0);
            ctx.line_to(w - 8.0, ry - 4.0);
            ctx.stroke();
        }

        // Box-model mini diagram
        let diag_h = (row_start_y - rows.len() as f64 * 18.0 - 12.0).min(80.0);
        if diag_h > 30.0 {
            let diag_y_top = diag_h - 4.0;
            let diag_w     = w - 20.0;
            let aspect     = if b.height > 0.0 { b.width / b.height } else { 1.0 };
            let box_h      = (diag_h * 0.6).min(50.0);
            let box_w      = (box_h * aspect).min(diag_w * 0.8);
            let box_x      = 10.0 + (diag_w - box_w) * 0.5;
            let box_y      = diag_y_top - (diag_h + box_h) * 0.5;

            ctx.set_fill_color(Color::rgba(0.10, 0.50, 1.0, 0.10));
            ctx.begin_path();
            ctx.rect(box_x, box_y, box_w, box_h);
            ctx.fill();
            ctx.set_stroke_color(Color::rgba(0.10, 0.50, 1.0, 0.50));
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rect(box_x, box_y, box_w, box_h);
            ctx.stroke();

            let dim = format!("{:.0} × {:.0}", b.width, b.height);
            ctx.set_font_size(10.0);
            ctx.set_fill_color(Color::rgba(0.10, 0.40, 0.90, 0.80));
            if let Some(m) = ctx.measure_text(&dim) {
                if m.width < box_w - 4.0 {
                    ctx.fill_text(
                        &dim,
                        box_x + (box_w - m.width) * 0.5,
                        box_y + (box_h - m.ascent - m.descent) * 0.5 + m.descent,
                    );
                }
            }
        }
    }
}
