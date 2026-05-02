//! `Table` — a virtualised data-table widget with header, scrolling body,
//! striping, overlines, row-selection, sort-toggle hooks and
//! `scroll_to_row`.
//!
//! Designed to mirror the shape of `egui_extras::TableBuilder` so the same
//! mental model carries over: configure columns (`auto` / `exact` /
//! `remainder.at_least(...).clip(...)`), describe the row set
//! (`Homogeneous { count, height }` or `Heterogeneous { heights }`), and
//! provide cell + header painters.  Cells are produced lazily — only rows
//! within the visible viewport are painted, so 100 000-row demos remain
//! cheap.
//!
//! The widget intentionally does *not* know about your data: it owns the
//! chrome (backgrounds, separators, scroll, selection bookkeeping) and
//! invokes user-supplied painters for cell content.  Cell painters get a
//! `CellInfo` (rect, row, column, selected, font, visuals) and may use any
//! of the project's draw primitives.
//!
//! Y-up note: agg-gui's coordinate system has its origin at the bottom-
//! left.  Rows are stored top-down in the public API (row 0 visually
//! topmost) and the widget converts to Y-up internally.
//!
//! Resizable columns are intentionally not implemented in this first
//! revision; the configuration is preserved so the API doesn't break when
//! it lands later.

mod body;
mod config;
mod state;

pub use config::{
    distribute_widths, CellInfo, CellPainter, ColumnSize, HeaderClick, HeaderInfo, HeaderPainter,
    RowPredicate, RowsProvider, TableColumn, TableRows, MIN_COL_W, RESIZE_HIT_HALF,
};

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::WidgetBase;
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::scroll_view::ScrollView;

use body::TableBody;
use state::TableState;

// ── Builder ────────────────────────────────────────────────────────────────

pub struct TableBuilder {
    state: TableState,
    columns: Vec<TableColumn>,
    header_height: f64,
    header_painter: Option<HeaderPainter>,
    header_click: Option<HeaderClick>,
    /// Forwarded to the inner `ScrollView::with_fade_color` so the
    /// scrollbar's edge-fade gradient blends into the table's actual
    /// ancestor background instead of the default `window_fill`.  When
    /// the table sits in a `FlexColumn::with_panel_bg`, pass the same
    /// panel fill colour here.
    fade_color: Option<Color>,
}

impl Default for TableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TableBuilder {
    pub fn new() -> Self {
        Self {
            state: TableState::defaults(),
            columns: Vec::new(),
            header_height: 22.0,
            header_painter: None,
            header_click: None,
            fade_color: None,
        }
    }

    /// Set the scroll-fade gradient colour on the body's inner
    /// `ScrollView`.  Pass the visible ancestor background colour
    /// (e.g. `Visuals::panel_fill` when the table sits on a panel) so
    /// the fade dissolves invisibly instead of painting a bright halo
    /// of the default `window_fill`.  See [`ScrollView::with_fade_color`].
    pub fn fade_color(mut self, c: Color) -> Self {
        self.fade_color = Some(c);
        self
    }

    pub fn columns(mut self, cols: Vec<TableColumn>) -> Self {
        self.columns = cols;
        self
    }

    pub fn striped(self, on: bool) -> Self {
        self.state.striped.set(on);
        self
    }
    pub fn striped_cell(self, cell: Rc<Cell<bool>>) -> Self {
        Self {
            state: TableState {
                striped: cell,
                ..self.state
            },
            ..self
        }
    }

    pub fn sense_click(self, on: bool) -> Self {
        self.state.sense_click.set(on);
        self
    }
    pub fn sense_click_cell(self, cell: Rc<Cell<bool>>) -> Self {
        Self {
            state: TableState {
                sense_click: cell,
                ..self.state
            },
            ..self
        }
    }

    pub fn rows(self, spec: TableRows) -> Self {
        *self.state.rows.borrow_mut() = spec;
        self
    }
    pub fn rows_cell(self, cell: Rc<RefCell<TableRows>>) -> Self {
        Self {
            state: TableState { rows: cell, ..self.state },
            ..self
        }
    }
    /// Install a closure that produces the current row spec.  The widget
    /// invokes it during each layout pass and writes the result into the
    /// internal `rows` cell, so external state changes flow in without
    /// the caller having to manage an observer widget.
    pub fn rows_provider(self, p: RowsProvider) -> Self {
        *self.state.rows_provider.borrow_mut() = Some(p);
        self
    }

