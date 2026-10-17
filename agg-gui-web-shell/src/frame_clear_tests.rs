//! The web shell's frame body (`default_paint`) starts every frame from the
//! background: an area no widget paints shows the frame's clear, never the
//! canvas's previous contents (a browser's WebGPU canvas starts transparent
//! black; later frames would keep earlier frames' drawing as trails).
//!
//! Runs natively on a headless GPU frame, which begins with a stated clear as
//! the browser loop begins with the theme's. Skipped (passes trivially) when
//! no GPU adapter is available.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use crate::host::{default_paint, Frame};
use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::{App, Event, EventResult, Rect, Size, Widget};
use agg_gui_wgpu::{HeadlessFrame, HeadlessGpu};

const W: u32 = 64;
const H: u32 = 48;
const BLUE: [u8; 4] = [0, 0, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];

/// Fills the left half of the canvas red while `paint_red` is set, and paints
/// nothing otherwise.
struct HalfPainter {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    paint_red: Rc<Cell<bool>>,
}

impl Widget for HalfPainter {
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
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.paint_red.get() {
            ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
            ctx.begin_path();
            ctx.rect(0.0, 0.0, self.bounds.width / 2.0, self.bounds.height);
            ctx.fill();
        }
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn canvas_frame(index: u64) -> Frame {
    Frame {
        width: W,
        height: H,
        device_scale: 1.0,
        duration: Duration::ZERO,
        index,
        needs_layout: true,
        input_since_last_frame: false,
    }
}

/// Pixel `(x, y)` of a top-row-first RGBA read-back.
fn px(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

#[test]
fn default_paint_frames_show_the_background_where_nothing_paints() {
    let Ok(gpu) = HeadlessGpu::shared() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let paint_red = Rc::new(Cell::new(true));
    let mut app = App::new(Box::new(HalfPainter {
        bounds: Rect::default(),
        children: Vec::new(),
        paint_red: Rc::clone(&paint_red),
    }));
    let mut headless = HeadlessFrame::new(gpu, W, H);
    let bg = Color::rgb(0.0, 0.0, 1.0);

    let frame = canvas_frame(1);
    headless.render(bg, |ctx| default_paint(&mut app, ctx, &frame));
    let rgba = headless.read_rgba().expect("read back frame 1");
    assert_eq!(
        px(&rgba, W / 4, H / 2),
        RED,
        "the widget paints the left half"
    );
    assert_eq!(
        px(&rgba, W * 3 / 4, H / 2),
        BLUE,
        "the unpainted right half shows the frame's background, not black"
    );

    paint_red.set(false);
    let frame = canvas_frame(2);
    headless.render(bg, |ctx| default_paint(&mut app, ctx, &frame));
    let rgba = headless.read_rgba().expect("read back frame 2");
    assert_eq!(
        px(&rgba, W / 4, H / 2),
        BLUE,
        "what the previous frame drew is cleared, not left as a trail"
    );
}
