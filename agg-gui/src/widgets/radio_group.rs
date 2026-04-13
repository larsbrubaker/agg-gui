//! `RadioGroup` — a set of mutually exclusive radio buttons.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;

const DOT_R: f64 = 8.0;   // outer circle radius
const GAP: f64 = 8.0;
const ROW_H: f64 = 28.0;

/// A group of mutually-exclusive radio options.
///
/// Each option is a `(label, value_string)` pair. `selected` is the index of
/// the currently chosen option.
pub struct RadioGroup {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    options: Vec<String>,
    selected: usize,
    hovered: Option<usize>,
    focused: bool,
    font: Arc<Font>,
    font_size: f64,
    on_change: Option<Box<dyn FnMut(usize)>>,
}

impl RadioGroup {
    pub fn new(options: Vec<impl Into<String>>, selected: usize, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            options: options.into_iter().map(|s| s.into()).collect(),
            selected,
            hovered: None,
            focused: false,
            font,
            font_size: 14.0,
            on_change: None,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }

    pub fn on_change(mut self, cb: impl FnMut(usize) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn selected(&self) -> usize { self.selected }

    pub fn set_selected(&mut self, idx: usize) {
        if idx < self.options.len() { self.selected = idx; }
    }

    fn fire(&mut self) {
        let idx = self.selected;
        if let Some(cb) = self.on_change.as_mut() { cb(idx); }
    }

    /// Y coordinate (bottom-left) of the center of row `i` in Y-up space.
    fn row_center_y(&self, i: usize, total_h: f64) -> f64 {
        let n = self.options.len();
        if n == 0 { return total_h * 0.5; }
        // rows are stacked top-to-bottom, so row 0 is at the top.
        // In Y-up, top row has the largest Y.
        let row_top_y = total_h - (i as f64) * ROW_H;
        row_top_y - ROW_H * 0.5
    }

    fn row_for_y(&self, pos_y: f64) -> Option<usize> {
        let h = self.bounds.height;
        for i in 0..self.options.len() {
            let cy = self.row_center_y(i, h);
            if pos_y >= cy - ROW_H * 0.5 && pos_y < cy + ROW_H * 0.5 {
                return Some(i);
            }
        }
        None
    }
}

impl Widget for RadioGroup {
    fn type_name(&self) -> &'static str { "RadioGroup" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn layout(&mut self, available: Size) -> Size {
        let h = self.options.len() as f64 * ROW_H;
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;

        // Focus outline around whole widget
        if self.focused {
            ctx.set_stroke_color(Color::rgba(0.22, 0.45, 0.88, 0.45));
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.rounded_rect(-2.0, -2.0, self.bounds.width + 4.0, h + 4.0, 4.0);
            ctx.stroke();
        }

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);

        for (i, label) in self.options.iter().enumerate() {
            let cy = self.row_center_y(i, h);
            let checked = i == self.selected;
            let hovered = self.hovered == Some(i);

            // Outer circle
            let border = if checked {
                Color::rgb(0.22, 0.45, 0.88)
            } else if hovered {
                Color::rgb(0.70, 0.71, 0.73)
            } else {
                Color::rgb(0.75, 0.76, 0.78)
            };
            let bg = if checked { Color::rgb(0.22, 0.45, 0.88) } else { Color::white() };

            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.circle(DOT_R, cy, DOT_R);
            ctx.fill();

            ctx.set_stroke_color(border);
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.circle(DOT_R, cy, DOT_R);
            ctx.stroke();

            // Inner dot when checked
            if checked {
                ctx.set_fill_color(Color::white());
                ctx.begin_path();
                ctx.circle(DOT_R, cy, DOT_R * 0.45);
                ctx.fill();
            }

            // Label
            ctx.set_fill_color(Color::rgb(0.1, 0.1, 0.1));
            if let Some(m) = ctx.measure_text(label) {
                let ty = cy - (m.ascent - m.descent) * 0.5 + m.descent;
                ctx.fill_text(label, DOT_R * 2.0 + GAP, ty);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.row_for_y(pos.y);
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, pos, .. } => {
                if let Some(i) = self.row_for_y(pos.y) {
                    self.selected = i;
                    self.fire();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::KeyDown { key, .. } => {
                let n = self.options.len();
                let changed = match key {
                    Key::ArrowUp | Key::ArrowLeft => {
                        if self.selected > 0 { self.selected -= 1; true } else { false }
                    }
                    Key::ArrowDown | Key::ArrowRight => {
                        if self.selected + 1 < n { self.selected += 1; true } else { false }
                    }
                    _ => false,
                };
                if changed { self.fire(); EventResult::Consumed } else { EventResult::Ignored }
            }
            Event::FocusGained => { self.focused = true;  EventResult::Ignored }
            Event::FocusLost   => { self.focused = false; EventResult::Ignored }
            _ => EventResult::Ignored,
        }
    }
}
