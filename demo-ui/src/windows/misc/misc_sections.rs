//! Section builders for the Misc Demos window that exercise the small widget
//! features (checkbox indeterminate, password reveal, DragValue suffix,
//! horizontal RadioGroup) plus egui's "Misc" custom-paint and colors sections.
//!
//! Split out of `misc_demos.rs` so that file stays under the project's
//! 800-line cap. The orchestrator in `misc_demos.rs` wires these into
//! `CollapsingHeader`s.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, Color, ColorWheelPicker, Conditional, DragValue, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Label, RadioGroup, Rebuilder, Rect, Size, SizedBox, Widget,
};

/// Font Awesome eye / eye-slash glyphs used by the password reveal toggle.
const FA_EYE: &str = "\u{f06e}";
const FA_EYE_SLASH: &str = "\u{f070}";

/// A color swatch + name row used by the Colors section.
/// The swatch rectangle is painted directly; the name renders through a real
/// `Label` child so its glyph cache stays warm across frames.
pub struct SwatchRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    color: Color,
}

impl SwatchRow {
    pub fn new(name: &str, color: Color, font: &Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(
                Label::new(name, Arc::clone(font)).with_font_size(11.5),
            )],
            color,
        }
    }
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
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// egui's "paint your own small icons" example: a 16 px circle bisected by
/// three radial strokes, painted directly with path commands.
struct IconPaint {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl IconPaint {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
}

impl Widget for IconPaint {
    fn type_name(&self) -> &'static str {
        "IconPaint"
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

