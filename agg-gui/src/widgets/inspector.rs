//! Widget inspector panel — shows the live widget tree in a side panel.
//!
//! `InspectorPanel` is placed as the last child of the root `Stack` so it
//! paints on top of everything.  When `show` is `false` it is fully
//! transparent and passes all events through.
//!
//! Usage:
//! ```rust
//! let show = Rc::new(Cell::new(false));
//! let nodes = Rc::new(RefCell::new(Vec::new()));
//! let panel = InspectorPanel::new(font, Rc::clone(&show), Rc::clone(&nodes));
//! ```
//! Before each paint, call `app.collect_inspector_nodes()` and write the
//! result into `nodes`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{InspectorNode, Widget};

const PANEL_W:   f64 = 300.0;
const ROW_H:     f64 = 20.0;
const INDENT_W:  f64 = 14.0;
const HEADER_H:  f64 = 32.0;
const FONT_SIZE: f64 = 12.0;

pub struct InspectorPanel {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    show: Rc<Cell<bool>>,
    nodes: Rc<RefCell<Vec<InspectorNode>>>,
    scroll_offset: f64,
    selected: Option<usize>,
    hovered_row: Option<usize>,
}

impl InspectorPanel {
    pub fn new(
        font: Arc<Font>,
        show: Rc<Cell<bool>>,
        nodes: Rc<RefCell<Vec<InspectorNode>>>,
    ) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            show,
            nodes,
            scroll_offset: 0.0,
            selected: None,
            hovered_row: None,
        }
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn panel_x(&self) -> f64 { self.bounds.width - PANEL_W }

    /// Row index under `pos` (in this widget's local coordinates), or None.
    fn row_at(&self, pos: Point) -> Option<usize> {
        if pos.x < self.panel_x() { return None; }
        let list_y_bottom = self.bounds.height - HEADER_H;  // list area bottom
        if pos.y > list_y_bottom { return None; }            // above header
        let n = self.nodes.borrow().len();
        let local_y = pos.y + self.scroll_offset;            // Y-up local in list
        let row = (local_y / ROW_H) as usize;
        if row < n { Some(row) } else { None }
    }

    fn max_scroll(&self) -> f64 {
        let n = self.nodes.borrow().len() as f64;
        let list_h = (self.bounds.height - HEADER_H).max(0.0);
        (n * ROW_H - list_h).max(0.0)
    }
}

impl Widget for InspectorPanel {
    fn type_name(&self) -> &'static str { "InspectorPanel" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds.width  = available.width;
        self.bounds.height = available.height;
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.show.get() { return; }

        let w  = self.bounds.width;
        let h  = self.bounds.height;
        let px = self.panel_x();

