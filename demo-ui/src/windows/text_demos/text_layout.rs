#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    measure_text_metrics, Button, Checkbox, Color, Container, DragValue, DrawCtx, Event,
    EventResult, FlexColumn, FlexRow, Font, Label, LabelAlign, MouseButton, Point, Rect,
    ScrollView, Separator, Size, SizedBox, TextField, Widget,
};

// ---------------------------------------------------------------------------
// Text Layout demo
// ---------------------------------------------------------------------------

const TEXT_LAYOUT_LOREM_IPSUM_LONG: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, \
quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure \
dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

/// Excerpt from Dolores Ibarruri's farewell speech to the International Brigades.
const TEXT_LAYOUT_LA_PASIONARIA: &str = "Mothers! Women!\n\
\n\
When the years pass by and the wounds of war are stanched; when the memory of the sad and bloody \
days dissipates in a present of liberty, of peace and of wellbeing; when the rancor have died out \
and pride in a free country is felt equally by all Spaniards, speak to your children. Tell them of \
these men of the International Brigades.\n\
\n\
Recount for them how, coming over seas and mountains, crossing frontiers bristling with bayonets, \
sought by raving dogs thirsting to tear their flesh, these men reached our country as crusaders for \
freedom, to fight and die for Spain's liberty and independence threatened by German and Italian \
fascism. They gave up everything - their loves, their countries, home and fortune, fathers, mothers, \
wives, brothers, sisters and children - and they came and said to us: \"We are here. Your cause, \
Spain's cause, is ours. It is the cause of all advanced and progressive mankind.\"\n\
\n\
- Dolores Ibarruri, 1938";

struct TextLayoutDemoState {
    max_rows: Rc<Cell<usize>>,
    break_mode: Rc<Cell<usize>>,
    overflow: Rc<Cell<usize>>,
    extra_letter_spacing: Rc<Cell<f64>>,
    custom_line_height: Rc<Cell<bool>>,
    line_height_pixels: Rc<Cell<f64>>,
    halign: Rc<Cell<usize>>,
    justify: Rc<Cell<bool>>,
    text_source: Rc<Cell<usize>>,
}

impl TextLayoutDemoState {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            max_rows: Rc::new(Cell::new(1000)),
            break_mode: Rc::new(Cell::new(0)),
            overflow: Rc::new(Cell::new(1)),
            extra_letter_spacing: Rc::new(Cell::new(0.0)),
            custom_line_height: Rc::new(Cell::new(false)),
            line_height_pixels: Rc::new(Cell::new(20.0)),
            halign: Rc::new(Cell::new(0)),
            justify: Rc::new(Cell::new(false)),
            text_source: Rc::new(Cell::new(0)),
        })
    }

    fn overflow_char(&self) -> Option<char> {
        match self.overflow.get() {
            1 => Some('…'),
            2 => Some('—'),
            3 => Some('-'),
            _ => None,
        }
    }

    fn align(&self) -> LabelAlign {
        match self.halign.get() {
            1 => LabelAlign::Center,
            2 => LabelAlign::Right,
            _ => LabelAlign::Left,
        }
    }

    fn text(&self) -> &'static str {
        if self.text_source.get() == 0 {
            TEXT_LAYOUT_LOREM_IPSUM_LONG
        } else {
            TEXT_LAYOUT_LA_PASIONARIA
        }
    }
}

#[derive(Clone)]
struct TextLayoutLine {
    text: String,
    paragraph_end: bool,
}

struct TextLayoutPreview {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    state: Rc<TextLayoutDemoState>,
    lines: Vec<TextLayoutLine>,
    line_h: f64,
    content_w: f64,
}

