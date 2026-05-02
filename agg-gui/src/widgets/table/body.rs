//! `TableBody` — the virtualised, scrollable body of a `Table`.
//!
//! This private widget is wrapped in a `ScrollView` by `TableBuilder::build`
//! and registered as the single child of `Table`.  It renders only the rows
//! visible in the current scroll viewport, keeping large row counts cheap.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widget::Widget;

use super::config::{CellInfo, TableRows};
use super::state::TableState;

// ── TableBody (private, virtualised) ────────────────────────────────────────

pub(super) struct TableBody {
    pub(super) bounds: Rect,
    pub(super) children: Vec<Box<dyn Widget>>,
    pub(super) font: Arc<Font>,
    pub(super) state: TableState,
}

impl TableBody {
    fn first_visible_row(&self, top_down_y: f64) -> usize {
        let rows = self.state.rows.borrow();
        let n = rows.count();
        match &*rows {
            TableRows::Homogeneous { height, .. } => {
                if *height <= 0.0 {
                    return 0;
                }
                ((top_down_y / *height).floor().max(0.0) as usize).min(n)
            }
            TableRows::Heterogeneous { heights } => {
                let mut y = 0.0;
                for (i, &h) in heights.iter().enumerate() {
                    if y + h > top_down_y {
                        return i;
                    }
                    y += h;
                }
                n
            }
        }
    }
}

impl Widget for TableBody {
    fn type_name(&self) -> &'static str {
        "TableBody"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        // Body content width = sum of column widths.  When that exceeds
        // the viewport width, the ScrollView wrapping us turns on
        // horizontal scrolling.  Falling back to `available.width` keeps
        // the body sensible on the very first frame before
        // `Table::layout` has had a chance to publish widths.
        let widths = self.state.widths.borrow();
        let sum_w: f64 = widths.iter().sum();
        drop(widths);
        let w = if sum_w > 0.0 { sum_w } else { available.width };
        let total_h = self.state.rows.borrow().total_height().max(1.0);
        self.bounds = Rect::new(0.0, 0.0, w, total_h);
        Size::new(w, total_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let widths = self.state.widths.borrow().clone();
        if widths.is_empty() {
            return;
        }
        let total_h = self.bounds.height;
        let total_w: f64 = widths.iter().sum();
        let rows = self.state.rows.borrow();
        let n = rows.count();
        if n == 0 {
            return;
        }

        let vp = self.state.viewport_cell.get();
        let visible_top = vp.y.max(0.0);
        let visible_bottom = (vp.y + vp.height).min(total_h);

        let striped = self.state.striped.get();
        let overline_pred = self.state.overline_pred.borrow();
        let selection_pred = self.state.selection_pred.borrow();
        let hovered_row = self.state.hovered_row.get();

        let first = self.first_visible_row(visible_top);
        let mut y_td = rows.top_down_y_at(first);

        let mut painter_opt = self.state.cell_painter.borrow_mut();

        for i in first..n {
            if y_td >= visible_bottom + 0.5 {
                break;
            }
            let h = rows.height_at(i);
            let row_y_yup = total_h - y_td - h;
            let selected = selection_pred
                .as_ref()
                .map(|p| p(i))
                .unwrap_or(false);

            if selected {
                ctx.set_fill_color(v.selection_bg);
                ctx.begin_path();
                ctx.rect(0.0, row_y_yup, total_w, h);
                ctx.fill();
            } else if striped && i % 2 == 0 {
                ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.07));
                ctx.begin_path();
                ctx.rect(0.0, row_y_yup, total_w, h);
                ctx.fill();
            }
            // Hover highlight — drawn ON TOP of striping so the user sees
            // the affordance even on a striped row, but UNDER selection
            // so the selection colour wins when both apply (it would be
            // visually noisy to mix a hover tint into a selected row).
            if !selected && hovered_row == Some(i) {
                ctx.set_fill_color(Color::rgba(
                    v.accent.r,
                    v.accent.g,
                    v.accent.b,
                    0.10,
                ));
                ctx.begin_path();
                ctx.rect(0.0, row_y_yup, total_w, h);
                ctx.fill();
            }

