//! Bidirectional tab: 100 lorem-ipsum paragraphs as non-wrapped single lines,
//! scrolled in both axes.  Our `ScrollView` now supports horizontal scrolling,
//! so this tab uses `.horizontal(true)` and lets the paragraph widget report a
//! wide natural width.

use std::sync::Arc;

use agg_gui::{
    DrawCtx, Event, EventResult, FlexColumn, Font, Rect, ScrollView, Separator,
    Size, Widget,
};

use super::helpers::{wrapped_label, LOREM_IPSUM_LONG};

const N_LINES:    usize = 100;
const LINE_HEIGHT: f64  = 20.0;
const FONT_SIZE:   f64  = 12.0;
/// Hard-coded content width wide enough that the full lorem ipsum line
/// requires horizontal scrolling on any reasonable window size.
const CONTENT_WIDTH: f64 = 2400.0;

struct LoremCanvas {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
}

impl LoremCanvas {
    fn new(font: Arc<Font>) -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), font }
    }
}

impl Widget for LoremCanvas {
    fn type_name(&self) -> &'static str { "LoremCanvas" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        let w = CONTENT_WIDTH.max(available.width);
        let h = (N_LINES as f64) * LINE_HEIGHT;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(FONT_SIZE);
        ctx.set_fill_color(v.text_color);

        let total_h = (N_LINES as f64) * LINE_HEIGHT;
        for i in 0..N_LINES {
            let y_bottom = total_h - (i as f64 + 1.0) * LINE_HEIGHT;
            let y_text   = y_bottom + (LINE_HEIGHT - FONT_SIZE) * 0.5;
            ctx.fill_text(LOREM_IPSUM_LONG, 4.0, y_text);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(wrapped_label(Arc::clone(&font),
        "100 lorem-ipsum paragraphs, rendered as single non-wrapped lines.  \
         Use the scrollbars or shift+wheel for horizontal scroll.", 11.0), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    let scroll = ScrollView::new(Box::new(LoremCanvas::new(Arc::clone(&font))))
        .horizontal(true);
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}