    pub fn overline_pred(self, pred: RowPredicate) -> Self {
        *self.state.overline_pred.borrow_mut() = Some(pred);
        self
    }

    /// Use a `HashSet<usize>` as the selection mask (highlights any row
    /// whose internal index is in the set).  Convenience over
    /// [`Self::selection_pred`] for the common case where rows are not
    /// reversed/transformed.
    pub fn selection(self, sel: Rc<RefCell<HashSet<usize>>>) -> Self {
        let pred: RowPredicate = Box::new(move |i| sel.borrow().contains(&i));
        *self.state.selection_pred.borrow_mut() = Some(pred);
        self
    }

    /// Pass an arbitrary predicate to compute "is row N selected?".  Useful
    /// when display indices differ from internal indices (e.g. reversed
    /// order) and the caller wants the highlight to track the display
    /// index rather than the internal one.
    pub fn selection_pred(self, pred: RowPredicate) -> Self {
        *self.state.selection_pred.borrow_mut() = Some(pred);
        self
    }

    pub fn resizable(self, on: bool) -> Self {
        self.state.resizable.set(on);
        self
    }
    pub fn resizable_cell(self, cell: Rc<Cell<bool>>) -> Self {
        Self {
            state: TableState {
                resizable: cell,
                ..self.state
            },
            ..self
        }
    }

    /// Adopt an external `Rc<RefCell<Vec<Option<f64>>>>` as the column
    /// overrides cell.  External code can clear it (e.g. on a Reset
    /// button) to restore the configured widths.
    pub fn column_overrides_cell(self, cell: Rc<RefCell<Vec<Option<f64>>>>) -> Self {
        Self {
            state: TableState {
                column_overrides: cell,
                ..self.state
            },
            ..self
        }
    }

    pub fn scroll_to_row_cell(self, cell: Rc<Cell<Option<usize>>>) -> Self {
        Self {
            state: TableState {
                scroll_to_row: cell,
                ..self.state
            },
            ..self
        }
    }

    pub fn scroll_offset_cell(self, cell: Rc<Cell<f64>>) -> Self {
        Self {
            state: TableState {
                scroll_offset: cell,
                ..self.state
            },
            ..self
        }
    }

    pub fn header_height(mut self, h: f64) -> Self {
        self.header_height = h;
        self
    }
    pub fn header_painter(mut self, p: HeaderPainter) -> Self {
        self.header_painter = Some(p);
        self
    }
    pub fn header_click(mut self, p: HeaderClick) -> Self {
        self.header_click = Some(p);
        self
    }

    pub fn cell_painter(self, p: CellPainter) -> Self {
        *self.state.cell_painter.borrow_mut() = Some(p);
        self
    }

    pub fn on_row_click(self, f: Box<dyn FnMut(usize, usize)>) -> Self {
        *self.state.on_row_click.borrow_mut() = Some(f);
        self
    }

    /// Materialise the configured table into a widget.
    pub fn build(self, font: Arc<Font>) -> Table {
        let body = TableBody {
            bounds: Rect::default(),
            children: Vec::new(),
            font: Arc::clone(&font),
            state: self.state.clone(),
        };
        let mut scroll = ScrollView::new(Box::new(body))
            .vertical(true)
            .horizontal(true)
            .with_offset_cell(Rc::clone(&self.state.scroll_offset))
            .with_h_offset_cell(Rc::clone(&self.state.h_offset))
            .with_viewport_cell(Rc::clone(&self.state.viewport_cell));
        if let Some(c) = self.fade_color {
            scroll = scroll.with_fade_color(c);
        }

        let n = self.columns.len();
        self.state.column_overrides.borrow_mut().resize(n, None);
        Table {
            bounds: Rect::default(),
            children: vec![Box::new(scroll)],
            base: WidgetBase::new(),
            font,
            columns: self.columns,
            state: self.state,
            header_height: self.header_height,
            header_painter: RefCell::new(self.header_painter),
            header_click: RefCell::new(self.header_click),
            drag_resize: Cell::new(None),
        }
    }
}

// ── Table widget ────────────────────────────────────────────────────────────

pub struct Table {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // [0] = ScrollView wrapping the body widget.
    base: WidgetBase,
    font: Arc<Font>,
    columns: Vec<TableColumn>,
    state: TableState,
    header_height: f64,
    header_painter: RefCell<Option<HeaderPainter>>,
    header_click: RefCell<Option<HeaderClick>>,
    /// Active column resize drag: (column_index, pointer_x_at_down, original_width).
    drag_resize: Cell<Option<(usize, f64, f64)>>,
}

