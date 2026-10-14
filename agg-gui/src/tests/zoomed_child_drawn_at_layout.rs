//! A widget is drawn where its layout puts it, within half a device pixel,
//! however deep it sits under a scaling `child_transform` (a node editor's
//! zoomed canvas) and at any display scale.
//!
//! The scene is MatterCAD's node card: a card on a zoomed, panned canvas, a
//! settings row inside it and a drop-down box inside the row, the row and the
//! box at half-logical offsets (a 2x layout snaps to device pixels). Painting
//! used to round each pixel-snapped child's offset in logical units, half a
//! unit per level, and then floor the device translation; under a 1.5x zoom
//! at 2x that drew the box 3 device pixels right of where hit testing,
//! `find_widget_screen_rect` and its own drop-down list placed it.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::widget::{find_widget_screen_rect, paint_subtree, Widget};
use crate::{Color, TransAffine};

const CANVAS_W: f64 = 800.0;
const CANVAS_H: f64 = 600.0;

/// A canvas whose children paint at `local * scale + offset`.
struct Canvas {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    scale: f64,
    offset: [f64; 2],
}

impl Widget for Canvas {
    fn type_name(&self) -> &'static str {
        "Canvas"
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
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn child_transform(&self) -> Option<TransAffine> {
        let mut t = TransAffine::new_scaling_uniform(self.scale);
        t.translate(self.offset[0], self.offset[1]);
        Some(t)
    }
}

/// A widget that fills its bounds when `fill` is set; `snapped` is its
/// `enforce_integer_bounds`.
struct Node {
    id: Option<&'static str>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    snapped: bool,
    fill: bool,
}

impl Widget for Node {
    fn type_name(&self) -> &'static str {
        "Node"
    }
    fn id(&self) -> Option<&str> {
        self.id
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
    fn enforce_integer_bounds(&self) -> bool {
        self.snapped
    }
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.fill {
            ctx.set_fill_color(Color::white());
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
            ctx.fill();
        }
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn node(bounds: Rect, snapped: bool, children: Vec<Box<dyn Widget>>) -> Box<dyn Widget> {
    Box::new(Node {
        id: None,
        bounds,
        children,
        snapped,
        fill: false,
    })
}

/// The card (not snapped, as a hosted card is) at a fractional canvas
/// position, its row at (30.5, 4) and the box at (204.5, 0) in the row.
fn scene(zoom: f64, pan: [f64; 2]) -> Canvas {
    let field = Box::new(Node {
        id: Some("box"),
        bounds: Rect::new(204.5, 0.0, 160.0, 24.0),
        children: vec![],
        snapped: true,
        fill: true,
    });
    let row = node(Rect::new(30.5, 4.0, 364.5, 24.0), true, vec![field]);
    let body = node(Rect::new(0.0, 6.0, 400.0, 119.0), true, vec![row]);
    let card = node(Rect::new(-40.3, 104.67, 400.0, 151.0), false, vec![body]);
    Canvas {
        bounds: Rect::new(0.0, 0.0, CANVAS_W, CANVAS_H),
        children: vec![card],
        scale: zoom,
        offset: pan,
    }
}

/// The left and bottom device-pixel edges of what was drawn: the first
/// column and row more than half covered (the fill is white on black).
fn drawn_left_bottom(fb: &Framebuffer) -> (f64, f64) {
    let (w, h) = (fb.width() as usize, fb.height() as usize);
    let px = fb.pixels();
    let covered = |x: usize, y: usize| px[(y * w + x) * 4] > 127;
    let mut left = usize::MAX;
    let mut bottom = usize::MAX;
    for y in 0..h {
        for x in 0..w {
            if covered(x, y) {
                left = left.min(x);
                bottom = bottom.min(y);
            }
        }
    }
    assert!(left != usize::MAX, "the box has to be drawn");
    (left as f64, bottom as f64)
}

#[test]
fn rust_only_a_zoomed_hosted_child_is_drawn_at_its_layout_position() {
    for ds in [1.0, 2.0] {
        for (zoom, pan) in [
            (1.0, [0.0, 0.0]),
            (0.5, [40.0, -25.0]),
            (1.5, [-60.0, 30.0]),
            (0.8164285714285714, [300.1428571428571, 153.57682653061224]),
            (1.5, [196.5, 142.0]),
        ] {
            let mut canvas = scene(zoom, pan);
            let layout = find_widget_screen_rect(&canvas, "box").expect("the box");
            let mut fb = Framebuffer::new((CANVAS_W * ds) as u32, (CANVAS_H * ds) as u32);
            {
                let mut ctx = GfxCtx::new(&mut fb);
                ctx.clear(Color::black());
                ctx.scale(ds, ds);
                paint_subtree(&mut canvas, &mut ctx);
            }
            let (left, bottom) = drawn_left_bottom(&fb);
            let context = format!("at {ds}x, zoom {zoom}, pan {pan:?}");
            assert!(
                (left - layout.x * ds).abs() <= 0.5 + 1e-9,
                "the box is drawn from device x {left}, its layout from {} - {context}",
                layout.x * ds
            );
            assert!(
                (bottom - layout.y * ds).abs() <= 0.5 + 1e-9,
                "the box is drawn from device y {bottom}, its layout from {} - {context}",
                layout.y * ds
            );
        }
    }
}
