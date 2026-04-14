//! `Checkbox` — a boolean toggle with a label.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;

const BOX_SIZE: f64 = 16.0;
const GAP: f64 = 8.0;

/// A boolean toggle with a square box and a text label.
pub struct Checkbox {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    label: String,
    font: Arc<Font>,
    font_size: f64,
    checked: bool,
    hovered: bool,
    focused: bool,
    on_change: Option<Box<dyn FnMut(bool)>>,
}

impl Checkbox {
    pub fn new(label: impl Into<String>, font: Arc<Font>, checked: bool) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            label: label.into(),
            font,
            font_size: 14.0,
            checked,
            hovered: false,
            focused: false,
            on_change: None,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }

    pub fn on_change(mut self, cb: impl FnMut(bool) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    pub fn checked(&self) -> bool { self.checked }
    pub fn set_checked(&mut self, v: bool) { self.checked = v; }

    fn toggle(&mut self) {
        self.checked = !self.checked;
        let v = self.checked;
        if let Some(cb) = self.on_change.as_mut() { cb(v); }
    }
}

impl Widget for Checkbox {
    fn type_name(&self) -> &'static str { "Checkbox" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn layout(&mut self, available: Size) -> Size {
        let h = BOX_SIZE.max(self.font_size * 1.5);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let h = self.bounds.height;
        let box_y = (h - BOX_SIZE) * 0.5;

        // Focus ring
        if self.focused {
            ctx.set_stroke_color(Color::rgba(0.22, 0.45, 0.88, 0.55));
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.rounded_rect(-1.5, box_y - 1.5, BOX_SIZE + 3.0, BOX_SIZE + 3.0, 4.0);
            ctx.stroke();
        }

        // Box background
        let bg = if self.checked {
            Color::rgb(0.22, 0.45, 0.88)
        } else if self.hovered {
            Color::rgb(0.92, 0.93, 0.95)
        } else {
            Color::rgb(1.0, 1.0, 1.0)
        };
        ctx.set_fill_color(bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, box_y, BOX_SIZE, BOX_SIZE, 3.0);
        ctx.fill();

        // Box border
        let border = if self.checked {
            Color::rgb(0.16, 0.36, 0.72)
        } else {
            Color::rgb(0.75, 0.76, 0.78)
        };
        ctx.set_stroke_color(border);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.rounded_rect(0.0, box_y, BOX_SIZE, BOX_SIZE, 3.0);
        ctx.stroke();

        // Checkmark — coordinates in Y-up space (origin = box bottom-left).
        // Fractions are (1 - Y-down-fraction) so the tick reads correctly.
        if self.checked {
            ctx.set_stroke_color(Color::white());
            ctx.set_line_width(2.0);
            ctx.begin_path();
            let bx = 0.0;
            let by = box_y;
            ctx.move_to(bx + 3.0,              by + BOX_SIZE * 0.55); // left, mid-high
            ctx.line_to(bx + BOX_SIZE * 0.42,  by + BOX_SIZE * 0.28); // bend at bottom
            ctx.line_to(bx + BOX_SIZE - 3.0,   by + BOX_SIZE * 0.75); // right, upper
            ctx.stroke();
        }

        // Label text
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(Color::rgb(0.1, 0.1, 0.1));
        let tx = BOX_SIZE + GAP;
        if let Some(m) = ctx.measure_text(&self.label) {
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
            ctx.fill_text(&self.label, tx, ty);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                EventResult::Consumed
            }
            Event::MouseUp { button: MouseButton::Left, pos, .. } => {
                if self.hit_test(*pos) { self.toggle(); }
                EventResult::Consumed
            }
            Event::KeyDown { key: Key::Char(' '), .. } => {
                self.toggle();
                EventResult::Consumed
            }
            Event::FocusGained => { self.focused = true;  EventResult::Ignored }
            Event::FocusLost   => { self.focused = false; EventResult::Ignored }
            _ => EventResult::Ignored,
        }
    }
}