impl TextLayoutPreview {
    fn new(font: Arc<Font>, state: Rc<TextLayoutDemoState>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            state,
            lines: Vec::new(),
            line_h: 18.0,
            content_w: 0.0,
        }
    }

    fn font_size(&self) -> f64 {
        13.0
    }

    fn line_width(&self, text: &str, extra_spacing: f64) -> f64 {
        let gaps = text.chars().count().saturating_sub(1) as f64;
        measure_text_metrics(&self.font, text, self.font_size()).width + gaps * extra_spacing
    }

    fn push_word_wrapped(
        &self,
        paragraph: &str,
        max_width: f64,
        extra_spacing: f64,
        out: &mut Vec<TextLayoutLine>,
    ) {
        if paragraph.is_empty() {
            out.push(TextLayoutLine {
                text: String::new(),
                paragraph_end: true,
            });
            return;
        }

        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if current.is_empty() || self.line_width(&candidate, extra_spacing) <= max_width {
                current = candidate;
            } else {
                out.push(TextLayoutLine {
                    text: std::mem::replace(&mut current, word.to_string()),
                    paragraph_end: false,
                });
            }
        }

        out.push(TextLayoutLine {
            text: current,
            paragraph_end: true,
        });
    }

    fn push_anywhere_wrapped(
        &self,
        paragraph: &str,
        max_width: f64,
        extra_spacing: f64,
        out: &mut Vec<TextLayoutLine>,
    ) {
        if paragraph.is_empty() {
            out.push(TextLayoutLine {
                text: String::new(),
                paragraph_end: true,
            });
            return;
        }

        let mut current = String::new();
        for ch in paragraph.chars() {
            let candidate = format!("{current}{ch}");
            if !current.is_empty() && self.line_width(&candidate, extra_spacing) > max_width {
                out.push(TextLayoutLine {
                    text: std::mem::replace(&mut current, ch.to_string()),
                    paragraph_end: false,
                });
            } else {
                current = candidate;
            }
        }

        out.push(TextLayoutLine {
            text: current,
            paragraph_end: true,
        });
    }

    fn append_overflow(&self, line: &mut String, max_width: f64, extra_spacing: f64) {
        let Some(ch) = self.state.overflow_char() else {
            return;
        };
        let marker = ch.to_string();
        while !line.is_empty()
            && self.line_width(&format!("{line}{marker}"), extra_spacing) > max_width
        {
            line.pop();
        }
        line.push(ch);
    }

    fn rebuild_lines(&mut self, available_w: f64) {
        let extra_spacing = self.state.extra_letter_spacing.get();
        let max_width = available_w.max(1.0);
        let mut lines = Vec::new();
        for paragraph in self.state.text().split('\n') {
            if self.state.break_mode.get() == 1 {
                self.push_anywhere_wrapped(paragraph, max_width, extra_spacing, &mut lines);
            } else {
                self.push_word_wrapped(paragraph, max_width, extra_spacing, &mut lines);
            }
        }

        let max_rows = self.state.max_rows.get();
        if lines.len() > max_rows {
            lines.truncate(max_rows);
            if let Some(last) = lines.last_mut() {
                self.append_overflow(&mut last.text, max_width, extra_spacing);
                last.paragraph_end = true;
            }
        }

        self.lines = lines;
        self.content_w = max_width;
        self.line_h = if self.state.custom_line_height.get() {
            self.state.line_height_pixels.get().max(8.0)
        } else {
            self.font_size() * 1.35
        };
    }

    fn paint_spaced_line(
        &self,
        ctx: &mut dyn DrawCtx,
        line: &str,
        x: f64,
        y: f64,
        extra_spacing: f64,
        justify_spacing: f64,
    ) {
        let mut cursor_x = x;
        for ch in line.chars() {
            let s = ch.to_string();
            ctx.fill_text(&s, cursor_x, y);
            let w = ctx.measure_text(&s).map(|m| m.width).unwrap_or(0.0);
            cursor_x += w + extra_spacing;
            if ch.is_whitespace() {
                cursor_x += justify_spacing;
            }
        }
    }
}

impl Widget for TextLayoutPreview {
    fn type_name(&self) -> &'static str {
        "TextLayoutPreview"
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
        let content_w = (available.width - 24.0).max(1.0);
        self.rebuild_lines(content_w);
        let content_h = self.lines.len().max(1) as f64 * self.line_h;
        Size::new(available.width, content_h + 24.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let v = ctx.visuals();
        let pad = 12.0;

        ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.06));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        ctx.set_stroke_color(v.widget_stroke);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(0.5, 0.5, (w - 1.0).max(0.0), (h - 1.0).max(0.0));
        ctx.stroke();

        ctx.save();
        ctx.clip_rect(pad, pad, self.content_w, (h - pad * 2.0).max(0.0));
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size());
        ctx.set_fill_color(v.text_color);

        let extra_spacing = self.state.extra_letter_spacing.get();
        let align = self.state.align();
        let justify = self.state.justify.get();
        let total_text_h = self.lines.len() as f64 * self.line_h;
        let mut y = h - pad - self.line_h * 0.5 - (self.line_h - self.font_size()) * 0.35;

        for (i, line) in self.lines.iter().enumerate() {
            if !line.text.is_empty() {
                let line_w = self.line_width(&line.text, extra_spacing);
                let is_last = i + 1 == self.lines.len();
                let should_justify = justify && !line.paragraph_end && !is_last;
                let spaces = line.text.chars().filter(|c| c.is_whitespace()).count();
                let justify_spacing = if should_justify && spaces > 0 {
                    ((self.content_w - line_w) / spaces as f64).max(0.0)
                } else {
                    0.0
                };
                let draw_w = if should_justify {
                    self.content_w
                } else {
                    line_w
                };
                let x = match align {
                    LabelAlign::Center => pad + (self.content_w - draw_w) * 0.5,
                    LabelAlign::Right => pad + self.content_w - draw_w,
                    LabelAlign::Left => pad,
                };

                if extra_spacing.abs() > 0.01 || justify_spacing > 0.0 {
                    self.paint_spaced_line(ctx, &line.text, x, y, extra_spacing, justify_spacing);
                } else {
                    ctx.fill_text(&line.text, x, y);
                }
            }
            y -= self.line_h;
            if h - y > total_text_h + pad {
                break;
            }
        }

        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

