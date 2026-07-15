use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Button, Checkbox, CollapsingHeader, Color, DragValue, DrawCtx, Event, EventResult, FlexColumn,
    FlexRow, Font, Label, RadioGroup, Rebuilder, Rect, Resize, ScrollView, Size, SizedBox, Slider,
    Widget,
};

use super::tree_section::tree_section;

/// A color swatch + name row used by the Colors section of misc_demos.
/// The swatch rectangle is painted directly; the name renders through a
/// real `Label` child so its glyph cache stays warm across frames.
struct SwatchRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    color: Color,
}

impl Widget for SwatchRow {
    fn type_name(&self) -> &'static str {
        "SwatchRow"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, 22.0);
        if let Some(child) = self.children.first_mut() {
            let s = child.layout(Size::new(available.width - 30.0, 22.0));
            child.set_bounds(Rect::new(28.0, (22.0 - s.height) * 0.5, s.width, s.height));
        }
        Size::new(available.width, 22.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 3.0, 20.0, 16.0, 3.0);
        ctx.fill();
        if let Some(child) = self.children.first_mut() {
            child.set_label_color(v.text_color);
        }
        // Label child paints itself via the framework's tree walk.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Box painting widget — draws N boxes whose visual properties are controlled
/// by shared cells (sliders set them externally).
struct BoxPainter {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    box_w: Rc<Cell<f64>>,
    box_h: Rc<Cell<f64>>,
    corner_radius: Rc<Cell<f64>>,
    stroke_width: Rc<Cell<f64>>,
    num_boxes: Rc<Cell<f64>>,
}

impl Widget for BoxPainter {
    fn type_name(&self) -> &'static str {
        "BoxPainter"
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
        // Height grows with the box height and wrap count so the boxes are
        // never clipped by the section (egui uses `horizontal_wrapped`).
        let bw = self.box_w.get().max(1.0);
        let bh = self.box_h.get().max(1.0);
        let n = self.num_boxes.get() as usize;
        let gap = 8.0_f64;
        let per_row = ((available.width + gap) / (bw + gap)).floor().max(1.0) as usize;
        let rows = if n == 0 {
            0
        } else {
            n.div_ceil(per_row.max(1))
        };
        let h = (rows as f64 * (bh + gap) + gap).max(8.0);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let cr = self.corner_radius.get();
        let sw = self.stroke_width.get();
        let n = self.num_boxes.get() as usize;
        let bw = self.box_w.get().max(1.0);
        let bh = self.box_h.get().max(1.0);
        let gap = 8.0_f64;
        let per_row = ((self.bounds.width + gap) / (bw + gap)).floor().max(1.0) as usize;
        for i in 0..n {
            let col = i % per_row;
            let row = i / per_row;
            let x = col as f64 * (bw + gap);
            // Y-up: first row sits at the top of the widget.
            let y = self.bounds.height - gap - (row as f64 + 1.0) * bh - row as f64 * gap;
            ctx.set_fill_color(Color::rgba(
                v.text_color.r,
                v.text_color.g,
                v.text_color.b,
                0.35,
            ));
            ctx.begin_path();
            ctx.rounded_rect(x, y, bw, bh, cr);
            ctx.fill();
            if sw > 0.0 {
                ctx.set_stroke_color(v.text_color);
                ctx.set_line_width(sw);
                ctx.begin_path();
                ctx.rounded_rect(x, y, bw, bh, cr);
                ctx.stroke();
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Stress-test circles widget — draws 100 circles of increasing radius.
struct ManyCirclesWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for ManyCirclesWidget {
    fn type_name(&self) -> &'static str {
        "ManyCirclesWidget"
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
        // Lay out 100 circles of radius 0..10 in wrapping rows.
        let cols = 20_usize;
        let cell = 18.0_f64;
        let rows = (100 + cols - 1) / cols;
        let h = rows as f64 * cell + 4.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let cols = 20_usize;
        let cell = 18.0_f64;
        let h = self.bounds.height;
        for i in 0..100_usize {
            let r = i as f64 * 0.5 + 0.5;
            let col = i % cols;
            let row = i / cols;
            let cx = col as f64 * cell + cell * 0.5;
            // Y-up: row 0 is at top = highest y
            let rows = (100 + cols - 1) / cols;
            let cy = h - row as f64 * cell - cell * 0.5;
            let _ = rows;
            ctx.set_fill_color(v.text_color);
            ctx.begin_path();
            ctx.circle(cx, cy, r.min(cell * 0.45));
            ctx.fill();
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build a FlexColumn section content for the Label section of Misc Demos.
fn label_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(4.0);

    let color_row = FlexRow::new()
        .with_gap(6.0)
        .add(Box::new(
            Label::new("Text can have", Arc::clone(font)).with_font_size(12.0),
        ))
        .add(Box::new(
            Label::new("color,", Arc::clone(font))
                .with_font_size(12.0)
                .with_color(Color::rgb(0.43, 1.0, 0.43)),
        ))
        .add(Box::new(
            Label::new("size,", Arc::clone(font))
                .with_font_size(12.0)
                .with_color(Color::rgb(0.50, 0.55, 1.0)),
        ))
        .add(Box::new(
            Label::new("and style.", Arc::clone(font))
                .with_font_size(12.0)
                .with_color(Color::rgb(1.0, 0.75, 0.40)),
        ));
    col.push(Box::new(color_row), 0.0);

    col.push(Box::new(Label::new(
        "The default font supports latin, cyrillic (ИÅđ…), math (∫√∞²⅓…), and emojis (💓🌟🖩…).",
        Arc::clone(font),
    ).with_font_size(12.0)), 0.0);

    Box::new(col)
}

/// Build the Misc widgets section content.
fn misc_widgets_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(6.0);

    let angle_cell = Rc::new(Cell::new(2.094_f64));
    {
        let ac = Rc::clone(&angle_cell);
        let angle_row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                Label::new("An angle:", Arc::clone(font)).with_font_size(12.5),
            ))
            .add(Box::new(
                SizedBox::new()
                    .with_height(28.0)
                    .with_width(80.0)
                    .with_child(Box::new(
                        DragValue::new(angle_cell.get(), -6.283, 6.283, Arc::clone(font))
                            .with_speed(0.02)
                            .with_decimals(2)
                            .on_change(move |v| ac.set(v)),
                    )),
            ));
        col.push(Box::new(angle_row), 0.0);
    }

    let pw_row = FlexRow::new()
        .with_gap(8.0)
        .add(Box::new(
            Label::new("Password:", Arc::clone(font)).with_font_size(12.5),
        ))
        .add_flex(
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    agg_gui::TextField::new(Arc::clone(font))
                        .with_font_size(12.5)
                        .with_placeholder("hunter2")
                        .with_password_mode(true),
                )),
            ),
            1.0,
        );
    col.push(Box::new(pw_row), 0.0);

    Box::new(col)
}