    fn layout(&mut self, _available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, 16.0, 16.0);
        Size::new(16.0, 16.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        use std::f64::consts::TAU;
        let cx = 8.0;
        let cy = 8.0;
        let r = 16.0 / 2.0 - 1.0;
        let color = Color::rgb(0.5, 0.5, 0.5);
        ctx.set_stroke_color(color);
        ctx.set_line_width(1.0);
        // Outer circle.
        ctx.begin_path();
        ctx.circle(cx, cy, r);
        ctx.stroke();
        // Vertical diameter and two radial strokes at TAU/8 and 3·TAU/8.
        let mut segment = |ax: f64, ay: f64, bx: f64, by: f64| {
            ctx.begin_path();
            ctx.move_to(ax, ay);
            ctx.line_to(bx, by);
            ctx.stroke();
        };
        segment(cx, cy - r, cx, cy + r);
        let a1 = TAU / 8.0;
        segment(cx, cy, cx + r * a1.cos(), cy + r * a1.sin());
        let a2 = 3.0 * TAU / 8.0;
        segment(cx, cy, cx + r * a2.cos(), cy + r * a2.sin());
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Misc widgets section: the egui angle drag (degrees with a `°` suffix, live
/// `≈ Nτ` readout) and a password field with an eye reveal toggle.
pub fn misc_widgets_section(font: &Arc<Font>) -> Box<dyn Widget> {
    use std::f64::consts::TAU;
    let mut col = FlexColumn::new().with_gap(6.0);

    // Angle is stored in radians but presented in degrees (egui `drag_angle`).
    let angle_cell = Rc::new(Cell::new(TAU / 3.0)); // 120°
    {
        let ac = Rc::clone(&angle_cell);
        let drag = DragValue::new(angle_cell.get().to_degrees(), -360.0, 360.0, Arc::clone(font))
            .with_speed(1.0)
            .with_decimals(0)
            .with_suffix("°")
            .on_change(move |deg| ac.set(deg.to_radians()));

        // Live "≈ Nτ" readout rebuilt whenever the angle changes.
        let readout = {
            let a = Rc::clone(&angle_cell);
            let f = Arc::clone(font);
            let version = {
                let a = Rc::clone(&angle_cell);
                move || (a.get() * 1000.0) as i64 as u64
            };
            Rebuilder::new(version, move || {
                Box::new(
                    Label::new(format!("≈ {:.3}τ", a.get() / TAU), Arc::clone(&f))
                        .with_font_size(12.5),
                ) as Box<dyn Widget>
            })
        };

        let angle_row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                Label::new("An angle:", Arc::clone(font)).with_font_size(12.5),
            ))
            .add(Box::new(
                SizedBox::new()
                    .with_height(28.0)
                    .with_width(80.0)
                    .with_child(Box::new(drag)),
            ))
            .add(Box::new(readout));
        col.push(Box::new(angle_row), 0.0);
    }

    // Password field with an eye reveal toggle driven by a shared cell.
    {
        let reveal = Rc::new(Cell::new(false));
        let field = agg_gui::TextField::new(Arc::clone(font))
            .with_font_size(12.5)
            .with_placeholder("hunter2")
            .with_password_mode(true)
            .with_password_reveal_cell(Rc::clone(&reveal));

        let eye = {
            let reveal = Rc::clone(&reveal);
            let f = Arc::clone(font);
            let version = {
                let reveal = Rc::clone(&reveal);
                move || reveal.get() as u64
            };
            Rebuilder::new(version, move || {
                let reveal = Rc::clone(&reveal);
                let glyph = if reveal.get() { FA_EYE_SLASH } else { FA_EYE };
                Box::new(
                    Button::new(glyph, Arc::clone(&f))
                        .with_font_size(13.0)
                        .on_click(move || reveal.set(!reveal.get())),
                ) as Box<dyn Widget>
            })
        };

        let pw_row = FlexRow::new()
            .with_gap(8.0)
            .add(Box::new(
                Label::new("Password:", Arc::clone(font)).with_font_size(12.5),
            ))
            .add_flex(
                Box::new(
                    SizedBox::new()
                        .with_height(28.0)
                        .with_child(Box::new(field)),
                ),
                1.0,
            )
            .add(Box::new(
                SizedBox::new()
                    .with_height(28.0)
                    .with_width(32.0)
                    .with_child(Box::new(eye)),
            ));
        col.push(Box::new(pw_row), 0.0);
    }

    Box::new(col)
}

/// Checkboxes section: 64 tiny empty checkboxes, a wrapped 64-radio row, and a
/// tri-state "Check/uncheck all" control over a 3-item checklist.
pub fn checkboxes_section(font: &Arc<Font>) -> Box<dyn Widget> {
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

    // 64 empty checkboxes all bound to one bool (egui shares `dummy_bool`).
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

    {
        let cell = Rc::clone(&shared_bool);
        col.push(
            Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
                Checkbox::new("checkbox", Arc::clone(font), shared_bool.get())
                    .with_font_size(12.5)
                    .with_state_cell(Rc::clone(&shared_bool))
                    .on_change(move |v| cell.set(v)),
            ))),
            0.0,
        );
    }

    col.push(
        Box::new(Label::new("Radiobuttons are similar:", Arc::clone(font)).with_font_size(11.5)),
        0.0,
    );

    // 64 empty radios + a labelled "radio_value" — one mutually-exclusive group
    // flowed with the new horizontal-wrap RadioGroup.
    {
        let radio_sel = Rc::new(Cell::new(0_usize));
        let mut opts: Vec<String> = (0..64).map(|_| String::new()).collect();
        opts.push("radio_value".to_string());
        let rs = Rc::clone(&radio_sel);
        col.push(
            Box::new(
                RadioGroup::new(opts, 0, Arc::clone(font))
                    .with_horizontal_wrap(true)
                    .with_font_size(12.5)
                    .with_selected_cell(Rc::clone(&radio_sel))
                    .on_change(move |i| rs.set(i)),
            ),
            0.0,
        );
    }

    col.push(
        Box::new(
            Label::new(
                "Checkboxes can be in an indeterminate state:",
                Arc::clone(font),
            )
            .with_font_size(11.5),
        ),
        0.0,
    );

    // Tri-state "Check/uncheck all" over a 3-item checklist (egui parity).
    let items: Vec<Rc<Cell<bool>>> = (0..3).map(|i| Rc::new(Cell::new(i == 0))).collect();
    let all_cell = Rc::new(Cell::new(items.iter().all(|c| c.get())));
    {
        let items_ind = items.clone();
        let items_set = items.clone();
        col.push(
            Box::new(SizedBox::new().with_height(24.0).with_child(Box::new(
                Checkbox::new("Check/uncheck all", Arc::clone(font), all_cell.get())
                    .with_font_size(12.5)
                    .with_state_cell(Rc::clone(&all_cell))
                    .with_indeterminate_fn(move || {
                        let any = items_ind.iter().any(|c| c.get());
                        let all = items_ind.iter().all(|c| c.get());
                        any && !all
                    })
                    .on_change(move |v| {
                        for c in &items_set {
                            c.set(v);
                        }
                    }),
            ))),
            0.0,
        );
    }
    for (i, item) in items.iter().enumerate() {
        let item_c = Rc::clone(item);
        let items_snapshot = items.clone();
        let all_c = Rc::clone(&all_cell);
        col.push(
            Box::new(SizedBox::new().with_height(24.0).with_child(Box::new(
                Checkbox::new(format!("Item {}", i + 1), Arc::clone(font), item.get())
                    .with_font_size(12.5)
                    .with_state_cell(Rc::clone(&item_c))
                    .on_change(move |_v| {
                        all_c.set(items_snapshot.iter().all(|c| c.get()));
                    }),
            ))),
            0.0,
        );
    }

    Box::new(col)
}

