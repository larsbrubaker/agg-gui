//! A rounded `Window` painted inside an ancestor's software-LCD backbuffer
//! (`BackbufferMode::LcdCoverage`, so the window draws into an `LcdGfxCtx`)
//! is cut to its rounded corners exactly as it is on an RGBA target:
//! agg-sharp's `WidgetBackbuffer` scales the panel by
//! `RoundedClipCoverage` before it lands in the LCD buffer, and
//! `LcdGfxCtx::set_layer_rounded_clip` does the same as its layer pops.
//! Also pins the layer seam at device scale 2 (the layer keeps the CTM's
//! scale, as `GfxCtx`'s does).

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{paint_subtree, BackbufferCache, BackbufferMode, Widget};
use crate::widgets::Window;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

const BACKDROP: Color = Color::from_rgb8(0, 0, 255);
const RED: Color = Color::from_rgb8(255, 0, 0);

/// Paints an opaque blue backdrop into its own LCD-coverage backbuffer;
/// its children (the window) paint into that same `LcdGfxCtx`.
struct LcdHost {
    bounds: Rect,
    cache: BackbufferCache,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for LcdHost {
    fn type_name(&self) -> &'static str {
        "LcdHost"
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
        for child in &mut self.children {
            let b = child.bounds();
            child.layout(Size::new(b.width, b.height));
            child.set_bounds(b);
        }
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(BACKDROP);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }
    fn backbuffer_mode(&self) -> BackbufferMode {
        BackbufferMode::LcdCoverage
    }
}

/// Window content that floods everything it is allowed to with red, so
/// only the window's rounded clip can keep red out of the corners.
struct RedFlood {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for RedFlood {
    fn type_name(&self) -> &'static str {
        "RedFlood"
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
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(RED);
        ctx.begin_path();
        ctx.rect(-500.0, -500.0, 2000.0, 2000.0);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

const WINDOW: Rect = Rect {
    x: 20.0,
    y: 20.0,
    width: 120.0,
    height: 80.0,
};

fn paint_host(scale: f64) -> Framebuffer {
    let font = Arc::new(Font::from_slice(FONT_BYTES).expect("font"));
    let flood = RedFlood {
        bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        children: Vec::new(),
    };
    let mut window = Window::new("Rounded", font, Box::new(flood))
        .with_bounds(WINDOW)
        .with_corner_radius(12.0);
    window.set_shadow(false);
    window.set_body_color(Some(RED));
    window.set_title_bar_color(Some(RED));
    window.set_border(0.0, None);
    let mut host = LcdHost {
        bounds: Rect::new(0.0, 0.0, 160.0, 120.0),
        cache: BackbufferCache::default(),
        children: vec![Box::new(window)],
    };
    host.layout(Size::new(160.0, 120.0));
    host.children[0].set_bounds(WINDOW);
    let w = (160.0 * scale) as u32;
    let h = (120.0 * scale) as u32;
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.scale(scale, scale);
        paint_subtree(&mut host, &mut ctx);
    }
    fb
}

fn rgb(fb: &Framebuffer, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

fn assert_corners_show_backdrop(fb: &Framebuffer, scale: f64) {
    let s = |v: f64| (v * scale) as u32;
    let (l, b) = (s(WINDOW.x), s(WINDOW.y));
    let (r, t) = (
        s(WINDOW.x + WINDOW.width) - 1,
        s(WINDOW.y + WINDOW.height) - 1,
    );
    for (x, y) in [(l, b), (r, b), (l, t), (r, t)] {
        let px = rgb(fb, x, y);
        assert!(
            px[0] < 16 && px[2] > 240,
            "corner pixel ({x}, {y}) of the rounded window must show the blue backdrop, got {px:?}"
        );
    }
    // The window itself is drawn: its middle is red.
    let mid = rgb(fb, s(WINDOW.x + 60.0), s(WINDOW.y + 30.0));
    assert!(
        mid[0] > 240 && mid[2] < 16,
        "the window's middle must be red, got {mid:?}"
    );
    // A pixel just inside a straight edge, away from any corner, is red too
    // (two in: the LCD filter's 5-tap reach leaves the outermost column of
    // any fill with subpixel fringe over the blue backdrop).
    let edge = rgb(fb, l + 2, s(WINDOW.y + 40.0));
    assert!(
        edge[0] > 240 && edge[2] < 16,
        "the window's straight left edge must be red, got {edge:?}"
    );
}

#[test]
fn rust_only_rounded_window_in_an_lcd_backbuffer_is_cut_to_its_corners() {
    let fb = paint_host(1.0);
    assert_corners_show_backdrop(&fb, 1.0);
}

#[test]
fn rust_only_rounded_window_in_an_lcd_backbuffer_is_cut_to_its_corners_at_device_scale_two() {
    let fb = paint_host(2.0);
    assert_corners_show_backdrop(&fb, 2.0);
}
