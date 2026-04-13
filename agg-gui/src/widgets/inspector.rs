//! Widget inspector panel — light-themed panel that fills its assigned bounds.
//!
//! Layout (Y-up, panel fills its full bounds):
//!
//! ```text
//! ┌─────────────────────┐ ← top (HEADER_H)  header
//! ├─────────────────────┤
//! │   tree rows…        │ ← tree area (scrollable)
//! ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤ ← draggable h-split
//! │   Properties        │ ← props area
//! └─────────────────────┘ ← bottom (y=0)
//! ```
//!
//! Width is controlled by the parent (TabView sidebar divider).
//! The horizontal split between tree and props is user-draggable.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{InspectorNode, Widget};
use crate::widgets::primitives::SizedBox;

// ── geometry constants ────────────────────────────────────────────────────────
const DEFAULT_PROPS_H: f64 = 180.0;
const ROW_H:           f64 = 20.0;
const INDENT_W:        f64 = 14.0;
const HEADER_H:        f64 = 30.0;
const FONT_SIZE:       f64 = 12.0;
const SPLIT_HIT:       f64 = 5.0;
const MIN_PROPS_H:     f64 = 60.0;
const MIN_TREE_H:      f64 = 60.0;

// ── light theme colors ────────────────────────────────────────────────────────
fn c_panel_bg()   -> Color { Color::rgb(0.965, 0.968, 0.975) }
fn c_header_bg()  -> Color { Color::rgb(0.910, 0.915, 0.925) }
fn c_props_bg()   -> Color { Color::rgb(0.950, 0.952, 0.960) }
fn c_split_bg()   -> Color { Color::rgba(0.0, 0.0, 0.0, 0.08) }
fn c_border()     -> Color { Color::rgba(0.0, 0.0, 0.0, 0.12) }
fn c_text()       -> Color { Color::rgb(0.12, 0.12, 0.15) }
fn c_dim_text()   -> Color { Color::rgba(0.0, 0.0, 0.0, 0.42) }
fn c_guide()      -> Color { Color::rgba(0.0, 0.0, 0.0, 0.10) }
fn c_row_hover()  -> Color { Color::rgba(0.10, 0.40, 0.90, 0.08) }
fn c_row_sel()    -> Color { Color::rgba(0.10, 0.40, 0.90, 0.14) }
fn c_row_alt()    -> Color { Color::rgba(0.0, 0.0, 0.0, 0.025) }

// ── struct ────────────────────────────────────────────────────────────────────

pub struct InspectorPanel {
    bounds:         Rect,
    children:       Vec<Box<dyn Widget>>,
    font:           Arc<Font>,
    nodes:          Rc<RefCell<Vec<InspectorNode>>>,
    hovered_bounds: Rc<RefCell<Option<Rect>>>,
    scroll_offset:  f64,
    selected:       Option<usize>,
    hovered_row:    Option<usize>,
    props_h:        f64,
    split_dragging: bool,
}

