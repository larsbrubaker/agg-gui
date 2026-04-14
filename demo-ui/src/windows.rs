//! Demo window content builders.
//!
//! Each function returns a `Box<dyn Widget>` that becomes the content of a
//! floating `Window`.  Real content is implemented for key demos; the rest show
//! a "Coming Soon" placeholder until they are fleshed out in later phases.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Checkbox, Color, DragValue, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, Hyperlink, Label, MarkdownView, ProgressBar, RadioGroup,
    Rect, ScrollView, Separator, Size, SizedBox, Slider, TextField, ToggleSwitch, Widget,
};
use agg_gui::widgets::button::ButtonTheme;

// ---------------------------------------------------------------------------
// "Coming Soon" placeholder
// ---------------------------------------------------------------------------

struct ComingSoon {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
}

impl ComingSoon {
    fn new() -> Self {
        Self { bounds: Rect::default(), children: Vec::new() }
    }
}

impl Widget for ComingSoon {
    fn type_name(&self) -> &'static str { "ComingSoon" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Returns a minimal placeholder window content.
pub fn coming_soon() -> Box<dyn Widget> {
    Box::new(ComingSoon::new())
}

// ---------------------------------------------------------------------------
// Widget Gallery
// ---------------------------------------------------------------------------

pub fn widget_gallery(font: Arc<Font>) -> Box<dyn Widget> {
    let slider_val = Rc::new(Cell::new(0.42_f64));
    let cb1        = Rc::new(Cell::new(true));
    let cb2        = Rc::new(Cell::new(false));
    let radio_sel  = Rc::new(Cell::new(0_usize));

    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.97, 0.97, 0.98));

    col.push(Box::new(Label::new("Buttons", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);

    let row = FlexRow::new().with_gap(8.0)
        .add(Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            Button::new("Primary", Arc::clone(&font)).with_font_size(12.0).on_click(|| {})
        ))))
        .add(Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            Button::new("Secondary", Arc::clone(&font)).with_font_size(12.0)
                .with_theme(ButtonTheme {
                    background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                    background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                    background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                    label_color:        Color::rgb(0.22, 0.45, 0.88),
                    border_radius:      6.0,
                    focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                    focus_ring_width:   2.5,
                }).on_click(|| {})
        ))))
        .add(Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            Button::new("Danger", Arc::clone(&font)).with_font_size(12.0)
                .with_theme(ButtonTheme {
                    background:         Color::rgb(0.88, 0.25, 0.18),
                    background_hovered: Color::rgb(0.95, 0.32, 0.24),
                    background_pressed: Color::rgb(0.72, 0.18, 0.12),
                    label_color:        Color::white(),
                    border_radius:      6.0,
                    focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                    focus_ring_width:   2.5,
                }).on_click(|| {})
        ))));
    col.push(Box::new(row), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Checkboxes", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    { let v = Rc::clone(&cb1);
      col.push(Box::new(Checkbox::new("Enable feature A", Arc::clone(&font), cb1.get())
          .with_font_size(13.0).on_change(move |v2| v.set(v2))), 0.0); }
    { let v = Rc::clone(&cb2);
      col.push(Box::new(Checkbox::new("Enable feature B", Arc::clone(&font), cb2.get())
          .with_font_size(13.0).on_change(move |v2| v.set(v2))), 0.0); }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Slider", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    { let sv = Rc::clone(&slider_val);
      col.push(Box::new(Slider::new(slider_val.get(), 0.0, 1.0, Arc::clone(&font))
          .with_step(0.01).on_change(move |v| sv.set(v))), 0.0); }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Radio", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    { let rs = Rc::clone(&radio_sel);
      col.push(Box::new(RadioGroup::new(
          vec!["Option A", "Option B", "Option C"],
          radio_sel.get(), Arc::clone(&font),
      ).with_font_size(13.0).on_change(move |i| rs.set(i))), 0.0); }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Progress Bar", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    col.push(Box::new(ProgressBar::new(slider_val.get(), Arc::clone(&font))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Toggle Switch", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    {
        let ts1 = Rc::new(Cell::new(true));
        let ts2 = Rc::new(Cell::new(false));
        let row = FlexRow::new().with_gap(16.0)
            .add(Box::new(ToggleSwitch::new(ts1.get()).with_state_cell(Rc::clone(&ts1))))
            .add(Box::new(Label::new("Enabled", Arc::clone(&font)).with_font_size(13.0)))
            .add(Box::new(ToggleSwitch::new(ts2.get()).with_state_cell(Rc::clone(&ts2))))
            .add(Box::new(Label::new("Disabled", Arc::clone(&font)).with_font_size(13.0)));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Drag Value", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    {
        let dv1 = Rc::new(Cell::new(42.0_f64));
        let dv2 = Rc::new(Cell::new(3.14_f64));
        let row = FlexRow::new().with_gap(8.0)
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(28.0).with_child({
                let v = Rc::clone(&dv1);
                Box::new(DragValue::new(dv1.get(), 0.0, 100.0, Arc::clone(&font))
                    .with_decimals(0).on_change(move |x| v.set(x)))
            })))
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(28.0).with_child({
                let v = Rc::clone(&dv2);
                Box::new(DragValue::new(dv2.get(), 0.0, 10.0, Arc::clone(&font))
                    .with_decimals(2).on_change(move |x| v.set(x)))
            })));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Hyperlink", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    col.push(Box::new(
        Hyperlink::new("Visit the agg-gui repository", Arc::clone(&font))
            .with_font_size(13.0)
            .on_click(|| {})
    ), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Text Input", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    col.push(Box::new(SizedBox::new().with_height(32.0).with_child(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(13.0).with_placeholder("Type something…")
    ))), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}

