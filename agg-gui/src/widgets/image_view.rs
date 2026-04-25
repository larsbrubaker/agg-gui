//! `ImageView` — paints an `Option<(rgba8_top_down, w, h)>` as an image.
//!
//! Reads its pixel data from an `Rc<RefCell<Option<(Vec<u8>, u32, u32)>>>`
//! — the same shape [`ScreenshotHandle::image`][crate::ScreenshotHandle]
//! produces.  Fits the image into the widget's bounds preserving aspect
//! ratio, then outlines it in the theme text colour so the image boundary
//! is always visible against the neutral pane.
//!
//! Shows a themed "No image yet." placeholder while the source is `None`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

pub struct ImageView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    font: Arc<Font>,
    source: Rc<RefCell<Option<(Vec<u8>, u32, u32)>>>,
    /// Text shown when `source` is `None`.
    placeholder: String,
    /// Height floor; the widget expands to available height but never below.
    min_height: f64,
    /// When `true` (default), paints a 1-px outline around the image.
    outline: bool,
    /// When `true` (default), fills the widget's rounded rect with
    /// `visuals().bg_color` before drawing the image.
    fill_background: bool,
}

impl ImageView {
    pub fn new(font: Arc<Font>, source: Rc<RefCell<Option<(Vec<u8>, u32, u32)>>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            font,
            source,
            placeholder: "No image yet.".into(),
            min_height: 120.0,
            outline: true,
            fill_background: true,
        }
    }

    pub fn with_placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }
    pub fn with_min_height(mut self, h: f64) -> Self {
        self.min_height = h;
        self
    }
    pub fn with_outline(mut self, on: bool) -> Self {
        self.outline = on;
        self
    }
    pub fn with_fill_background(mut self, on: bool) -> Self {
        self.fill_background = on;
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }
}

impl Widget for ImageView {
    fn type_name(&self) -> &'static str {
        "ImageView"
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

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    fn layout(&mut self, available: Size) -> Size {
        let h = available.height.max(self.min_height);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        if self.fill_background {
            ctx.set_fill_color(v.bg_color);
            ctx.begin_path();
            ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
            ctx.fill();
        }

        let src = self.source.borrow();
        if let Some((pixels, iw, ih)) = src.as_ref() {
            let iwf = *iw as f64;
            let ihf = *ih as f64;
            let scale = (w / iwf).min(h / ihf).max(0.0);
            let dw = iwf * scale;
            let dh = ihf * scale;
            let dx = (w - dw) * 0.5;
            let dy = (h - dh) * 0.5;
            ctx.draw_image_rgba(pixels, *iw, *ih, dx, dy, dw, dh);

            if self.outline {
                ctx.set_stroke_color(v.text_color);
                ctx.set_line_width(1.0);
                ctx.begin_path();
                ctx.rect(dx, dy, dw, dh);
                ctx.stroke();
            }
        } else {
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(13.0);
            ctx.set_fill_color(v.text_dim);
            if let Some(m) = ctx.measure_text(&self.placeholder) {
                let tx = (w - m.width) * 0.5;
                let ty = h * 0.5 - (m.ascent - m.descent) * 0.5;
                ctx.fill_text(&self.placeholder, tx, ty);
            }
        }

        // Silence unused warning on `Color` when neither branch uses it.
        let _ = Color::white();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
