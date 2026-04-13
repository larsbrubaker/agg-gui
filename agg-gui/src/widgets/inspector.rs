//! Widget inspector panel — uses the system `TreeView` for tree display.
//!
//! Layout (Y-up, panel fills its full bounds):
//! ```text
//! ┌─────────────────────┐ ← top (HEADER_H)   header (painted monolithically)
//! ├─────────────────────┤
//! │   TreeView          │ ← tree area (TreeView painted here)
//! ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤ ← draggable h-split
//! │   Properties        │ ← props area (painted monolithically)
//! └─────────────────────┘ ← bottom (y=0)
//! ```

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{InspectorNode, Widget};
use crate::widgets::tree_view::{NodeIcon, TreeNode, TreeView};

// ── geometry constants ────────────────────────────────────────────────────────
const DEFAULT_PROPS_H: f64 = 180.0;
const FONT_SIZE:       f64 = 12.0;
const HEADER_H:        f64 = 30.0;
const SPLIT_HIT:       f64 = 5.0;
const MIN_PROPS_H:     f64 = 60.0;
const MIN_TREE_H:      f64 = 60.0;

// ── light theme colors ────────────────────────────────────────────────────────
fn c_panel_bg()  -> Color { Color::rgb(0.965, 0.968, 0.975) }
fn c_header_bg() -> Color { Color::rgb(0.910, 0.915, 0.925) }
fn c_props_bg()  -> Color { Color::rgb(0.950, 0.952, 0.960) }
fn c_split_bg()  -> Color { Color::rgba(0.0, 0.0, 0.0, 0.08) }
fn c_border()    -> Color { Color::rgba(0.0, 0.0, 0.0, 0.12) }
fn c_text()      -> Color { Color::rgb(0.12, 0.12, 0.15) }
fn c_dim_text()  -> Color { Color::rgba(0.0, 0.0, 0.0, 0.42) }

// ── event translation helper ──────────────────────────────────────────────────