struct SelectionButtons {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    labels: Vec<String>,
    selected: Rc<Cell<usize>>,
    hovered: Option<usize>,
    font_size: f64,
    label_widgets: Vec<Label>,
}

impl SelectionButtons {
    fn new(options: Vec<impl Into<String>>, selected: Rc<Cell<usize>>, font: Arc<Font>) -> Self {
        let labels: Vec<String> = options.into_iter().map(|s| s.into()).collect();
        let font_size = 12.0;
        let label_widgets = labels
            .iter()
            .map(|text| {
                Label::new(text.as_str(), Arc::clone(&font))
                    .with_font_size(font_size)
                    .with_align(LabelAlign::Center)
            })
            .collect();
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            labels,
            selected,
            hovered: None,
            font_size,
            label_widgets,
        }
    }

    fn button_h(&self) -> f64 {
        (self.font_size * 1.7).max(24.0)
    }

    fn index_at(&self, p: Point) -> Option<usize> {
        if self.labels.is_empty()
            || p.x < 0.0
            || p.y < 0.0
            || p.x > self.bounds.width
            || p.y > self.bounds.height
        {
            return None;
        }
        let cell_w = self.bounds.width / self.labels.len() as f64;
        Some(((p.x / cell_w).floor() as usize).min(self.labels.len() - 1))
    }
}

impl Widget for SelectionButtons {
    fn type_name(&self) -> &'static str {
        "SelectionButtons"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let h = self.button_h();
        let w = available.width;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        if !self.labels.is_empty() {
            let cell_w = w / self.labels.len() as f64;
            for label in &mut self.label_widgets {
                label.layout(Size::new(cell_w, h));
                label.set_bounds(Rect::new(0.0, 0.0, cell_w, h));
            }
        }
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.labels.is_empty() {
            return;
        }

        let v = ctx.visuals();
        let n = self.labels.len();
        let cell_w = self.bounds.width / n as f64;
        let h = self.bounds.height;
        let selected = self.selected.get().min(n - 1);