impl Table {
    /// Begin configuring a new table.  Convenience for [`TableBuilder::new`].
    pub fn builder() -> TableBuilder {
        TableBuilder::new()
    }

    /// Reset every column's user-resized override back to its configured
    /// `auto`/`exact`/`remainder` width.
    pub fn reset_column_widths(&self) {
        self.state.column_overrides.borrow_mut().clear();
    }

    /// Read-only handle to the column overrides — `None` for the
    /// configured default, `Some(w)` for user-resized columns.  Useful for
    /// persistence layers that want to save and restore the layout.
    pub fn column_overrides(&self) -> Rc<RefCell<Vec<Option<f64>>>> {
        Rc::clone(&self.state.column_overrides)
    }

    /// Replace the row set in place.  Useful for switching between
    /// homogeneous/heterogeneous modes without rebuilding the widget.
    pub fn set_rows(&self, rows: TableRows) {
        *self.state.rows.borrow_mut() = rows;
    }

    /// Read-only access to the current top-down row set spec.
    pub fn rows_handle(&self) -> Rc<RefCell<TableRows>> {
        Rc::clone(&self.state.rows)
    }

    pub fn margin(mut self, m: crate::layout_props::Insets) -> Self {
        self.base.margin = m;
        self
    }
}

impl Widget for Table {
    fn type_name(&self) -> &'static str {
        "Table"
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

    fn margin(&self) -> crate::layout_props::Insets {
        self.base.margin
    }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width.max(40.0);
        let h = available.height.max(40.0);
        self.bounds = Rect::new(0.0, 0.0, w, h);

        // Pull live row spec from the provider closure (if any) before any
        // measurement that depends on row count or heights.
        if let Some(p) = self.state.rows_provider.borrow().as_ref() {
            let new_rows = p();
            *self.state.rows.borrow_mut() = new_rows;
        }

        // Apply any pending scroll-to-row before laying out the scroll view.
        if let Some(target) = self.state.scroll_to_row.get() {
            self.state.scroll_to_row.set(None);
            let rows = self.state.rows.borrow();
            let n = rows.count();
            if n > 0 {
                let target = target.min(n - 1);
                self.state.scroll_offset.set(rows.top_down_y_at(target));
            }
        }

        // Compute column widths & publish.  The natural content width
        // (sum of column widths) may exceed the table's own width when
        // the user has resized columns wider than the viewport — the
        // body's ScrollView handles horizontal panning in that case.
        let overrides = self.state.column_overrides.borrow().clone();
        let widths = distribute_widths(&self.columns, w, &overrides);
        let content_w: f64 = widths.iter().sum();
        *self.state.widths.borrow_mut() = widths;
        self.state.content_w.set(content_w);

        // Body / scroll view fills the area below the header.
        let body_h = (h - self.header_height).max(0.0);
        let scroll = &mut self.children[0];
        scroll.layout(Size::new(w, body_h));
        scroll.set_bounds(Rect::new(0.0, 0.0, w, body_h));

        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let widths = self.state.widths.borrow().clone();
        let header_y = self.bounds.height - self.header_height;
        let h = self.header_height;
        let viewport_w = self.bounds.width;
        let h_offset = self.state.h_offset.get();