impl InspectorPanel {
    pub fn new(
        font:           Arc<Font>,
        nodes:          Rc<RefCell<Vec<InspectorNode>>>,
        hovered_bounds: Rc<RefCell<Option<Rect>>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            nodes,
            hovered_bounds,
            scroll_offset: 0.0,
            selected: None,
            hovered_row: None,
            props_h: DEFAULT_PROPS_H,
            split_dragging: false,
        }
    }

    // ── geometry helpers ──────────────────────────────────────────────────────

    /// Height of the area containing both tree and props (below header).
    fn list_area_h(&self) -> f64 { (self.bounds.height - HEADER_H).max(0.0) }

    /// Y coordinate of the tree/props split (from bottom).
    fn split_y(&self) -> f64 {
        self.props_h.clamp(
            MIN_PROPS_H,
            (self.list_area_h() - MIN_TREE_H).max(MIN_PROPS_H),
        )
    }

    /// Y-bottom of the first visible tree row.
    fn tree_origin_y(&self) -> f64 { self.split_y() + 4.0 }

    fn tree_area_h(&self) -> f64 {
        (self.list_area_h() - self.split_y() - 4.0).max(0.0)
    }

    fn max_scroll(&self) -> f64 {
        let n = self.nodes.borrow().len() as f64;
        (n * ROW_H - self.tree_area_h()).max(0.0)
    }

    fn row_y_bottom(&self, i: usize) -> f64 {
        // Y-up, top-down list: row 0 is at the top (highest Y).
        // Row i bottom = list_h - (i+1)*ROW_H + scroll_offset
        // scroll_offset > 0 moves content up (reveals later rows).
        self.list_area_h() - (i as f64 + 1.0) * ROW_H + self.scroll_offset
    }

    fn row_at(&self, pos: Point) -> Option<usize> {
        let tree_top = self.list_area_h();
        let tree_bot = self.tree_origin_y();
        if pos.y < tree_bot || pos.y > tree_top { return None; }
        // row i occupies: bottom = list_h - (i+1)*ROW_H + scroll, top = list_h - i*ROW_H + scroll
        // → i = floor((tree_top + scroll_offset - pos.y) / ROW_H)
        let row = ((tree_top + self.scroll_offset - pos.y) / ROW_H) as usize;
        let n = self.nodes.borrow().len();
        if row < n { Some(row) } else { None }
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

        // Rebuild children: one SizedBox per node, positioned using the
        // correct top-down Y-up formula so tests can inspect row positions.
        let n = self.nodes.borrow().len();
        self.children.clear();
        for i in 0..n {
            let row_bot = self.row_y_bottom(i);
            let mut row = SizedBox::new()
                .with_width(available.width)
                .with_height(ROW_H);
            row.set_bounds(Rect::new(0.0, row_bot, available.width, ROW_H));
            self.children.push(Box::new(row));
        }

        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w  = self.bounds.width;
        let h  = self.bounds.height;
        let sy = self.split_y();
        let list_h = self.list_area_h();
        let hdr_y  = h - HEADER_H;

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
            ctx.fill_text(title, 12.0, hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent);
        }

        let count_txt = format!("{} widgets", self.nodes.borrow().len());
        ctx.set_font_size(11.0);
        ctx.set_fill_color(c_dim_text());
        if let Some(m) = ctx.measure_text(&count_txt) {
            ctx.fill_text(&count_txt, w - m.width - 10.0,
                hdr_y + (HEADER_H - m.ascent - m.descent) * 0.5 + m.descent);
        }

        // ── tree rows ────────────────────────────────────────────────────────
        ctx.set_font_size(FONT_SIZE);
        let nodes = self.nodes.borrow();

        for (i, node) in nodes.iter().enumerate() {
            let row_bot = self.row_y_bottom(i);
            let row_top = row_bot + ROW_H;
            if row_top < self.tree_origin_y() || row_bot > list_h { continue; }

            let is_sel = self.selected == Some(i);
            let is_hov = self.hovered_row == Some(i);

            let bg = if is_sel      { c_row_sel() }
                     else if is_hov { c_row_hover() }
                     else if i % 2 == 0 { c_row_alt() }
                     else { Color::transparent() };

            if bg.a > 0.0 {
                ctx.set_fill_color(bg);
                ctx.begin_path();
                ctx.rect(0.0, row_bot, w, ROW_H);
                ctx.fill();
            }

            let indent = node.depth as f64 * INDENT_W;
            let tx = 8.0 + indent;
            let ty = row_bot + (ROW_H - FONT_SIZE) * 0.5 + FONT_SIZE * 0.75;

            if node.depth > 0 {
                let guide_x = 8.0 + (node.depth as f64 - 1.0) * INDENT_W + INDENT_W * 0.5;
                ctx.set_stroke_color(c_guide());
                ctx.set_line_width(1.0);
                ctx.begin_path();
                ctx.move_to(guide_x, row_bot);
                ctx.line_to(guide_x, row_bot + ROW_H * 0.5);
                ctx.line_to(tx, row_bot + ROW_H * 0.5);
                ctx.stroke();
            }

            ctx.set_fill_color(if is_sel { Color::rgb(0.10, 0.35, 0.80) } else { c_text() });
            ctx.fill_text(node.type_name, tx, ty);

            let b = &node.screen_bounds;
            let sz_txt = format!("{:.0}×{:.0}", b.width, b.height);
            ctx.set_fill_color(c_dim_text());
            ctx.set_font_size(10.0);
            if let Some(m) = ctx.measure_text(&sz_txt) {
                ctx.fill_text(&sz_txt, w - m.width - 6.0, ty);
            }
            ctx.set_font_size(FONT_SIZE);
        }
        drop(nodes);

        // ── horizontal split handle ──────────────────────────────────────────
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

        // ── properties pane ──────────────────────────────────────────────────
        ctx.set_fill_color(c_props_bg());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, sy - 2.0);
        ctx.fill();

        self.paint_properties(ctx, sy - 2.0);
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if self.on_split_handle(*pos) {
                    self.split_dragging = true;
                    return EventResult::Consumed;
                }
                self.selected = self.row_at(*pos);
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
                self.hovered_row = self.row_at(*pos);
                EventResult::Ignored
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                if self.split_dragging {
                    self.split_dragging = false;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseWheel { pos: _, delta_y } => {
                self.scroll_offset = (self.scroll_offset + delta_y * 30.0)
                    .clamp(0.0, self.max_scroll());
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── paint helpers ─────────────────────────────────────────────────────────────

impl InspectorPanel {
    fn paint_properties(&self, ctx: &mut dyn DrawCtx, available_h: f64) {
        if available_h < 4.0 { return; }
        let w = self.bounds.width;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(10.0);
        ctx.set_fill_color(c_dim_text());
        let heading = "PROPERTIES";
        ctx.fill_text(heading, 10.0, available_h - 14.0);

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
            let diag_w = w - 20.0;

            let aspect = if b.height > 0.0 { b.width / b.height } else { 1.0 };
            let box_h = (diag_h * 0.6).min(50.0);
            let box_w = (box_h * aspect).min(diag_w * 0.8);
            let box_x = 10.0 + (diag_w - box_w) * 0.5;
            let box_y = diag_y_top - (diag_h + box_h) * 0.5;

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
                    ctx.fill_text(&dim,
                        box_x + (box_w - m.width) * 0.5,
                        box_y + (box_h - m.ascent - m.descent) * 0.5 + m.descent);
                }
            }
        }
    }
}