/// Build the Checkboxes section content.
fn checkboxes_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(4.0);

    col.push(
        Box::new(
            Label::new(
                "Checkboxes with empty labels take up very little space:",
                Arc::clone(font),
            )
            .with_font_size(11.5),
        ),
        0.0,
    );

    let shared_bool = Rc::new(Cell::new(false));
    for _row in 0..4 {
        let mut cb_row = FlexRow::new().with_gap(2.0);
        for _c in 0..16 {
            let cell = Rc::clone(&shared_bool);
            cb_row.push(
                Box::new(
                    SizedBox::new()
                        .with_height(22.0)
                        .with_width(22.0)
                        .with_child(Box::new(
                            Checkbox::new("", Arc::clone(font), cell.get())
                                .with_font_size(11.0)
                                .with_state_cell(Rc::clone(&cell))
                                .on_change(move |v| cell.set(v)),
                        )),
                ),
                0.0,
            );
        }
        col.push(Box::new(cb_row), 0.0);
    }

    col.push(
        Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            Checkbox::new("checkbox", Arc::clone(font), false).with_font_size(12.5),
        ))),
        0.0,
    );

    col.push(
        Box::new(Label::new("Radio buttons:", Arc::clone(font)).with_font_size(11.5)),
        0.0,
    );

    let radio_sel = Rc::new(Cell::new(0_usize));
    {
        let rs = Rc::clone(&radio_sel);
        col.push(
            Box::new(
                RadioGroup::new(
                    vec!["Option A", "Option B", "Option C"],
                    radio_sel.get(),
                    Arc::clone(font),
                )
                .with_font_size(12.5)
                .on_change(move |i| rs.set(i)),
            ),
            0.0,
        );
    }

    Box::new(col)
}