        // Header background spans the visible viewport width — the
        // backdrop is always opaque even when the table content is
        // wider than the viewport.
        ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.5, 0.10));
        ctx.begin_path();
        ctx.rect(0.0, header_y, viewport_w, h);
        ctx.fill();

        // Header bottom border.
        ctx.set_stroke_color(v.separator);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, header_y);
        ctx.line_to(viewport_w, header_y);
        ctx.stroke();

        // Clip and translate so column-cell painters and separators draw
        // in CONTENT space (origin = leftmost column edge) but render
        // only within the visible header strip.
        ctx.save();
        ctx.clip_rect(0.0, header_y, viewport_w, h);
        ctx.translate(-h_offset, 0.0);

        // Per-column header painters.
        if let Some(painter) = self.header_painter.borrow_mut().as_mut() {
            let mut x = 0.0;
            for (col, &cw) in widths.iter().enumerate() {
                let info = HeaderInfo {
                    col,
                    rect: Rect::new(x, header_y, cw, h),
                    visuals: &v,
                    font: &self.font,
                };
                ctx.save();
                ctx.clip_rect(x, header_y, cw, h);
                painter(&info, ctx);
                ctx.restore();
                x += cw;
            }
        }

        // Vertical column separators across the header.  Resizable column
        // edges get a slightly thicker line so users can see where to grab.
        let mut sx = 0.0;
        let dragging = self.drag_resize.get().map(|(c, _, _)| c);
        for (i, &cw) in widths.iter().enumerate() {
            sx += cw;
            if i + 1 < widths.len() {
                let is_resizable = self.columns.get(i).map(|c| c.resizable).unwrap_or(false)
                    && self.state.resizable.get();
                let is_active = dragging == Some(i);
                let color = if is_active {
                    v.accent
                } else if is_resizable {
                    Color::rgba(v.separator.r, v.separator.g, v.separator.b, 0.9)
                } else {
                    v.separator
                };
                ctx.set_stroke_color(color);
                ctx.set_line_width(if is_active { 2.0 } else { 1.0 });
                ctx.begin_path();
                ctx.move_to(sx, header_y);
                ctx.line_to(sx, header_y + h);
                ctx.stroke();
            }
        }
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let header_y = self.bounds.height - self.header_height;
        let in_header = |y: f64| y >= header_y && y <= header_y + self.header_height;
        let h_offset = self.state.h_offset.get();

        // Helper: which (resizable) column edge — if any — is the pointer
        // (in CONTENT space, i.e. with the horizontal scroll offset
        // already added back) within `RESIZE_HIT_HALF` of?  Used both
        // for cursor change on hover and for starting a drag.
        let resize_target_at = |content_x: f64, y: f64| -> Option<(usize, f64)> {
            if !in_header(y) || !self.state.resizable.get() {
                return None;
            }
            let widths = self.state.widths.borrow().clone();
            let mut acc = 0.0;
            for (col, &cw) in widths.iter().enumerate() {
                let edge = acc + cw;
                let last = col + 1 == widths.len();
                let resizable = self
                    .columns
                    .get(col)
                    .map(|c| c.resizable)
                    .unwrap_or(false);
                if !last && resizable && (content_x - edge).abs() <= RESIZE_HIT_HALF {
                    return Some((col, cw));
                }
                acc += cw;
            }
            None
        };

        // Active resize drag — must be checked first so MouseMove stays
        // routed to us via the framework's mouse-capture path.
        if let Some((col, content_x0, w0)) = self.drag_resize.get() {
            match event {
                Event::MouseMove { pos } => {
                    set_cursor_icon(CursorIcon::ResizeHorizontal);
                    let content_x = pos.x + h_offset;
                    let dx = content_x - content_x0;
                    let new_w = (w0 + dx).max(MIN_COL_W);
                    let mut overs = self.state.column_overrides.borrow_mut();
                    if overs.len() <= col {
                        overs.resize(col + 1, None);
                    }
                    overs[col] = Some(new_w);
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                Event::MouseUp {
                    button: MouseButton::Left,
                    ..
                } => {
                    self.drag_resize.set(None);
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                _ => {}
            }
        }

        // Hover affordance for resize handles — only meaningful when not
        // already dragging (handled above).
        if let Event::MouseMove { pos } = event {
            let content_x = pos.x + h_offset;
            if resize_target_at(content_x, pos.y).is_some() {
                set_cursor_icon(CursorIcon::ResizeHorizontal);
                return EventResult::Consumed;
            }
        }

        if let Event::MouseDown {
            pos,
            button: MouseButton::Left,
            ..
        } = event
        {
            let content_x = pos.x + h_offset;
            if let Some((col, cw)) = resize_target_at(content_x, pos.y) {
                // SNAPSHOT every column's current width into overrides at
                // drag-start.  Without this, dragging one Remainder column
                // would leak space taken/given into the other Remainder
                // columns via re-distribution; the user expects only the
                // dragged edge to move.  Sized columns already had their
                // override (or are fixed) — pinning everything ensures
                // distribute_widths returns the same vector after the
                // drag for non-target columns.
                {
                    let widths = self.state.widths.borrow().clone();
                    let mut overs = self.state.column_overrides.borrow_mut();
                    overs.resize(widths.len(), None);
                    for (j, &w) in widths.iter().enumerate() {
                        if overs[j].is_none() {
                            overs[j] = Some(w);
                        }
                    }
                }
                self.drag_resize.set(Some((col, content_x, cw)));
                set_cursor_icon(CursorIcon::ResizeHorizontal);
                crate::animation::request_draw();
                return EventResult::Consumed;
            }
        }

        if let Event::MouseUp {
            pos,
            button: MouseButton::Left,
            ..
        } = event
        {
            if in_header(pos.y) {
                let widths = self.state.widths.borrow().clone();
                let content_x = pos.x + h_offset;
                let mut x = 0.0;
                for (col, cw) in widths.iter().enumerate() {
                    if content_x >= x && content_x < x + cw {
                        let local_x = content_x - x;
                        let local_y = pos.y - header_y;
                        if let Some(cb) = self.header_click.borrow_mut().as_mut() {
                            let r = cb(col, local_x, local_y);
                            if r == EventResult::Consumed {
                                crate::animation::request_draw();
                            }
                            return r;
                        }
                        return EventResult::Ignored;
                    }
                    x += cw;
                }
            }
        }
        EventResult::Ignored
    }
}