/// Colors section: one editable sRGBA picker (via `ColorWheelPicker`) plus the
/// named-swatch rows. egui shows four premul/unmul u8/f32 variants; the library
/// picker only round-trips a single sRGBA colour cheaply, so we show one
/// editable picker and a note, then the static named swatches.
pub fn colors_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(2.0);

    col.push(
        Box::new(
            Label::new(
                "Click the swatch to edit this sRGBA color:",
                Arc::clone(font),
            )
            .with_font_size(11.5),
        ),
        0.0,
    );
    // The picker self-sizes (hue ring + SV triangle + alpha + preview), so it
    // is pushed directly rather than wrapped in a fixed-height box.
    col.push(
        Box::new(
            ColorWheelPicker::new(Color::rgb(0.22, 0.45, 0.88), Arc::clone(font))
                .with_show_alpha(true)
                .with_font_size(12.0),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "(Premultiplied and unmultiplied u8 & f32 variants also exist.)",
                Arc::clone(font),
            )
            .with_font_size(10.5),
        ),
        0.0,
    );

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
        col.push(Box::new(SwatchRow::new(name, color, font)), 0.0);
    }

    Box::new(col)
}

/// Custom Collapsing Header section.
///
/// Our `CollapsingHeader` doesn't support embedding a control in its header
/// row, so we build the honest closest version: a `Checkbox` toggle in the
/// header row wired to a `Conditional` body, which expands/collapses exactly
/// like egui's custom collapsing header with a header-embedded control.
pub fn custom_collapsing_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(4.0);

    let open = Rc::new(Cell::new(true));
    {
        let toggle_cell = Rc::clone(&open);
        col.push(
            Box::new(SizedBox::new().with_height(24.0).with_child(Box::new(
                Checkbox::new("Show body (toggle in header row)", Arc::clone(font), open.get())
                    .with_font_size(12.5)
                    .with_state_cell(Rc::clone(&toggle_cell))
                    .on_change(move |v| toggle_cell.set(v)),
            ))),
            0.0,
        );
    }

    let mut body = FlexColumn::new().with_gap(4.0).with_padding(8.0);
    body.push(
        Box::new(
            Label::new(
                "The checkbox above is embedded in the header row and toggles this body.",
                Arc::clone(font),
            )
            .with_font_size(11.5),
        ),
        0.0,
    );
    col.push(
        Box::new(Conditional::new(Rc::clone(&open), Box::new(body))),
        0.0,
    );

    Box::new(col)
}

/// "Misc" paint-your-own-icon section (egui's small custom-paint example).
pub fn paint_icon_section(font: &Arc<Font>) -> Box<dyn Widget> {
    let row = FlexRow::new()
        .with_gap(8.0)
        .add(Box::new(
            Label::new(
                "You can pretty easily paint your own small icons:",
                Arc::clone(font),
            )
            .with_font_size(12.0),
        ))
        .add(Box::new(
            SizedBox::new()
                .with_width(16.0)
                .with_height(16.0)
                .with_child(Box::new(IconPaint::new())),
        ));
    Box::new(row)
}
