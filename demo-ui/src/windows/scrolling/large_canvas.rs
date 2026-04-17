//! Large canvas tab: 10 000 rows rendered via a manual painter that honours
//! `ScrollView`'s viewport cell.  Each row is offset horizontally by
//! `(i % 100) px` to demonstrate a content canvas that is both very tall and
//! wide enough to scroll horizontally.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    DrawCtx, Event, EventResult, FlexColumn, Font, Rect, ScrollView, Separator,
    Size, Widget,
};

use super::helpers::wrapped_label;

const NUM_ROWS:   usize = 10_000;
const ROW_HEIGHT: f64   = 20.0;
const FONT_SIZE:  f64   = 12.0;
/// Maximum horizontal indent produced by `(i % 100) px` — plus a bit for text.
const CONTENT_WIDTH: f64 = 700.0;

struct VirtualCanvas {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    font:     Arc<Font>,
    viewport: Rc<Cell<Rect>>,
}

impl VirtualCanvas {
    fn new(font: Arc<Font>, viewport: Rc<Cell<Rect>>) -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), font, viewport }
    }
}

impl Widget for VirtualCanvas {
    fn type_name(&self) -> &'static str { "VirtualCanvas" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // Parent `ScrollView` with horizontal scroll passes `f64::MAX/2` for
        // `available.width` so it can absorb whatever natural width we
        // report — never `.max(available.width)` that value.
        let _ = available;
        let w = CONTENT_WIDTH;
        let h = (NUM_ROWS as f64) * ROW_HEIGHT;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v  = ctx.visuals();
        let vp = self.viewport.get();

        // vp = viewport in content-space top-down coords: (x, y_top, w, h)
        let first = (vp.y / ROW_HEIGHT).floor().max(0.0) as usize;
        let last  = ((vp.y + vp.height) / ROW_HEIGHT).ceil() as usize + 1;
        let last  = last.min(NUM_ROWS);

        let total_h = (NUM_ROWS as f64) * ROW_HEIGHT;
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(FONT_SIZE);
        ctx.set_fill_color(v.text_color);

        for i in first..last {
            let indent   = (i % 100) as f64;
            let x        = indent;
            let y_bottom = total_h - (i as f64 + 1.0) * ROW_HEIGHT;
            let y_text   = y_bottom + (ROW_HEIGHT - FONT_SIZE) * 0.5;
            let text = format!(
                "This is row {}/{}, indented by {} pixels",
                i + 1, NUM_ROWS, indent as i32,
            );
            ctx.fill_text(&text, x, y_text);
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

pub fn build(font: Arc<Font>) -> Box<dyn Widget> {
    let viewport = Rc::new(Cell::new(Rect::default()));

    let mut col = FlexColumn::new().with_gap(6.0).with_padding(10.0);
    col.push(wrapped_label(Arc::clone(&font),
        "10 000 rows, indented by their index mod 100, painted only where the \
         viewport intersects them.  Both axes scroll — horizontal via shift+wheel \
         or the bottom scrollbar.", 11.0), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    let canvas = VirtualCanvas::new(Arc::clone(&font), Rc::clone(&viewport));
    let scroll = ScrollView::new(Box::new(canvas))
        .horizontal(true)
        .with_viewport_cell(Rc::clone(&viewport));
    col.push(Box::new(scroll), 1.0);

    Box::new(col)
}