// ---------------------------------------------------------------------------
// Sliders demo
// ---------------------------------------------------------------------------

pub fn sliders(font: Arc<Font>) -> Box<dyn Widget> {
    let v0 = Rc::new(Cell::new(0.5_f64));
    let v1 = Rc::new(Cell::new(25.0_f64));
    let v2 = Rc::new(Cell::new(0.001_f64));
    let v3 = Rc::new(Cell::new(0.75_f64));

    let mut col = FlexColumn::new()
        .with_gap(18.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.97, 0.97, 0.98));

    col.push(Box::new(Label::new("Float  0.0 → 1.0", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    { let sv = Rc::clone(&v0);
      col.push(Box::new(Slider::new(v0.get(), 0.0, 1.0, Arc::clone(&font))
          .with_step(0.01).on_change(move |v| sv.set(v))), 0.0); }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Integer  0 → 100", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    { let sv = Rc::clone(&v1);
      col.push(Box::new(Slider::new(v1.get(), 0.0, 100.0, Arc::clone(&font))
          .with_step(1.0).on_change(move |v| sv.set(v))), 0.0); }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Small step  0.0001 → 0.01", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    { let sv = Rc::clone(&v2);
      col.push(Box::new(Slider::new(v2.get(), 0.0001, 0.01, Arc::clone(&font))
          .with_step(0.0001).on_change(move |v| sv.set(v))), 0.0); }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new("Clamped range  0.25 → 0.75", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    { let sv = Rc::clone(&v3);
      col.push(Box::new(Slider::new(v3.get(), 0.25, 0.75, Arc::clone(&font))
          .with_step(0.005).on_change(move |v| sv.set(v))), 0.0); }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Text Edit demo
// ---------------------------------------------------------------------------

pub fn text_edit(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.97, 0.97, 0.98));

    col.push(Box::new(Label::new("Single-line", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    col.push(Box::new(SizedBox::new().with_height(32.0).with_child(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(13.0).with_placeholder("Click to edit…")
    ))), 0.0);

    col.push(Box::new(Label::new("With initial text", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    col.push(Box::new(SizedBox::new().with_height(32.0).with_child(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(13.0)
            .with_text("Hello, world!")
    ))), 0.0);

    col.push(Box::new(Label::new("Read-only", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);
    col.push(Box::new(SizedBox::new().with_height(32.0).with_child(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(13.0)
            .with_text("This field is read-only")
            .with_read_only(true)
    ))), 0.0);

    col.push(Box::new(Label::new(
        "Ctrl+A select all • Ctrl+C/X/V clipboard • Home/End • Shift+arrows",
        Arc::clone(&font),
    ).with_font_size(11.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.35))), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Code Editor demo
// ---------------------------------------------------------------------------

pub fn code_editor(font: Arc<Font>) -> Box<dyn Widget> {
    const SAMPLE: &str = "\
fn main() {\n\
    let greeting = \"Hello, agg-gui!\";\n\
    println!(\"{}\", greeting);\n\
\n\
    let values: Vec<f64> = (0..10)\n\
        .map(|i| i as f64 * 0.1)\n\
        .collect();\n\
\n\
    for (i, v) in values.iter().enumerate() {\n\
        println!(\"[{i}] {v:.2}\");\n\
    }\n\
}";

    let bg = Color::rgb(0.12, 0.13, 0.15);
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .with_background(bg);

    col.push(Box::new(Label::new("main.rs", Arc::clone(&font))
        .with_font_size(11.0)
        .with_color(Color::rgba(1.0, 1.0, 1.0, 0.45))), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // Render each line as a label — simple but effective without a real editor widget.
    for (i, line) in SAMPLE.lines().enumerate() {
        let line_num = format!("{:>3}  ", i + 1);
        let row = FlexRow::new().with_gap(0.0)
            .add(Box::new(Label::new(line_num, Arc::clone(&font))
                .with_font_size(12.5)
                .with_color(Color::rgba(1.0, 1.0, 1.0, 0.22))))
            .add(Box::new(Label::new(line, Arc::clone(&font))
                .with_font_size(12.5)
                .with_color(Color::rgba(0.85, 0.90, 0.95, 1.0))));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // Editable single-line command bar at the bottom.
    let bar = FlexRow::new().with_gap(8.0)
        .add(Box::new(Label::new(">", Arc::clone(&font))
            .with_font_size(13.0)
            .with_color(Color::rgb(0.4, 0.8, 0.4))))
        .add_flex(Box::new(SizedBox::new().with_height(28.0).with_child(Box::new(
            TextField::new(Arc::clone(&font))
                .with_font_size(13.0)
                .with_placeholder("command…")
        ))), 1.0);
    col.push(Box::new(bar), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}

// ---------------------------------------------------------------------------
// Tooltips demo
// ---------------------------------------------------------------------------

pub fn tooltips(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(14.0)
        .with_padding(16.0)
        .with_background(Color::rgb(0.97, 0.97, 0.98));

    col.push(Box::new(Label::new("Tooltip demos", Arc::clone(&font))
        .with_font_size(12.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.50))), 0.0);

    for label in ["Hover me (A)", "Hover me (B)", "Hover me (C)"] {
        col.push(Box::new(SizedBox::new().with_height(30.0).with_child(Box::new(
            Button::new(label, Arc::clone(&font))
                .with_font_size(13.0)
                .on_click(|| {})
        ))), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Tooltip widget not yet implemented — hover state tracked, \
         overlay rendering planned for Phase 5.",
        Arc::clone(&font),
    ).with_font_size(11.0).with_color(Color::rgba(0.0, 0.0, 0.0, 0.40))), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// 3D Cube window content (wraps a platform-provided GL widget)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// About window
// ---------------------------------------------------------------------------

/// About window content: renders README.md via `MarkdownView` inside a scroll view.
pub fn about(font: Arc<Font>) -> Box<dyn Widget> {
    // Embed README.md at compile time.
    let readme = include_str!("../../README.md");

    let md_view = MarkdownView::new(readme, Arc::clone(&font))
        .with_font_size(13.0)
        .with_padding(12.0);

    Box::new(ScrollView::new(Box::new(md_view)))
}

pub fn cube_content(font: Arc<Font>, cube_widget: Box<dyn Widget>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_background(Color::rgb(0.08, 0.08, 0.12));

    col.push(Box::new(Label::new("GL — rotating cube", Arc::clone(&font))
        .with_font_size(11.0).with_color(Color::rgba(1.0, 1.0, 1.0, 0.55))), 0.0);
    col.push(cube_widget, 1.0);

    Box::new(col)
}
