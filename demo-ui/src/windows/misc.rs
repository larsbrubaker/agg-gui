//! Miscellaneous demo windows: Frame, Extra Viewport, Highlighting,
//! Interactive Container, Font Book, and Misc Demos.
//!
//! These demos showcase layout containers, custom painting, and Unicode glyph
//! display without requiring external state or animation.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Checkbox, Color, Container, DragValue, DrawCtx, Event, EventResult, FlexColumn, FlexRow,
    Font, Label, MouseButton, Point, RadioGroup, Rect, ScrollView, Separator,
    Size, SizedBox, Slider, Widget,
};
use agg_gui::widget::paint_subtree;

// ---------------------------------------------------------------------------
// Frame demo
// ---------------------------------------------------------------------------

/// Build the Frame demo — three `Container` widgets with different border and
/// background combinations placed side by side.
pub fn frame_demo(font: Arc<Font>) -> Box<dyn Widget> {
    let mut outer = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    outer.push(Box::new(Label::new("Container styles", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    // Three boxes side by side.
    let row = FlexRow::new().with_gap(10.0)
        .add(Box::new(
            Container::new()
                .with_background(Color::rgba(0.22, 0.45, 0.88, 0.12))
                .with_border(Color::rgb(0.22, 0.45, 0.88), 1.5)
                .with_corner_radius(6.0)
                .with_padding(10.0)
                .add(Box::new(Label::new("Accent fill\nblue border", Arc::clone(&font))
                    .with_font_size(12.0)))
        ))
        .add(Box::new(
            Container::new()
                .with_background(Color::rgba(0.18, 0.72, 0.42, 0.12))
                .with_border(Color::rgb(0.18, 0.72, 0.42), 1.5)
                .with_corner_radius(6.0)
                .with_padding(10.0)
                .add(Box::new(Label::new("Green fill\ngreen border", Arc::clone(&font))
                    .with_font_size(12.0)))
        ))
        .add(Box::new(
            Container::new()
                .with_background(Color::rgba(0.88, 0.25, 0.18, 0.10))
                .with_border(Color::rgb(0.88, 0.25, 0.18), 1.5)
                .with_corner_radius(6.0)
                .with_padding(10.0)
                .add(Box::new(Label::new("Danger fill\nred border", Arc::clone(&font))
                    .with_font_size(12.0)))
        ));

    outer.push(Box::new(row), 0.0);

    outer.push(Box::new(Separator::horizontal()), 0.0);
    outer.push(Box::new(Label::new(
        "Containers support background color, border color/width, corner radius,\n\
         and inner padding. Children are laid out in a top-down stack.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    outer.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(outer)
}

// ---------------------------------------------------------------------------
// Extra Viewport demo
// ---------------------------------------------------------------------------

/// Build the Extra Viewport demo — informational placeholder.
pub fn extra_viewport(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new(
        "Extra viewports are not supported on this platform.",
        Arc::clone(&font),
    ).with_font_size(13.0)), 0.0);

    Box::new(col)
}

// ---------------------------------------------------------------------------
// Highlighting demo
// ---------------------------------------------------------------------------

/// A widget that draws colored highlight boxes behind individual words.
///
/// This simulates syntax highlighting without a real text-layout engine:
/// each word is measured, a highlight rect is drawn behind it, and then the
/// word is drawn on top.
struct HighlightWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
    /// (word, highlight_color, text_color).
    words:    Vec<(&'static str, Color, Color)>,
}

impl Widget for HighlightWidget {
    fn type_name(&self) -> &'static str { "HighlightWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, 36.0);
        Size::new(available.width, 36.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(14.0);

        let pad   = 4.0;
        let h     = self.bounds.height;
        let mut x = pad;
        let baseline = h * 0.35; // Y-up: baseline in lower portion

        for (word, bg, fg) in &self.words {
            if let Some(m) = ctx.measure_text(word) {
                let word_w = m.width;
                let box_h  = m.ascent - m.descent + 4.0;
                let box_y  = baseline + m.descent - 2.0;

                // Highlight box.
                ctx.set_fill_color(*bg);
                ctx.begin_path();
                ctx.rounded_rect(x - 2.0, box_y, word_w + 4.0, box_h, 3.0);
                ctx.fill();

                // Word text.
                ctx.set_fill_color(*fg);
                ctx.fill_text(word, x, baseline);

                x += word_w + 8.0; // gap between words
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Highlighting demo — several highlighted word spans demonstrating
/// per-glyph color control.
pub fn highlighting(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Colored text segments", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(HighlightWidget {
        bounds:   Rect::default(),
        children: Vec::new(),
        font:     Arc::clone(&font),
        words: vec![
            ("fn",     Color::rgba(0.22, 0.45, 0.88, 0.30), Color::rgb(0.22, 0.45, 0.88)),
            ("main",   Color::rgba(0.86, 0.78, 0.40, 0.30), Color::rgb(0.86, 0.78, 0.40)),
            ("()",     Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.70, 0.70, 0.70)),
            ("{",      Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.90, 0.90, 0.90)),
        ],
    }), 0.0);

    col.push(Box::new(HighlightWidget {
        bounds:   Rect::default(),
        children: Vec::new(),
        font:     Arc::clone(&font),
        words: vec![
            ("let",    Color::rgba(0.22, 0.45, 0.88, 0.30), Color::rgb(0.22, 0.45, 0.88)),
            ("x",      Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.90, 0.90, 0.90)),
            ("=",      Color::rgba(0.90, 0.90, 0.90, 0.10), Color::rgb(0.60, 0.60, 0.60)),
            ("42;",    Color::rgba(0.82, 0.60, 0.45, 0.30), Color::rgb(0.82, 0.60, 0.45)),
        ],
    }), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Each token is measured, a highlight rect is drawn, then the text.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// ---------------------------------------------------------------------------
// Interactive Container demo
// ---------------------------------------------------------------------------

/// A widget that changes its appearance on hover and click.
///
/// Text is rendered through a backbuffered Label child.  Because the click
/// count changes rarely (only on mouse-up), the label cache stays warm most
/// frames and avoids unnecessary glyph rasterization.
struct InteractiveBox {
    bounds:       Rect,
    children:     Vec<Box<dyn Widget>>,
    hovered:      bool,
    pressed:      bool,
    clicks:       u32,
    /// Backbuffered label for the centered text.
    label_widget: Label,
}

impl InteractiveBox {
    fn new(font: Arc<Font>) -> Self {
        Self {
            bounds:       Rect::default(),
            children:     Vec::new(),
            hovered:      false,
            pressed:      false,
            clicks:       0,
            label_widget: Label::new("Click me!", font).with_font_size(13.0),
        }
    }
}

impl Widget for InteractiveBox {
    fn type_name(&self) -> &'static str { "InteractiveBox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width.min(200.0);
        let h = 60.0_f64;
        self.bounds = Rect::new(0.0, 0.0, w, h);

        // Update label text from click count.
        let text = if self.clicks == 0 {
            "Click me!".to_string()
        } else {
            format!("Clicked {} time{}", self.clicks, if self.clicks == 1 { "" } else { "s" })
        };
        self.label_widget.set_text(text);

        // Center the label within the box.
        let ls = self.label_widget.layout(Size::new(w, h));
        let lx = (w - ls.width) * 0.5;
        let ly = (h - ls.height) * 0.5;
        self.label_widget.set_bounds(Rect::new(lx, ly, ls.width, ls.height));

        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        let bg = if self.pressed {
            v.accent_pressed
        } else if self.hovered {
            v.accent_hovered
        } else {
            v.widget_bg
        };

        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 8.0);
        ctx.fill();

        ctx.set_stroke_color(if self.hovered { v.accent } else { v.widget_stroke });
        ctx.set_line_width(if self.hovered { 2.0 } else { 1.0 });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 8.0);
        ctx.stroke();

        // Paint label via backbuffered child.
        let text_color = if self.pressed { Color::white() } else { v.text_color };
        self.label_widget.set_color(text_color);
        let lb = self.label_widget.bounds();
        ctx.save(); ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label_widget, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was_hovered = self.hovered;
                self.hovered = pos.x >= 0.0 && pos.x <= self.bounds.width
                    && pos.y >= 0.0 && pos.y <= self.bounds.height;
                if self.hovered != was_hovered { EventResult::Consumed } else { EventResult::Ignored }
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                if self.hovered {
                    self.pressed = true;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp { button: MouseButton::Left, .. } => {
                if self.pressed {
                    self.pressed = false;
                    if self.hovered { self.clicks += 1; }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0 && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}

/// Build the Interactive Container demo — a box that responds to hover and click.
pub fn interactive_container(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(Box::new(Label::new("Hover and click the box", Arc::clone(&font))
        .with_font_size(12.0)), 0.0);

    col.push(Box::new(InteractiveBox::new(Arc::clone(&font))), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(Label::new(
        "Background, border, and label change on hover / press.",
        Arc::clone(&font),
    ).with_font_size(11.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

// font_book is in the sibling module font_book.rs (re-exported from windows.rs).
// ---------------------------------------------------------------------------
// Misc Demos
// ---------------------------------------------------------------------------

/// A color swatch + name row used by the Colors section of misc_demos.
struct SwatchRow {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    color:    Color,
    label:    Label,
}

impl Widget for SwatchRow {
    fn type_name(&self) -> &'static str { "SwatchRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, 22.0);
        let ls = self.label.layout(Size::new(available.width - 30.0, 22.0));
        self.label.set_bounds(Rect::new(28.0, (22.0 - ls.height) * 0.5, ls.width, ls.height));
        Size::new(available.width, 22.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(self.color);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 3.0, 20.0, 16.0, 3.0);
        ctx.fill();
        self.label.set_color(v.text_color);
        let lb = self.label.bounds();
        ctx.save(); ctx.translate(lb.x, lb.y);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Box painting widget — draws N boxes whose visual properties are controlled
/// by shared cells (sliders set them externally).
struct BoxPainter {
    bounds:        Rect,
    children:      Vec<Box<dyn Widget>>,
    corner_radius: Rc<Cell<f64>>,
    stroke_width:  Rc<Cell<f64>>,
    num_boxes:     Rc<Cell<f64>>,
}

impl Widget for BoxPainter {
    fn type_name(&self) -> &'static str { "BoxPainter" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let h = 60.0_f64;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v  = ctx.visuals();
        let cr = self.corner_radius.get();
        let sw = self.stroke_width.get();
        let n  = self.num_boxes.get() as usize;
        let bw = 60.0_f64;
        let bh = 32.0_f64;
        let gap = 8.0_f64;
        let start_y = (self.bounds.height - bh) * 0.5;
        for i in 0..n {
            let x = i as f64 * (bw + gap);
            ctx.set_fill_color(Color::rgba(
                v.text_color.r, v.text_color.g, v.text_color.b, 0.35,
            ));
            ctx.begin_path();
            ctx.rounded_rect(x, start_y, bw, bh, cr);
            ctx.fill();
            if sw > 0.0 {
                ctx.set_stroke_color(v.text_color);
                ctx.set_line_width(sw);
                ctx.begin_path();
                ctx.rounded_rect(x, start_y, bw, bh, cr);
                ctx.stroke();
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Stress-test circles widget — draws 100 circles of increasing radius.
struct ManyCirclesWidget {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for ManyCirclesWidget {
    fn type_name(&self) -> &'static str { "ManyCirclesWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

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

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

/// Build the Misc Demos window matching egui's ✨ Misc Demos content.
///
/// Sections (shown flat, matching egui CollapsingHeader content):
/// - Label: colored text, font samples
/// - Misc widgets: angle drag, password field
/// - Checkboxes: checkbox grid, radio buttons
/// - Colors: named color swatches
/// - Box rendering: sliders + custom boxes
/// - Many circles: stress-test rendering
pub fn misc_demos(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(6.0)
        .with_padding(12.0)
        .with_panel_bg();

    let sec = |title: &str, f: &Arc<Font>| -> Box<dyn Widget> {
        Box::new(Label::new(title, Arc::clone(f)).with_font_size(12.5))
    };

    // ── Label section ────────────────────────────────────────────────────────
    col.push(sec("Label", &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    // Colored text samples on one row.
    let color_row = FlexRow::new().with_gap(6.0)
        .add(Box::new(Label::new("Text can have", Arc::clone(&font)).with_font_size(12.0)))
        .add(Box::new(Label::new("color,", Arc::clone(&font))
            .with_font_size(12.0)
            .with_color(Color::rgb(0.43, 1.0, 0.43))))
        .add(Box::new(Label::new("size,", Arc::clone(&font))
            .with_font_size(12.0)
            .with_color(Color::rgb(0.50, 0.55, 1.0))))
        .add(Box::new(Label::new("and style.", Arc::clone(&font))
            .with_font_size(12.0)
            .with_color(Color::rgb(1.0, 0.75, 0.40))));
    col.push(Box::new(color_row), 0.0);

    col.push(Box::new(Label::new(
        "The default font supports latin, cyrillic (ИÅđ…), math (∫√∞²⅓…), and emojis (💓🌟🖩…).",
        Arc::clone(&font),
    ).with_font_size(12.0)), 0.0);

    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── Misc widgets section ─────────────────────────────────────────────────
    col.push(sec("Misc widgets", &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    let angle_cell = Rc::new(Cell::new(2.094_f64)); // τ/3 ≈ 120°
    {
        let ac = Rc::clone(&angle_cell);
        let angle_row = FlexRow::new().with_gap(8.0)
            .add(Box::new(Label::new("An angle:", Arc::clone(&font)).with_font_size(12.5)))
            .add(Box::new(SizedBox::new().with_height(28.0).with_width(80.0).with_child(
                Box::new(DragValue::new(angle_cell.get(), -6.283, 6.283, Arc::clone(&font))
                    .with_speed(0.02)
                    .with_decimals(2)
                    .on_change(move |v| ac.set(v)))
            )));
        col.push(Box::new(angle_row), 0.0);
    }

    let pw_row = FlexRow::new().with_gap(8.0)
        .add(Box::new(Label::new("Password:", Arc::clone(&font)).with_font_size(12.5)))
        .add_flex(Box::new(SizedBox::new().with_height(28.0).with_child(
            Box::new(agg_gui::TextField::new(Arc::clone(&font))
                .with_font_size(12.5)
                .with_placeholder("hunter2")
                .with_password_mode(true))
        )), 1.0);
    col.push(Box::new(pw_row), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── Checkboxes section ───────────────────────────────────────────────────
    col.push(sec("Checkboxes", &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new(
        "Checkboxes with empty labels take up very little space:",
        Arc::clone(&font),
    ).with_font_size(11.5)), 0.0);

    // Small checkbox grid (10 columns × 2 rows = 20 checkboxes).
    let shared_bool = Rc::new(Cell::new(false));
    for _row in 0..2 {
        let mut cb_row = FlexRow::new().with_gap(2.0);
        for _col in 0..10 {
            let cell = Rc::clone(&shared_bool);
            cb_row.push(Box::new(SizedBox::new().with_height(22.0).with_width(22.0).with_child(
                Box::new(Checkbox::new("", Arc::clone(&font), cell.get())
                    .with_font_size(11.0)
                    .with_state_cell(Rc::clone(&cell))
                    .on_change(move |v| cell.set(v)))
            )), 0.0);
        }
        col.push(Box::new(cb_row), 0.0);
    }

    col.push(Box::new(SizedBox::new().with_height(28.0).with_child(
        Box::new(Checkbox::new("checkbox", Arc::clone(&font), false)
            .with_font_size(12.5))
    )), 0.0);

    col.push(Box::new(Label::new("Radio buttons:", Arc::clone(&font))
        .with_font_size(11.5)), 0.0);
    let radio_sel = Rc::new(Cell::new(0_usize));
    {
        let rs = Rc::clone(&radio_sel);
        col.push(Box::new(RadioGroup::new(
            vec!["Option A", "Option B", "Option C"],
            radio_sel.get(),
            Arc::clone(&font),
        ).with_font_size(12.5)
         .on_change(move |i| rs.set(i))), 0.0);
    }
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── Colors section ───────────────────────────────────────────────────────
    col.push(sec("Colors", &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    let named_colors: &[(&str, Color)] = &[
        ("Red",    Color::rgb(0.88, 0.25, 0.18)),
        ("Orange", Color::rgb(0.92, 0.55, 0.15)),
        ("Yellow", Color::rgb(0.92, 0.85, 0.15)),
        ("Green",  Color::rgb(0.25, 0.78, 0.30)),
        ("Cyan",   Color::rgb(0.22, 0.65, 0.88)),
        ("Blue",   Color::rgb(0.22, 0.45, 0.88)),
        ("Purple", Color::rgb(0.60, 0.25, 0.88)),
        ("Pink",   Color::rgb(0.88, 0.25, 0.65)),
    ];
    for &(name, color) in named_colors {
        let swatch = SwatchRow { bounds: Rect::default(), children: Vec::new(), color,
                                 label: Label::new(name, Arc::clone(&font)).with_font_size(11.5) };
        col.push(Box::new(swatch), 0.0);
    }
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── Box rendering section ────────────────────────────────────────────────
    col.push(sec("Test box rendering", &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    let corner_r = Rc::new(Cell::new(5.0_f64));
    let stroke_w = Rc::new(Cell::new(2.0_f64));
    let num_boxes = Rc::new(Cell::new(3.0_f64));

    {
        let cr = Rc::clone(&corner_r);
        let sw = Rc::clone(&stroke_w);
        let nb = Rc::clone(&num_boxes);

        col.push(Box::new(SizedBox::new().with_height(28.0).with_child(
            Box::new(Slider::new(corner_r.get(), 0.0, 50.0, Arc::clone(&font))
                .with_step(0.5).on_change(move |v| cr.set(v)))
        )), 0.0);
        col.push(Box::new(Label::new("corner radius", Arc::clone(&font)).with_font_size(10.5)), 0.0);

        col.push(Box::new(SizedBox::new().with_height(28.0).with_child(
            Box::new(Slider::new(stroke_w.get(), 0.0, 10.0, Arc::clone(&font))
                .with_step(0.5).on_change(move |v| sw.set(v)))
        )), 0.0);
        col.push(Box::new(Label::new("stroke width", Arc::clone(&font)).with_font_size(10.5)), 0.0);

        col.push(Box::new(SizedBox::new().with_height(28.0).with_child(
            Box::new(Slider::new(num_boxes.get(), 0.0, 8.0, Arc::clone(&font))
                .with_step(1.0).on_change(move |v| nb.set(v)))
        )), 0.0);
        col.push(Box::new(Label::new("number of boxes", Arc::clone(&font)).with_font_size(10.5)), 0.0);
    }

    col.push(Box::new(BoxPainter {
        bounds: Rect::default(), children: Vec::new(),
        corner_radius: Rc::clone(&corner_r),
        stroke_width:  Rc::clone(&stroke_w),
        num_boxes:     Rc::clone(&num_boxes),
    }), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── Many circles section ─────────────────────────────────────────────────
    col.push(sec("Many circles of different sizes", &font), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(ManyCirclesWidget { bounds: Rect::default(), children: Vec::new() }), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(ScrollView::new(Box::new(col)))
}