        for i in 0..n {
            let x = i as f64 * cell_w;
            let is_selected = i == selected;
            let is_hovered = self.hovered == Some(i);
            let bg = if is_selected {
                v.accent
            } else if is_hovered {
                v.widget_bg_hovered
            } else {
                v.widget_bg
            };
            let text = if is_selected {
                Color::white()
            } else {
                v.text_color
            };

            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(x, 0.0, cell_w, h, 4.0);
            ctx.fill();

            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.rounded_rect(
                x + 0.5,
                0.5,
                (cell_w - 1.0).max(0.0),
                (h - 1.0).max(0.0),
                4.0,
            );
            ctx.stroke();

            self.label_widgets[i].set_color(text);
            let lb = self.label_widgets[i].bounds();
            ctx.save();
            ctx.translate(x + (cell_w - lb.width) * 0.5, (h - lb.height) * 0.5);
            paint_subtree(&mut self.label_widgets[i], ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.index_at(*pos);
                if was != self.hovered {
                    agg_gui::animation::request_tick();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                if let Some(i) = self.index_at(*pos) {
                    if self.selected.get() != i {
                        self.selected.set(i);
                        agg_gui::animation::request_tick();
                    }
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}

fn text_layout_control_row(
    label: &'static str,
    control: Box<dyn Widget>,
    font: Arc<Font>,
) -> Box<dyn Widget> {
    Box::new(
        FlexRow::new()
            .with_gap(10.0)
            .add(Box::new(
                Label::new(label, Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_max_size(Size::new(130.0, f64::MAX))
                    .with_min_size(Size::new(130.0, 0.0)),
            ))
            .add_flex(control, 1.0),
    )
}

/// Build the Text Layout demo — mirrors egui's LayoutJob playground with live
/// controls for wrapping, elision, spacing, line height, alignment, and text.
pub fn text_layout(font: Arc<Font>) -> Box<dyn Widget> {
    let state = TextLayoutDemoState::new();
    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(14.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new("Text layout", Arc::clone(&font))
                .with_font_size(18.0)
                .with_color(Color::rgb(0.22, 0.45, 0.88)),
        ),
        0.0,
    );
    col.push(
        Box::new(
            Label::new(
                "A live LayoutJob-style playground modeled on egui's Text Layout demo.",
                Arc::clone(&font),
            )
            .with_font_size(12.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);

    {
        let cell = Rc::clone(&state.max_rows);
        col.push(
            text_layout_control_row(
                "Max rows:",
                Box::new(
                    DragValue::new(cell.get() as f64, 0.0, 1000.0, Arc::clone(&font))
                        .with_decimals(0)
                        .with_speed(1.0)
                        .on_change(move |v| cell.set(v.round().max(0.0) as usize)),
                ),
                Arc::clone(&font),
            ),
            0.0,
        );
    }

    col.push(
        text_layout_control_row(
            "Line-break:",
            Box::new(SelectionButtons::new(
                vec!["word boundaries", "anywhere"],
                Rc::clone(&state.break_mode),
                Arc::clone(&font),
            )),
            Arc::clone(&font),
        ),
        0.0,
    );

    col.push(
        text_layout_control_row(
            "Overflow character:",
            Box::new(SelectionButtons::new(
                vec!["None", "…", "—", "  -  "],
                Rc::clone(&state.overflow),
                Arc::clone(&font),
            )),
            Arc::clone(&font),
        ),
        0.0,
    );

    {
        let cell = Rc::clone(&state.extra_letter_spacing);
        col.push(
            text_layout_control_row(
                "Extra letter spacing:",
                Box::new(
                    DragValue::new(cell.get(), -5.0, 20.0, Arc::clone(&font))
                        .with_decimals(1)
                        .with_speed(0.1)
                        .on_change(move |v| cell.set(v)),
                ),
                Arc::clone(&font),
            ),
            0.0,
        );
    }

    let mut line_height_row = FlexRow::new().with_gap(10.0);
    line_height_row.push(
        Box::new(
            Checkbox::new("Custom", Arc::clone(&font), state.custom_line_height.get())
                .with_font_size(12.0)
                .with_state_cell(Rc::clone(&state.custom_line_height)),
        ),
        0.0,
    );
    {
        let cell = Rc::clone(&state.line_height_pixels);
        line_height_row.push(
            Box::new(
                DragValue::new(cell.get(), 8.0, 64.0, Arc::clone(&font))
                    .with_decimals(0)
                    .with_speed(1.0)
                    .on_change(move |v| cell.set(v.round().max(8.0))),
            ),
            1.0,
        );
    }
    col.push(
        text_layout_control_row("Line height:", Box::new(line_height_row), Arc::clone(&font)),
        0.0,
    );

    col.push(
        text_layout_control_row(
            "Horizontal align:",
            Box::new(SelectionButtons::new(
                vec!["Left", "Center", "Right"],
                Rc::clone(&state.halign),
                Arc::clone(&font),
            )),
            Arc::clone(&font),
        ),
        0.0,
    );

    col.push(
        text_layout_control_row(
            "Justify:",
            Box::new(
                Checkbox::new("Fill row width", Arc::clone(&font), state.justify.get())
                    .with_font_size(12.0)
                    .with_state_cell(Rc::clone(&state.justify)),
            ),
            Arc::clone(&font),
        ),
        0.0,
    );

    col.push(
        text_layout_control_row(
            "Text:",
            Box::new(SelectionButtons::new(
                vec!["Lorem Ipsum", "La Pasionaria"],
                Rc::clone(&state.text_source),
                Arc::clone(&font),
            )),
            Arc::clone(&font),
        ),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(TextLayoutPreview::new(Arc::clone(&font), state)),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(ScrollView::new(Box::new(col)))
}
