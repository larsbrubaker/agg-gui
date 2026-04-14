//! `ProgressBar` — a read-only horizontal progress indicator.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

const BAR_H: f64 = 18.0;
const WIDGET_H: f64 = 24.0;

/// A horizontal progress bar. `value` is in `[0.0, 1.0]`.
pub struct ProgressBar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    value: f64,
    show_text: bool,
    font: Arc<Font>,
    font_size: f64,
    fill_color: Color,
}

impl ProgressBar {
    pub fn new(value: f64, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            value: value.clamp(0.0, 1.0),
            show_text: true,
            font,
            font_size: 11.0,
            fill_color: Color::rgb(0.22, 0.45, 0.88),
        }
    }

    pub fn with_show_text(mut self, show: bool) -> Self { self.show_text = show; self }
    pub fn with_fill_color(mut self, color: Color) -> Self { self.fill_color = color; self }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    pub fn set_value(&mut self, v: f64) {
        self.value = v.clamp(0.0, 1.0);
    }

    pub fn value(&self) -> f64 { self.value }
}

impl Widget for ProgressBar {
    fn type_name(&self) -> &'static str { "ProgressBar" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, WIDGET_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let bar_y = (h - BAR_H) * 0.5;
        let r = BAR_H * 0.5;

        // Track
        ctx.set_fill_color(Color::rgb(0.88, 0.89, 0.91));
        ctx.begin_path();
        ctx.rounded_rect(0.0, bar_y, w, BAR_H, r);
        ctx.fill();

        // Fill
        let fill_w = (w * self.value).max(0.0);
        if fill_w >= 1.0 {
            ctx.set_fill_color(self.fill_color);
            ctx.begin_path();
            if fill_w >= w - 0.5 {
                ctx.rounded_rect(0.0, bar_y, fill_w, BAR_H, r);
            } else {
                // Only round left side when not full
                ctx.rounded_rect(0.0, bar_y, fill_w, BAR_H, r);
            }
            ctx.fill();
        }

        // Percentage text centered over bar
        if self.show_text {
            let label = format!("{:.0}%", self.value * 100.0);
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(self.font_size);
            // Text color: white when covered by fill, dark otherwise
            let mid = w * 0.5;
            let text_color = if fill_w > mid {
                Color::rgba(1.0, 1.0, 1.0, 0.9)
            } else {
                Color::rgba(0.2, 0.2, 0.22, 0.8)
            };
            ctx.set_fill_color(text_color);
            if let Some(m) = ctx.measure_text(&label) {
                let tx = (w - m.width) * 0.5;
                let ty = bar_y + BAR_H * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
                ctx.fill_text(&label, tx, ty);
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