/// Build the Colors section content.
fn colors_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(2.0);

    let named_colors: &[(&str, Color)] = &[
        ("Red", Color::rgb(0.88, 0.25, 0.18)),
        ("Orange", Color::rgb(0.92, 0.55, 0.15)),
        ("Yellow", Color::rgb(0.92, 0.85, 0.15)),
        ("Green", Color::rgb(0.25, 0.78, 0.30)),
        ("Cyan", Color::rgb(0.22, 0.65, 0.88)),
        ("Blue", Color::rgb(0.22, 0.45, 0.88)),
        ("Purple", Color::rgb(0.60, 0.25, 0.88)),
        ("Pink", Color::rgb(0.88, 0.25, 0.65)),
    ];
    for &(name, color) in named_colors {
        col.push(
            Box::new(SwatchRow {
                bounds: Rect::default(),
                children: vec![Box::new(
                    Label::new(name, Arc::clone(font)).with_font_size(11.5),
                )],
                color,
            }),
            0.0,
        );
    }

    Box::new(col)
}

/// Build the Test box rendering section content.
fn box_rendering_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(4.0);

    let box_w = Rc::new(Cell::new(64.0_f64));
    let box_h = Rc::new(Cell::new(32.0_f64));
    let corner_r = Rc::new(Cell::new(5.0_f64));
    let stroke_w = Rc::new(Cell::new(2.0_f64));
    let num_boxes = Rc::new(Cell::new(1.0_f64));

    {
        let bw = Rc::clone(&box_w);
        col.push(
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    Slider::new(box_w.get(), 0.0, 500.0, Arc::clone(font))
                        .with_step(1.0)
                        .on_change(move |v| bw.set(v)),
                )),
            ),
            0.0,
        );
        col.push(
            Box::new(Label::new("width", Arc::clone(font)).with_font_size(10.5)),
            0.0,
        );
    }
    {
        let bh = Rc::clone(&box_h);
        col.push(
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    Slider::new(box_h.get(), 0.0, 500.0, Arc::clone(font))
                        .with_step(1.0)
                        .on_change(move |v| bh.set(v)),
                )),
            ),
            0.0,
        );
        col.push(
            Box::new(Label::new("height", Arc::clone(font)).with_font_size(10.5)),
            0.0,
        );
    }
    {
        let cr = Rc::clone(&corner_r);
        col.push(
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    Slider::new(corner_r.get(), 0.0, 50.0, Arc::clone(font))
                        .with_step(0.5)
                        .on_change(move |v| cr.set(v)),
                )),
            ),
            0.0,
        );
        col.push(
            Box::new(Label::new("corner radius", Arc::clone(font)).with_font_size(10.5)),
            0.0,
        );
    }
    {
        let sw = Rc::clone(&stroke_w);
        col.push(
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    Slider::new(stroke_w.get(), 0.0, 10.0, Arc::clone(font))
                        .with_step(0.5)
                        .on_change(move |v| sw.set(v)),
                )),
            ),
            0.0,
        );
        col.push(
            Box::new(Label::new("stroke width", Arc::clone(font)).with_font_size(10.5)),
            0.0,
        );
    }
    {
        let nb = Rc::clone(&num_boxes);
        col.push(
            Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    Slider::new(num_boxes.get(), 0.0, 8.0, Arc::clone(font))
                        .with_step(1.0)
                        .on_change(move |v| nb.set(v)),
                )),
            ),
            0.0,
        );
        col.push(
            Box::new(Label::new("number of boxes", Arc::clone(font)).with_font_size(10.5)),
            0.0,
        );
    }

    col.push(
        Box::new(BoxPainter {
            bounds: Rect::default(),
            children: Vec::new(),
            box_w: Rc::clone(&box_w),
            box_h: Rc::clone(&box_h),
            corner_radius: Rc::clone(&corner_r),
            stroke_width: Rc::clone(&stroke_w),
            num_boxes: Rc::clone(&num_boxes),
        }),
        0.0,
    );

    Box::new(col)
}

/// Build the Columns section content — a slider (1..=10) driving a row of that
/// many equal-flex columns, mirroring egui's `ui.columns`.
fn columns_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(6.0);

    let num_columns = Rc::new(Cell::new(2.0_f64));

    {
        let row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                Label::new("Columns", Arc::clone(font)).with_font_size(12.0),
            ))
            .add_flex(
                Box::new(
                    SizedBox::new().with_height(28.0).with_child(Box::new(
                        Slider::new(num_columns.get(), 1.0, 10.0, Arc::clone(font))
                            .with_step(1.0)
                            .with_decimals(0)
                            .with_value_cell(Rc::clone(&num_columns)),
                    )),
                ),
                1.0,
            );
        col.push(Box::new(row), 0.0);
    }

    let version = {
        let num_columns = Rc::clone(&num_columns);
        move || num_columns.get() as u64
    };
    let builder = {
        let font = Arc::clone(font);
        let num_columns = Rc::clone(&num_columns);
        move || build_columns(&font, &num_columns)
    };
    col.push(Box::new(Rebuilder::new(version, builder)), 0.0);

    Box::new(col)
}

