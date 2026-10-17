//! A shell frame starts from the background (`host::default_paint`) and the
//! presented-overlay pass keeps the frame's pixels (`paint::paint_overlay_pass`),
//! checked on the GPU's own pixels through a headless frame.
//!
//! The headless frame begins with a stated clear, as `Painter::paint` begins
//! with the theme's: an area no widget paints must show that clear, never the
//! target's previous contents (black on a fresh target, an earlier frame's
//! drawing otherwise). Skipped (pass trivially) when no GPU adapter is
//! available.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::{App, Event, EventResult, Rect, Size, Widget};
use agg_gui_wgpu::{HeadlessFrame, HeadlessGpu};

use crate::host::{default_paint, Frame};
use crate::paint::paint_overlay_pass;

const W: u32 = 64;
const H: u32 = 48;
const BLUE: [u8; 4] = [0, 0, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];

/// Fills the left half of the window red while `paint_red` is set, and
/// paints nothing otherwise — an animated widget over an unpainted area.
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

fn shell_frame(index: u64) -> Frame {
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

    let frame = shell_frame(1);
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
    let frame = shell_frame(2);
    headless.render(bg, |ctx| default_paint(&mut app, ctx, &frame));
    let rgba = headless.read_rgba().expect("read back frame 2");
    assert_eq!(
        px(&rgba, W / 4, H / 2),
        BLUE,
        "what the previous frame drew is cleared, not left as a trail"
    );
}

#[test]
fn the_overlay_pass_keeps_the_frame_pixels() {
    let Ok(gpu) = HeadlessGpu::shared() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let mut headless = HeadlessFrame::new(gpu, W, H);
    // Not the theme's background, so a pass that cleared to the theme would show.
    let bg = Color::rgb(0.0, 0.0, 1.0);
    assert_ne!(agg_gui::current_visuals().bg_color, bg);
    headless.render(bg, |_| {});

    let view = headless.target().view().clone();
    let frame = shell_frame(1);
    paint_overlay_pass(headless.ctx_mut(), view, &frame, |ctx| {
        ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 8.0, 8.0);
        ctx.fill();
    });
    let rgba = headless.read_rgba().expect("read back the overlaid frame");
    assert_eq!(
        px(&rgba, 2, H - 3),
        RED,
        "the overlay draws (bottom-left, Y up)"
    );
    assert_eq!(
        px(&rgba, W / 2, H / 2),
        BLUE,
        "the overlay pass keeps the frame's pixels instead of clearing them"
    );
}
