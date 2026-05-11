//! Table demo — parity port of egui's `TableDemo` built on the library
//! `agg_gui::Table` widget.  The demo is a thin layer of state + a cell
//! painter; all chrome (header, scrolling, virtualisation, striping,
//! overlines, selection, scroll-to-row) lives in the library widget.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, CellInfo, Checkbox, Color, Conditional, EventResult, FlexColumn, FlexRow, Font,
    HeaderInfo, Label, RadioGroup, Separator, Size, SizedBox, Slider, Table, TableBuilder,
    TableColumn, TableRows, Widget,
};

const NUM_MANUAL_ROWS: usize = 20;
const TEXT_HEIGHT: f64 = 18.0;
const ROW_THIN: f64 = 18.0;
const ROW_THICK: f64 = 30.0;
const CELL_PAD_X: f64 = 6.0;

fn thick_row(i: usize) -> bool {
    i % 6 == 0
}

fn long_text(i: usize) -> String {
    format!(
        "Row {i} has some long text that you may want to clip, or it will take up too much horizontal space!"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DemoType {
    Manual,
    ManyHomogeneous,
    ManyHeterogeneous,
}

impl DemoType {
    fn from_idx(i: usize) -> Self {
        match i {
            0 => DemoType::Manual,
            1 => DemoType::ManyHomogeneous,
            _ => DemoType::ManyHeterogeneous,
        }
    }
}

fn build_rows(demo: DemoType, num_rows: usize) -> TableRows {
    match demo {
        DemoType::Manual => TableRows::Heterogeneous {
            heights: (0..NUM_MANUAL_ROWS)
                .map(|i| if thick_row(i) { ROW_THICK } else { ROW_THIN })
                .collect(),
        },
        DemoType::ManyHomogeneous => TableRows::Homogeneous {
            count: num_rows,
            height: TEXT_HEIGHT,
        },
        DemoType::ManyHeterogeneous => TableRows::Heterogeneous {
            heights: (0..num_rows)
                .map(|i| if thick_row(i) { ROW_THICK } else { ROW_THIN })
                .collect(),
        },
    }
}

// ── Shared state ────────────────────────────────────────────────────────────

struct DemoState {
    striped: Rc<Cell<bool>>,
    overline: Rc<Cell<bool>>,
    resizable: Rc<Cell<bool>>,
    clickable: Rc<Cell<bool>>,
    demo_idx: Rc<Cell<usize>>,
    num_rows: Rc<Cell<f64>>,
    scroll_to_row_input: Rc<Cell<f64>>,
    scroll_to_row_pending: Rc<Cell<Option<usize>>>,
    selection: Rc<RefCell<HashSet<usize>>>,
    checked: Rc<Cell<bool>>,
    reversed: Rc<Cell<bool>>,
    /// Mirror of the table's column-override cell, so Reset can clear it.
    column_overrides: Rc<RefCell<Vec<Option<f64>>>>,
}

impl DemoState {
    fn defaults() -> Rc<Self> {
        Rc::new(DemoState {
            striped: Rc::new(Cell::new(true)),
            overline: Rc::new(Cell::new(true)),
            resizable: Rc::new(Cell::new(true)),
            clickable: Rc::new(Cell::new(true)),
            demo_idx: Rc::new(Cell::new(0)),
            num_rows: Rc::new(Cell::new(10_000.0)),
            scroll_to_row_input: Rc::new(Cell::new(0.0)),
            scroll_to_row_pending: Rc::new(Cell::new(None)),
            selection: Rc::new(RefCell::new(HashSet::new())),
            checked: Rc::new(Cell::new(false)),
            reversed: Rc::new(Cell::new(false)),
            column_overrides: Rc::new(RefCell::new(Vec::new())),
        })
    }

    fn current_demo(&self) -> DemoType {
        DemoType::from_idx(self.demo_idx.get())
    }
}

// ── Top controls ────────────────────────────────────────────────────────────

fn label_row(font: Arc<Font>, text: &str, control: Box<dyn Widget>) -> FlexRow {
    let lbl = Label::new(text, font)
        .with_font_size(12.0)
        .with_min_size(Size::new(110.0, 0.0))
        .with_max_size(Size::new(110.0, f64::MAX));
    FlexRow::new()
        .with_gap(8.0)
        .add(Box::new(lbl))
        .add_flex(control, 1.0)
}

fn build_controls(font: Arc<Font>, st: Rc<DemoState>) -> Box<dyn Widget> {
    let f = font;

    let mut top = FlexRow::new().with_gap(12.0);
    top = top.add(Box::new(
        Checkbox::new("Striped", Arc::clone(&f), st.striped.get())
            .with_state_cell(Rc::clone(&st.striped)),
    ));
    top = top.add(Box::new(
        Checkbox::new("Overline some rows", Arc::clone(&f), st.overline.get())
            .with_state_cell(Rc::clone(&st.overline)),
    ));
    top = top.add(Box::new(
        Checkbox::new("Resizable columns", Arc::clone(&f), st.resizable.get())
            .with_state_cell(Rc::clone(&st.resizable)),
    ));
    top = top.add(Box::new(
        Checkbox::new("Clickable rows", Arc::clone(&f), st.clickable.get())
            .with_state_cell(Rc::clone(&st.clickable)),
    ));

    let radios = RadioGroup::new(
        vec![
            "Few, manual rows",
            "Thousands of rows of same height",
            "Thousands of rows of differing heights",
        ],
        st.demo_idx.get(),
        Arc::clone(&f),
    )
    .with_selected_cell(Rc::clone(&st.demo_idx));

    let num_rows_slider = {
        let cell = Rc::clone(&st.num_rows);
        Slider::new(cell.get(), 0.0, 100_000.0, Arc::clone(&f))
            .with_step(1.0)
            .with_decimals(0)
            .with_value_cell(cell)
    };

    let scroll_to_slider = {
        let cell = Rc::clone(&st.scroll_to_row_input);
        let pending = Rc::clone(&st.scroll_to_row_pending);
        Slider::new(0.0, 0.0, 100_000.0, Arc::clone(&f))
            .with_step(1.0)
            .with_decimals(0)
            .with_value_cell(cell)
            .on_change(move |v| {
                pending.set(Some(v as usize));
            })
    };

    let st_for_reset = Rc::clone(&st);
    let reset_btn = Button::new("Reset", Arc::clone(&f)).on_click(move || {
        st_for_reset.selection.borrow_mut().clear();
        st_for_reset.checked.set(false);
        st_for_reset.reversed.set(false);
        st_for_reset.column_overrides.borrow_mut().clear();
    });

    let mut col = FlexColumn::new().with_gap(6.0);
    col = col.add(Box::new(top));
    col = col.add(Box::new(
        Label::new("Table type:", Arc::clone(&f)).with_font_size(12.0),
    ));
    col = col.add(Box::new(radios));

    // Num-rows slider is meaningful only for the Many* row modes — egui
    // hides it in Manual mode for the same reason.  We mirror that by
    // wrapping the row in `Conditional` and toggling visibility from a
    // `VisibilitySync` widget that observes `demo_idx`.
    let num_rows_visible = Rc::new(Cell::new(st.demo_idx.get() != 0));
    let num_rows_row = label_row(Arc::clone(&f), "Num rows", Box::new(num_rows_slider));
    col = col.add(Box::new(Conditional::new(
        Rc::clone(&num_rows_visible),
        Box::new(num_rows_row),
    )));
    col = col.add(Box::new(VisibilitySync {
        bounds: agg_gui::Rect::default(),
        children: Vec::new(),
        flag: num_rows_visible,
        source: Rc::clone(&st.demo_idx),
        predicate: Box::new(|i| i != 0),
        last: Cell::new(None),
    }));
    col = col.add(Box::new(label_row(
        Arc::clone(&f),
        "Row to scroll to",
        Box::new(scroll_to_slider),
    )));
    col = col.add(Box::new(reset_btn));
    col = col.add(Box::new(Separator::horizontal()));
    Box::new(col)
}

// ── Tiny reactive observer used to drive `Conditional` from a usize cell ────
//
// `VisibilitySync` is an invisible widget that runs a one-line predicate
// every layout pass and writes its result to a target visibility cell.
// The library widget that consumes the cell (`Conditional` in our case)
// reads the boolean during its own layout in the same frame, so the UI
// stays in sync with state changes without manual `request_draw` calls.

struct VisibilitySync {
    bounds: agg_gui::Rect,
    children: Vec<Box<dyn Widget>>,
    flag: Rc<Cell<bool>>,
    source: Rc<Cell<usize>>,
    predicate: Box<dyn Fn(usize) -> bool>,
    last: Cell<Option<usize>>,
}

impl Widget for VisibilitySync {
    fn type_name(&self) -> &'static str {
        "VisibilitySync"
    }
    fn bounds(&self) -> agg_gui::Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: agg_gui::Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _a: Size) -> Size {
        let v = self.source.get();
        if self.last.get() != Some(v) {
            self.last.set(Some(v));
            self.flag.set((self.predicate)(v));
            agg_gui::animation::request_draw();
        }
        Size::new(0.0, 0.0)
    }
    fn paint(&mut self, _ctx: &mut dyn agg_gui::DrawCtx) {}
    fn is_visible(&self) -> bool {
        false
    }
    fn on_event(&mut self, _e: &agg_gui::Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Cell + header painters ──────────────────────────────────────────────────

fn make_cell_painter(state: Rc<DemoState>) -> Box<dyn FnMut(&CellInfo, &mut dyn agg_gui::DrawCtx)> {
    Box::new(move |info: &CellInfo, ctx: &mut dyn agg_gui::DrawCtx| {
        let v = info.visuals;
        let demo = state.current_demo();
        let n = match demo {
            DemoType::Manual => NUM_MANUAL_ROWS,
            _ => state.num_rows.get() as usize,
        };
        let display_idx = if state.reversed.get() && n > 0 {
            n - 1 - info.row
        } else {
            info.row
        };
        let row_h = info.rect.height;
        let row_y = info.rect.y;

        ctx.set_font(Arc::clone(info.font));
        ctx.set_font_size(12.0);
        ctx.set_fill_color(v.text_color);
        let baseline = row_y + (row_h - 12.0) * 0.5;

        match info.col {
            0 => {
                let s = display_idx.to_string();
                ctx.fill_text(&s, info.rect.x + CELL_PAD_X, baseline);
            }
            1 => {
                let avail = (info.rect.width - CELL_PAD_X * 2.0).max(0.0);
                let txt = long_text(display_idx);
                let display = agg_gui::widgets::table::clip_text_to_width(ctx, &txt, avail);
                ctx.fill_text(&display, info.rect.x + CELL_PAD_X, baseline);
            }
            2 => {
                ctx.set_stroke_color(v.separator);
                ctx.set_line_width(1.0);
                ctx.begin_path();
                let mid = row_y + row_h * 0.5;
                ctx.move_to(info.rect.x + 4.0, mid);
                ctx.line_to(info.rect.x + info.rect.width - 4.0, mid);
                ctx.stroke();
            }
            3 => {
                let checked = state.checked.get();
                let box_size = 12.0;
                let bx = info.rect.x + CELL_PAD_X;
                let by = row_y + (row_h - box_size) * 0.5;
                ctx.set_fill_color(if checked { v.accent } else { v.widget_bg });
                ctx.begin_path();
                ctx.rounded_rect(bx, by, box_size, box_size, 2.0);
                ctx.fill();
                ctx.set_stroke_color(v.widget_stroke);
                ctx.set_line_width(1.0);
                ctx.begin_path();
                ctx.rounded_rect(bx, by, box_size, box_size, 2.0);
                ctx.stroke();
                if checked {
                    ctx.set_stroke_color(Color::rgb(1.0, 1.0, 1.0));
                    ctx.set_line_width(1.5);
                    ctx.begin_path();
                    ctx.move_to(bx + 2.0, by + box_size * 0.5);
                    ctx.line_to(bx + box_size * 0.4, by + 2.0);
                    ctx.line_to(bx + box_size - 2.0, by + box_size - 2.0);
                    ctx.stroke();
                }
                let label_x = bx + box_size + 4.0;
                let label_max = (info.rect.x + info.rect.width - label_x - CELL_PAD_X).max(0.0);
                ctx.set_fill_color(v.text_color);
                let lbl = agg_gui::widgets::table::clip_text_to_width(ctx, "Click me", label_max);
                ctx.fill_text(&lbl, label_x, baseline);
            }
            4 => {
                let is_thick = !matches!(demo, DemoType::ManyHomogeneous) && thick_row(display_idx);
                let txt = if is_thick {
                    "Extra thick row"
                } else {
                    "Normal row"
                };
                let size = if is_thick { 14.0 } else { 12.0 };
                ctx.set_font_size(size);
                let bl = row_y + (row_h - size) * 0.5;
                ctx.fill_text(txt, info.rect.x + CELL_PAD_X, bl);
                ctx.set_font_size(12.0);
            }
            _ => {}
        }
    })
}

fn make_header_painter(
    state: Rc<DemoState>,
) -> Box<dyn FnMut(&HeaderInfo, &mut dyn agg_gui::DrawCtx)> {
    let labels = [
        "Row",
        "Clipped text",
        "Expanding content",
        "Interaction",
        "Content",
    ];
    Box::new(move |info: &HeaderInfo, ctx: &mut dyn agg_gui::DrawCtx| {
        let v = info.visuals;
        ctx.set_font(Arc::clone(info.font));
        ctx.set_font_size(12.5);
        ctx.set_fill_color(v.text_color);
        let baseline = info.rect.y + (info.rect.height - 12.0) * 0.5;
        let lbl = if info.col < labels.len() {
            labels[info.col]
        } else {
            ""
        };

        if info.col == 0 {
            let arrow_w = 22.0;
            let pad = 4.0;
            let arrow_x = info.rect.x + info.rect.width - arrow_w - pad;
            let arrow_y = info.rect.y + (info.rect.height - 16.0) * 0.5;
            ctx.fill_text(lbl, info.rect.x + CELL_PAD_X, baseline);

            // Sort-toggle button — kept visually neutral on every state.
            // The arrow glyph alone signals direction; we don't carry a
            // "pressed" highlight forward, so the button doesn't look
            // stuck in a focused state after toggling.
            ctx.set_fill_color(v.widget_bg);
            ctx.begin_path();
            ctx.rounded_rect(arrow_x, arrow_y, arrow_w, 16.0, 3.0);
            ctx.fill();
            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(arrow_x, arrow_y, arrow_w, 16.0, 3.0);
            ctx.stroke();
            ctx.set_fill_color(v.text_color);
            // Font Awesome caret-up / caret-down — these glyphs are
            // chained into the demo font as a fallback (see
            // `windows/system_fonts.rs`) so they render reliably here.
            let arrow = if state.reversed.get() {
                "\u{F0D8}"
            } else {
                "\u{F0D7}"
            };
            ctx.set_font_size(12.0);
            let m_w = ctx.measure_text(arrow).map(|m| m.width).unwrap_or(8.0);
            let cx = arrow_x + (arrow_w - m_w) * 0.5;
            ctx.fill_text(arrow, cx, arrow_y + 2.0);
        } else {
            let avail = (info.rect.width - CELL_PAD_X * 2.0).max(0.0);
            let display = agg_gui::widgets::table::clip_text_to_width(ctx, lbl, avail);
            ctx.fill_text(&display, info.rect.x + CELL_PAD_X, baseline);
        }
    })
}

// ── Public entry point ──────────────────────────────────────────────────────

pub fn table_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let state = DemoState::defaults();

    // Build the library Table.
    let cell_painter = make_cell_painter(Rc::clone(&state));
    let header_painter = make_header_painter(Rc::clone(&state));

    // Sort-toggle hook on the "Row" header column.
    let st_for_header = Rc::clone(&state);
    let header_click: agg_gui::widgets::table::HeaderClick = Box::new(move |col, _x, _y| {
        if col == 0 {
            let cur = st_for_header.reversed.get();
            st_for_header.reversed.set(!cur);
            return EventResult::Consumed;
        }
        EventResult::Ignored
    });

    let scroll_to_cell: Rc<Cell<Option<usize>>> = Rc::new(Cell::new(None));

    // Display-index helper: when reversed, "row N" appears in slot
    // (count - 1 - N).  All highlights/overlines should track that.
    // Row count is computed from (demo_idx, num_rows) directly so we
    // don't depend on the table's internal cell update timing.
    let display_idx_of = {
        let demo_idx = Rc::clone(&state.demo_idx);
        let num_rows = Rc::clone(&state.num_rows);
        let reversed = Rc::clone(&state.reversed);
        move |internal: usize| -> usize {
            let n = match DemoType::from_idx(demo_idx.get()) {
                DemoType::Manual => NUM_MANUAL_ROWS,
                _ => num_rows.get() as usize,
            };
            if reversed.get() && n > 0 {
                n.saturating_sub(1).saturating_sub(internal)
            } else {
                internal
            }
        }
    };

    // Live row spec: returns the current TableRows derived from
    // (demo_idx, num_rows) on every layout pass.  Replaces the old
    // observer-widget hack that wasn't reliably picking up slider drags.
    let rows_provider_state = Rc::clone(&state);
    let scroll_to_for_provider = Rc::clone(&scroll_to_cell);
    let rows_provider = Box::new(move || {
        let demo = DemoType::from_idx(rows_provider_state.demo_idx.get());
        let n = rows_provider_state.num_rows.get() as usize;
        // Drain any pending scroll-to-row request alongside, so the
        // table picks it up in the same layout pass.
        if let Some(target) = rows_provider_state.scroll_to_row_pending.take() {
            scroll_to_for_provider.set(Some(target));
        }
        build_rows(demo, n)
    });

    let table: Table = TableBuilder::new()
        .columns(vec![
            // All five columns are made resizable so users can grab any
            // edge — egui's demo behaves the same.
            TableColumn::auto(56.0).resizable(true),
            TableColumn::remainder()
                .at_least(40.0)
                .clip(true)
                .resizable(true),
            TableColumn::auto(72.0).resizable(true),
            TableColumn::remainder().resizable(true),
            TableColumn::remainder().resizable(true),
        ])
        .striped_cell(Rc::clone(&state.striped))
        .sense_click_cell(Rc::clone(&state.clickable))
        .resizable_cell(Rc::clone(&state.resizable))
        .column_overrides_cell(Rc::clone(&state.column_overrides))
        .rows_provider(rows_provider)
        // Match the panel we sit on so the scrollbar fade dissolves
        // invisibly into the demo's panel background instead of
        // painting a bright window-fill halo.
        .fade_color(agg_gui::current_visuals().panel_fill)
        .scroll_to_row_cell(Rc::clone(&scroll_to_cell))
        .selection_pred({
            let selection = Rc::clone(&state.selection);
            let display_idx_of = display_idx_of.clone();
            Box::new(move |i| selection.borrow().contains(&display_idx_of(i)))
        })
        .overline_pred({
            let overline = Rc::clone(&state.overline);
            let display_idx_of = display_idx_of.clone();
            Box::new(move |i: usize| overline.get() && display_idx_of(i) % 7 == 3)
        })
        .on_row_click({
            let selection = Rc::clone(&state.selection);
            let checked = Rc::clone(&state.checked);
            let display_idx_of = display_idx_of.clone();
            Box::new(move |i, col| {
                // Column 3 hosts the shared "Click me" checkbox — clicking
                // anywhere inside that column toggles `checked` (matching
                // egui where the checkbox is a real interactive widget),
                // and does NOT also flip selection.  All other columns
                // toggle the row's selection.
                if col == 3 {
                    checked.set(!checked.get());
                    return;
                }
                let idx = display_idx_of(i);
                let mut sel = selection.borrow_mut();
                if sel.contains(&idx) {
                    sel.remove(&idx);
                } else {
                    sel.insert(idx);
                }
            })
        })
        .header_painter(header_painter)
        .header_click(header_click)
        .cell_painter(cell_painter)
        .build(Arc::clone(&font));

    let controls = build_controls(Arc::clone(&font), Rc::clone(&state));

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg();
    col = col.add(controls);
    col.push(Box::new(SizedBox::new().with_height(2.0)), 0.0);
    col.push(Box::new(table), 1.0);
    Box::new(col)
}