            if let Some(pred) = overline_pred.as_ref() {
                if pred(i) {
                    ctx.set_stroke_color(v.accent);
                    ctx.set_line_width(1.5);
                    ctx.begin_path();
                    ctx.move_to(0.0, row_y_yup + h);
                    ctx.line_to(total_w, row_y_yup + h);
                    ctx.stroke();
                }
            }

            if let Some(painter) = painter_opt.as_mut() {
                let mut x = 0.0;
                for (col, &cw) in widths.iter().enumerate() {
                    let info = CellInfo {
                        row: i,
                        col,
                        rect: Rect::new(x, row_y_yup, cw, h),
                        selected,
                        visuals: &v,
                        font: &self.font,
                    };
                    ctx.save();
                    ctx.clip_rect(x, row_y_yup, cw, h);
                    painter(&info, ctx);
                    ctx.restore();
                    x += cw;
                }
            }

            y_td += h;
        }

        // Vertical column dividers spanning the visible band.
        ctx.set_stroke_color(Color::rgba(v.separator.r, v.separator.g, v.separator.b, 0.4));
        ctx.set_line_width(1.0);
        let visible_top_yup = total_h - visible_bottom;
        let visible_bot_yup = total_h - visible_top;
        let mut sx = 0.0;
        for (c, &cw) in widths.iter().enumerate() {
            sx += cw;
            if c + 1 < widths.len() {
                ctx.begin_path();
                ctx.move_to(sx, visible_top_yup);
                ctx.line_to(sx, visible_bot_yup);
                ctx.stroke();
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        // Always track hover, even when sense_click is off — the hover
        // tint is a visual affordance that works independently of click
        // handling (matches how rows in egui glow on hover regardless
        // of sense).
        if let Event::MouseMove { pos } = event {
            let mut new_hover: Option<usize> = None;
            if pos.x >= 0.0
                && pos.x <= self.bounds.width
                && pos.y >= 0.0
                && pos.y <= self.bounds.height
            {
                let total_h = self.bounds.height;
                let rows = self.state.rows.borrow();
                let n = rows.count();
                let y_td = total_h - pos.y;
                let mut y_acc = 0.0;
                for i in 0..n {
                    let h = rows.height_at(i);
                    if y_td >= y_acc && y_td <= y_acc + h {
                        new_hover = Some(i);
                        break;
                    }
                    y_acc += h;
                }
            }
            let prev = self.state.hovered_row.get();
            if prev != new_hover {
                self.state.hovered_row.set(new_hover);
                crate::animation::request_draw();
            }
            // Don't consume — allow ScrollView wheel handling etc. to
            // continue receiving the event.
        }

        if !self.state.sense_click.get() {
            return EventResult::Ignored;
        }
        if let Event::MouseUp {
            pos,
            button: MouseButton::Left,
            ..
        } = event
        {
            if pos.x < 0.0 || pos.x > self.bounds.width || pos.y < 0.0 || pos.y > self.bounds.height
            {
                return EventResult::Ignored;
            }
            let total_h = self.bounds.height;
            let widths = self.state.widths.borrow().clone();
            let rows = self.state.rows.borrow();
            let n = rows.count();
            let y_td = total_h - pos.y;
            let mut y_acc = 0.0;
            for i in 0..n {
                let h = rows.height_at(i);
                if y_td >= y_acc && y_td <= y_acc + h {
                    let mut x = 0.0;
                    let mut col_hit = 0;
                    for (col, &cw) in widths.iter().enumerate() {
                        if pos.x >= x && pos.x < x + cw {
                            col_hit = col;
                            break;
                        }
                        x += cw;
                    }
                    drop(rows);
                    if let Some(cb) = self.state.on_row_click.borrow_mut().as_mut() {
                        cb(i, col_hit);
                    }
                    // Selection / "checked" toggles drive visual changes
                    // a layer above us; we cannot tell from here whether
                    // anything changed, so always schedule a frame.  This
                    // mirrors the `request_draw` policy other interactive
                    // widgets in the project follow.
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                y_acc += h;
            }
        }
        EventResult::Ignored
    }
}
