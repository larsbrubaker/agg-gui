//! `Hyperlink` — a clickable label rendered in link style (blue, underlined).
//!
//! Unlike a full URL-opening widget, `Hyperlink` fires a plain `on_click`
//! callback so callers can open URLs via whatever platform mechanism is
//! available (`web_sys::window().open()` on WASM, `open::open()` on native).

use std::sync::Arc;


use crate::event::{Event, EventResult, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

// Colors are resolved from ctx.visuals() at paint time.

/// A text label that looks like a hyperlink (blue, underlined) and fires a
/// callback when clicked.
pub struct Hyperlink {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    base:     WidgetBase,

    text:      String,
    font:      Arc<Font>,
    font_size: f64,

    hovered:  bool,
    on_click: Option<Box<dyn FnMut()>>,
}

impl Hyperlink {
    pub fn new(text: impl Into<String>, font: Arc<Font>) -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            base:     WidgetBase::new(),
            text:     text.into(),
            font,
            font_size: 14.0,
            hovered:  false,
            on_click: None,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self { self.font_size = size; self }
    pub fn on_click(mut self, cb: impl FnMut() + 'static) -> Self {
        self.on_click = Some(Box::new(cb)); self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
}

impl Widget for Hyperlink {
    fn type_name(&self) -> &'static str { "Hyperlink" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn is_focusable(&self) -> bool { true }

    fn layout(&mut self, available: Size) -> Size {
        let h = self.font_size * 1.5;
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let color = if self.hovered { v.text_link_hovered } else { v.text_link };
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);
        ctx.set_fill_color(color);

        let h = self.bounds.height;
        let m = ctx.measure_text(&self.text).unwrap_or_default();
        let ty = h * 0.5 - (m.ascent - m.descent) * 0.5;
        ctx.fill_text(&self.text, 0.0, ty);

        // Underline — drawn at the text baseline.
        let uw = m.width;
        let uy = ty - m.descent - 1.0; // 1 px below baseline
        ctx.set_stroke_color(color);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, uy);
        ctx.line_to(uw, uy);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, .. } => EventResult::Consumed,
            Event::MouseUp   { button: MouseButton::Left, pos, .. } => {
                if self.hit_test(*pos) {
                    if let Some(cb) = self.on_click.as_mut() { cb(); }
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}