// ── Helpers exposed to users for cell painters ──────────────────────────────

/// Trim `text` from the end with an ellipsis until it fits `max_w`, using
/// the current font/size on `ctx`.
pub fn clip_text_to_width(ctx: &dyn DrawCtx, text: &str, max_w: f64) -> String {
    if let Some(m) = ctx.measure_text(text) {
        if m.width <= max_w {
            return text.to_string();
        }
    }
    let mut out = text.to_string();
    let ell = "…";
    while !out.is_empty() {
        out.pop();
        let candidate = format!("{out}{ell}");
        if let Some(m) = ctx.measure_text(&candidate) {
            if m.width <= max_w {
                return candidate;
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribute_widths_splits_remainders_equally() {
        let cols = vec![
            TableColumn::auto(50.0),
            TableColumn::remainder().at_least(40.0),
            TableColumn::auto(60.0),
            TableColumn::remainder(),
            TableColumn::remainder(),
        ];
        let widths = distribute_widths(&cols, 410.0, &[]);
        assert_eq!(widths[0], 50.0);
        assert_eq!(widths[2], 60.0);
        assert!((widths[1] - 100.0).abs() < 0.001);
        assert!((widths[3] - 100.0).abs() < 0.001);
        assert!((widths[4] - 100.0).abs() < 0.001);
    }

    #[test]
    fn distribute_widths_respects_at_least() {
        let cols = vec![
            TableColumn::auto(200.0),
            TableColumn::remainder().at_least(40.0),
        ];
        let widths = distribute_widths(&cols, 100.0, &[]);
        assert!(widths[1] >= 40.0);
    }

    #[test]
    fn distribute_widths_pins_overrides_and_redistributes_remainders() {
        let cols = vec![
            TableColumn::auto(50.0),
            TableColumn::remainder().at_least(20.0),
            TableColumn::remainder().at_least(20.0),
            TableColumn::remainder().at_least(20.0),
        ];
        // User dragged column 1 to 200 px wide.
        let overrides = vec![None, Some(200.0), None, None];
        let widths = distribute_widths(&cols, 500.0, &overrides);
        assert_eq!(widths[0], 50.0);
        assert!((widths[1] - 200.0).abs() < 0.001);
        // Remaining 250 split between cols 2 and 3 = 125 each.
        assert!((widths[2] - 125.0).abs() < 0.001);
        assert!((widths[3] - 125.0).abs() < 0.001);
    }

    #[test]
    fn distribute_widths_clamps_override_min() {
        let cols = vec![
            TableColumn::auto(100.0),
            TableColumn::remainder().at_least(20.0),
        ];
        let widths = distribute_widths(&cols, 200.0, &[Some(2.0), None]);
        assert!(widths[0] >= MIN_COL_W);
    }

    #[test]
    fn rows_homogeneous_total() {
        let r = TableRows::Homogeneous {
            count: 5,
            height: 10.0,
        };
        assert_eq!(r.total_height(), 50.0);
        assert_eq!(r.height_at(3), 10.0);
        assert_eq!(r.top_down_y_at(2), 20.0);
    }

    #[test]
    fn rows_heterogeneous_total() {
        let r = TableRows::Heterogeneous {
            heights: vec![10.0, 20.0, 30.0],
        };
        assert_eq!(r.total_height(), 60.0);
        assert_eq!(r.top_down_y_at(2), 30.0);
    }
}