        // ── dim overlay on the left ─────────────────────────────────────────
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.18));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, px, h);
        ctx.fill();

        // ── panel background ────────────────────────────────────────────────
        ctx.set_fill_color(Color::rgb(0.14, 0.14, 0.18));
        ctx.begin_path();
        ctx.rect(px, 0.0, PANEL_W, h);
        ctx.fill();

        // Left border
        ctx.set_stroke_color(Color::rgba(1.0, 1.0, 1.0, 0.08));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(px, 0.0);
        ctx.line_to(px, h);
        ctx.stroke();

        // ── header ─────────────────────────────────────────────────────────
        let hdr_y = h - HEADER_H;
        ctx.set_fill_color(Color::rgb(0.18, 0.18, 0.24));
        ctx.begin_path();
        ctx.rect(px, hdr_y, PANEL_W, HEADER_H);
        ctx.fill();

        ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(13.0);
        if let Some(m) = ctx.measure_text("Widget Inspector") {
            ctx.fill_text("Widget Inspector", px + 12.0, hdr_y + (HEADER_H - (m.ascent + m.descent)) * 0.5 + m.descent);
        }

        // Count display
        let n = self.nodes.borrow().len();
        let count_txt = format!("{n} widgets");
        ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 0.40));
        ctx.set_font_size(11.0);
        if let Some(m) = ctx.measure_text(&count_txt) {
            ctx.fill_text(&count_txt, w - m.width - 10.0, hdr_y + (HEADER_H - (m.ascent + m.descent)) * 0.5 + m.descent);
        }

        // ── list rows ───────────────────────────────────────────────────────
        ctx.set_font_size(FONT_SIZE);
        let list_h = hdr_y;
        let nodes = self.nodes.borrow();

        for (i, node) in nodes.iter().enumerate() {
            let row_y_bottom = i as f64 * ROW_H - self.scroll_offset;
            let row_y_top    = row_y_bottom + ROW_H;
            // Skip rows outside the visible list area
            if row_y_top < 0.0 || row_y_bottom > list_h { continue; }

            let is_selected = self.selected == Some(i);
            let is_hovered  = self.hovered_row == Some(i);

            // Row background
            let bg = if is_selected {
                Color::rgba(0.22, 0.45, 0.88, 0.35)
            } else if is_hovered {
                Color::rgba(1.0, 1.0, 1.0, 0.06)
            } else if i % 2 == 0 {
                Color::rgba(1.0, 1.0, 1.0, 0.02)
            } else {
                Color::transparent()
            };
            if bg.a > 0.0 {
                ctx.set_fill_color(bg);
                ctx.begin_path();
                ctx.rect(px, row_y_bottom, PANEL_W, ROW_H);
                ctx.fill();
            }

            // Indented type name
            let indent = node.depth as f64 * INDENT_W;
            let tx = px + 8.0 + indent;
            let ty = row_y_bottom + (ROW_H - FONT_SIZE) * 0.5 + FONT_SIZE * 0.75;

            // Depth guide lines
            if node.depth > 0 {
                ctx.set_stroke_color(Color::rgba(1.0, 1.0, 1.0, 0.08));
                ctx.set_line_width(1.0);
                ctx.begin_path();
                ctx.move_to(px + 8.0 + (node.depth as f64 - 1.0) * INDENT_W + INDENT_W * 0.5,
                             row_y_bottom);
                ctx.line_to(px + 8.0 + (node.depth as f64 - 1.0) * INDENT_W + INDENT_W * 0.5,
                             row_y_bottom + ROW_H * 0.5);
                ctx.line_to(tx, row_y_bottom + ROW_H * 0.5);
                ctx.stroke();
            }

            let text_color = if is_selected {
                Color::rgb(0.85, 0.92, 1.0)
            } else {
                Color::rgba(1.0, 1.0, 1.0, 0.80)
            };
            ctx.set_fill_color(text_color);
            ctx.fill_text(node.type_name, tx, ty);

            // Bounds annotation (right-aligned, dimmed)
            let b = &node.screen_bounds;
            let dim_txt = format!("{:.0}×{:.0}", b.width, b.height);
            ctx.set_fill_color(Color::rgba(1.0, 1.0, 1.0, 0.28));
            ctx.set_font_size(10.0);
            if let Some(m) = ctx.measure_text(&dim_txt) {
                ctx.fill_text(&dim_txt, w - m.width - 6.0, ty);
            }
            ctx.set_font_size(FONT_SIZE);
        }
        drop(nodes);

        // ── selected widget highlight overlay (drawn over the main content) ─
        if let Some(sel) = self.selected {
            if let Some(node) = self.nodes.borrow().get(sel) {
                let b = &node.screen_bounds;
                ctx.set_stroke_color(Color::rgba(0.22, 0.70, 1.0, 0.90));
                ctx.set_line_width(2.0);
                ctx.begin_path();
                ctx.rect(b.x, b.y, b.width, b.height);
                ctx.stroke();
                ctx.set_fill_color(Color::rgba(0.22, 0.70, 1.0, 0.08));
                ctx.begin_path();
                ctx.rect(b.x, b.y, b.width, b.height);
                ctx.fill();
            }
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        self.show.get() && local_pos.x >= self.panel_x()
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if !self.show.get() { return EventResult::Ignored; }
        match event {
            Event::MouseMove { pos } => {
                if pos.x >= self.panel_x() {
                    self.hovered_row = self.row_at(*pos);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if pos.x >= self.panel_x() {
                    self.selected = self.row_at(*pos);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseWheel { pos, delta_y } => {
                if pos.x >= self.panel_x() {
                    self.scroll_offset = (self.scroll_offset - delta_y * 30.0)
                        .clamp(0.0, self.max_scroll());
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}