/// Translate the Y coordinate of a mouse event by subtracting `offset_y`.
/// X is unchanged. Non-mouse events pass through unchanged.
fn translate_event(event: &Event, offset_y: f64) -> Event {
    match event {
        Event::MouseDown { pos, button, modifiers } => Event::MouseDown {
            pos: Point::new(pos.x, pos.y - offset_y),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseMove { pos } => Event::MouseMove {
            pos: Point::new(pos.x, pos.y - offset_y),
        },
        Event::MouseUp { pos, button, modifiers } => Event::MouseUp {
            pos: Point::new(pos.x, pos.y - offset_y),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseWheel { pos, delta_y } => Event::MouseWheel {
            pos: Point::new(pos.x, pos.y - offset_y),
            delta_y: *delta_y,
        },
        other => other.clone(),
    }
}

// ── InspectorPanel ────────────────────────────────────────────────────────────

pub struct InspectorPanel {
    bounds:         Rect,
    /// Always empty — children are not exposed; TreeView is managed directly.
    _children:      Vec<Box<dyn Widget>>,
    font:           Arc<Font>,
    nodes:          Rc<RefCell<Vec<InspectorNode>>>,
    /// Selected: original node index; synced from TreeView selection.
    selected:       Option<usize>,
    props_h:        f64,
    split_dragging: bool,
    /// Written by layout(); read by the render loop to draw an overlay.
    pub hovered_bounds: Rc<RefCell<Option<Rect>>>,
    /// The tree widget, managed directly (not in children).
    pub(crate) tree_view: TreeView,
}

impl InspectorPanel {
    pub fn new(
        font:           Arc<Font>,
        nodes:          Rc<RefCell<Vec<InspectorNode>>>,
        hovered_bounds: Rc<RefCell<Option<Rect>>>,
    ) -> Self {
        let tree_view = TreeView::new(Arc::clone(&font))
            .with_row_height(20.0)
            .with_font_size(12.0)
            .with_indent_width(14.0);
        Self {
            bounds: Rect::default(),
            _children: Vec::new(),
            font,
            nodes,
            selected: None,
            props_h: DEFAULT_PROPS_H,
            split_dragging: false,
            hovered_bounds,
            tree_view,
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

    fn on_split_handle(&self, pos: Point) -> bool {
        let sy = self.split_y();
        pos.y >= sy - SPLIT_HIT && pos.y <= sy + SPLIT_HIT
    }

    fn pos_in_tree_area(&self, pos: Point) -> bool {
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        pos.y >= tree_bot && pos.y <= tree_top
    }

    /// Forward event to the TreeView, translating Y into tree-local coordinates.
    fn forward_to_tree(&mut self, event: &Event) -> EventResult {
        // tree_view.bounds().y is tree_origin_y() in panel-local space — subtracting
        // it converts panel-local Y to TreeView-local Y (where y=0 is the bottom of
        // the tree area).
        let offset_y = self.tree_view.bounds().y;
        let translated = translate_event(event, offset_y);
        self.tree_view.on_event(&translated)
    }
}

// ── Widget impl ───────────────────────────────────────────────────────────────

impl Widget for InspectorPanel {
    fn type_name(&self) -> &'static str { "InspectorPanel" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self._children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self._children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds.width  = available.width;
        self.bounds.height = available.height;

        let nodes = self.nodes.borrow();

        // Preserve expansion/selection state by index before rebuilding.
        let old_expanded: Vec<bool> = self.tree_view.nodes.iter()
            .map(|n| n.is_expanded).collect();
        let old_selected: Vec<bool> = self.tree_view.nodes.iter()
            .map(|n| n.is_selected).collect();

        self.tree_view.nodes.clear();

        // Convert flat InspectorNode list (with depths) to parent-child TreeNode
        // structure. Uses a depth stack: depth_stack[d] = tree node index of the
        // last node placed at depth d.
        let mut depth_stack: Vec<usize> = Vec::new();
        let mut per_parent_counts: std::collections::HashMap<Option<usize>, u32> =
            std::collections::HashMap::new();

        for (orig_idx, node) in nodes.iter().enumerate() {
            let parent = if node.depth == 0 {
                None
            } else {
                depth_stack.get(node.depth.saturating_sub(1)).copied()
            };

            let order = {
                let cnt = per_parent_counts.entry(parent).or_insert(0);
                let o = *cnt;
                *cnt += 1;
                o
            };

            // Label: "TypeName  width×height"
            let b = &node.screen_bounds;
            let label = format!("{}  {:.0}×{:.0}", node.type_name, b.width, b.height);

            let tv_idx = self.tree_view.nodes.len();
            self.tree_view.nodes.push(TreeNode::new(label, NodeIcon::Package, parent, order));

            // Restore or default expansion (default: expanded so tree is open).
            self.tree_view.nodes[tv_idx].is_expanded =
                old_expanded.get(orig_idx).copied().unwrap_or(true);
            self.tree_view.nodes[tv_idx].is_selected =
                old_selected.get(orig_idx).copied().unwrap_or(false);

            // Update depth stack.
            if depth_stack.len() <= node.depth {
                depth_stack.resize(node.depth + 1, 0);
            }
            depth_stack[node.depth] = tv_idx;
        }

        // Sync selected field from TreeView selection.
        self.selected = self.tree_view.nodes.iter().position(|n| n.is_selected);

        // Update hovered_bounds for the render-loop overlay.
        *self.hovered_bounds.borrow_mut() = self.tree_view.hovered_node_idx()
            .and_then(|i| nodes.get(i))
            .map(|n| n.screen_bounds);

        // Layout the TreeView inside the tree area.
        let tree_w   = available.width;
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        let tree_h   = (tree_top - tree_bot).max(0.0);
        self.tree_view.set_bounds(Rect::new(0.0, tree_bot, tree_w, tree_h));
        self.tree_view.layout(Size::new(tree_w, tree_h));

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w     = self.bounds.width;
        let h     = self.bounds.height;
        let sy    = self.split_y();
        let hdr_y = h - HEADER_H;

        // ── panel background ─────────────────────────────────────────────────
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

        // ── tree area: clip then paint TreeView ──────────────────────────────
        let tree_bot = self.tree_origin_y();
        let tree_top = self.list_area_h();
        let tree_h   = (tree_top - tree_bot).max(0.0);
        if tree_h > 0.0 {
            ctx.save();
            ctx.clip_rect(0.0, tree_bot, w, tree_h);
            ctx.translate(0.0, tree_bot);
            self.tree_view.paint(ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if self.on_split_handle(*pos) {
                    self.split_dragging = true;
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    return self.forward_to_tree(event);
                }
                EventResult::Ignored
            }
            Event::MouseMove { pos } => {
                if self.split_dragging {
                    self.props_h = pos.y.clamp(
                        MIN_PROPS_H,
                        (self.list_area_h() - MIN_TREE_H).max(MIN_PROPS_H),
                    );
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    let _ = self.forward_to_tree(event);
                }
                EventResult::Ignored
            }
            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                if self.split_dragging {
                    self.split_dragging = false;
                    return EventResult::Consumed;
                }
                if self.pos_in_tree_area(*pos) {
                    return self.forward_to_tree(event);
                }
                EventResult::Ignored
            }
            Event::MouseWheel { pos, .. } if self.pos_in_tree_area(*pos) => {
                self.forward_to_tree(event)
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