/// Build a row of `n` equal-flex columns; the last one carries a "Delete this"
/// button that decrements the count (egui parity).
fn build_columns(font: &Arc<Font>, num_columns: &Rc<Cell<f64>>) -> Box<dyn Widget> {
    let n = (num_columns.get() as usize).clamp(1, 10);
    let mut row = FlexRow::new().with_gap(8.0);
    for i in 0..n {
        let mut column = FlexColumn::new().with_gap(4.0).add(Box::new(
            Label::new(
                format!("Column {} out of {}", i + 1, n),
                Arc::clone(font),
            )
            .with_font_size(11.5)
            .with_wrap(true),
        ));
        if i + 1 == n {
            let num_columns = Rc::clone(num_columns);
            column = column.add(Box::new(
                SizedBox::new().with_height(26.0).with_child(Box::new(
                    Button::new("Delete this", Arc::clone(font))
                        .with_font_size(11.0)
                        .on_click(move || {
                            let next = (num_columns.get() - 1.0).max(1.0);
                            num_columns.set(next);
                        }),
                )),
            ));
        }
        row = row.add_flex(Box::new(column), 1.0);
    }
    Box::new(row)
}

/// Build the Resize section content — a user-draggable `Resize` area with the
/// same explanatory text egui shows (`misc_demo_window.rs` "Resize" header).
/// Pull the SE handle to resize the region; egui's default height is 100.
fn resize_section(font: &Arc<Font>) -> Box<dyn Widget> {
    // `top_anchor` keeps the labels pinned to the top of the frame when the
    // user drags the region taller, matching egui's top-down layout.
    let mut inner = FlexColumn::new()
        .with_gap(4.0)
        .with_padding(8.0)
        .with_top_anchor(true);
    inner.push(
        Box::new(Label::new("This ui can be resized!", Arc::clone(font)).with_font_size(12.0)),
        0.0,
    );
    inner.push(
        Box::new(
            Label::new("Just pull the handle on the bottom right", Arc::clone(font))
                .with_font_size(12.0),
        ),
        0.0,
    );
    Box::new(
        Resize::new(Box::new(inner))
            .with_default_size(Size::new(250.0, 100.0))
            .with_min_size_hint(Size::new(80.0, 40.0))
            .with_max_size_hint(Size::new(4000.0, 3000.0)),
    )
}

/// Build the Misc Demos window — ✨ Misc Demos — matching egui's CollapsingHeader layout.
///
/// Each section is a `CollapsingHeader` (click to expand/collapse), matching
/// egui's `MiscDemoWindow` exactly.  "Label" is open by default; all others start closed.
pub fn misc_demos(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(1.0)
        .with_padding(6.0)
        .with_panel_bg();

    // ── Label (default open) ────────────────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Label", Arc::clone(&font))
                .default_open(true)
                .with_content(label_section(&font)),
        ),
        0.0,
    );

    // ── Misc widgets (default closed) ───────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Misc widgets", Arc::clone(&font))
                .default_open(false)
                .with_content(misc_widgets_section(&font)),
        ),
        0.0,
    );

    // ── Colors (default closed) ──────────────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Colors", Arc::clone(&font))
                .default_open(false)
                .with_content(colors_section(&font)),
        ),
        0.0,
    );

    // ── Tree (default closed) ────────────────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Tree", Arc::clone(&font))
                .default_open(false)
                .with_content(tree_section(&font)),
        ),
        0.0,
    );

    // ── Checkboxes (default closed) ─────────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Checkboxes", Arc::clone(&font))
                .default_open(false)
                .with_content(checkboxes_section(&font)),
        ),
        0.0,
    );

    // ── Columns (default closed) ─────────────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Columns", Arc::clone(&font))
                .default_open(false)
                .with_content(columns_section(&font)),
        ),
        0.0,
    );

    // ── Test box rendering (default closed) ─────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Test box rendering", Arc::clone(&font))
                .default_open(false)
                .with_content(box_rendering_section(&font)),
        ),
        0.0,
    );

    // ── Resize (default closed) ──────────────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Resize", Arc::clone(&font))
                .default_open(false)
                .with_content(resize_section(&font)),
        ),
        0.0,
    );

    // ── Many circles (default closed) ────────────────────────────────────────
    col.push(
        Box::new(
            CollapsingHeader::new("Many circles of different sizes", Arc::clone(&font))
                .default_open(false)
                .with_content(Box::new(ManyCirclesWidget {
                    bounds: Rect::default(),
                    children: Vec::new(),
                })),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}
