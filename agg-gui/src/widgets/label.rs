//! `Label` — static text display widget.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;

/// Horizontal alignment for `Label` text.
#[derive(Clone, Copy, Debug, Default)]
pub enum LabelAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A non-interactive text widget.
pub struct Label {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    text: String,
    font: Arc<Font>,
    font_size: f64,
    color: Color,
    align: LabelAlign,
}

impl Label {
    pub fn new(text: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            text: text.into(),
            font,
            font_size: 14.0,
            color: Color::rgb(0.1, 0.1, 0.1),
            align: LabelAlign::Left,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }
    pub fn with_color(mut self, color: Color) -> Self { self.color = color; self }
    pub fn with_align(mut self, align: LabelAlign) -> Self { self.align = align; self }

    pub fn set_text(&mut self, text: impl Into<String>) { self.text = text.into(); }
}

impl Widget for Label {
    fn type_name(&self) -> &'static str { "Label" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // Height based on font size; full available width.
        let h = self.font_size * 1.5;
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(self.color);

        if let Some(m) = ctx.measure_text(&self.text) {
            let ty = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
            let tx = match self.align {
                LabelAlign::Left   => 0.0,
                LabelAlign::Center => (w - m.width) * 0.5,
                LabelAlign::Right  => w - m.width,
            };
            ctx.fill_text(&self.text, tx, ty);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
