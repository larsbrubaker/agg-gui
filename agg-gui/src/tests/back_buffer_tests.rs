//! Port of agg-sharp `Agg.UI/BackBufferTests.cs`: a double-buffered widget
//! draws the same pixels as an unbuffered one, a backbuffer opacity change
//! recomposites the cached layer without repainting its content, and a
//! backbuffer at a fractional position still lands its text 1:1 on the
//! pixels above it.
//!
//! C#'s `TextWidget.DoubleBufferDefault` is `Label::with_has_backbuffer`
//! (the label's CPU cache); C#'s `DrawString` reference is the same label
//! painted directly (`with_has_backbuffer(false)`). C#'s double-buffered
//! `GuiWidget` with `BackbufferOpacity` is a `Window` (agg-gui's widget with
//! a retained layer and an opacity) painted on `RetainedLayerCtx`
//! (`retained_layers.rs`), a ctx with retained layers that records the
//! composite alpha. C#'s `FindLeastSquaresMatch(image, maxError)` at the
//! expected spot is the mean squared channel difference against `maxError`.

use super::retained_layers::RetainedLayerCtx;
use super::*;
use crate::event::{Event, EventResult};
use crate::geometry::Rect;
use crate::text::Font;
use crate::widget::paint_subtree;
use crate::widgets::window::Window;
use crate::widgets::Label;
use crate::DrawCtx;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

/// Paint `text` as a label at `(x, 0)` on white, buffered or not, in
/// greyscale (C#'s test images take no subpixel text; agg-gui's direct text
/// path has none either, only its label cache does).
fn label_image(text: &str, x: f64, buffered: bool, w: u32, h: u32) -> Framebuffer {
    crate::font_settings::set_lcd_enabled(false);
    let mut label = Label::new(text, font())
        .with_font_size(12.0)
        .with_color(Color::black())
        .with_has_backbuffer(buffered);
    let used = label.layout(Size::new(w as f64, h as f64));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.translate(x, 0.0);
        paint_subtree(&mut label, &mut ctx);
    }
    crate::font_settings::clear_lcd_enabled_override();
    fb
}

fn mean_squared_difference(a: &Framebuffer, b: &Framebuffer) -> f64 {
    let sum: f64 = a
        .pixels()
        .iter()
        .zip(b.pixels())
        .map(|(p, q)| {
            let d = *p as f64 - *q as f64;
            d * d
        })
        .sum();
    sum / a.pixels().len() as f64
}

#[test]
fn double_buffer_tests() {
    let buffered = label_image("testing", 0.0, true, 65, 50);

    // make sure the frame comparison function works.
    let copy = buffered.pixels().to_vec();
    assert_eq!(buffered.pixels(), &copy[..]);

    let direct = label_image("testing", 0.0, false, 65, 50);
    assert!(
        buffered.pixels() == direct.pixels(),
        "a double-buffered label draws the same pixels as an unbuffered one"
    );
}

/// A widget that counts the times its content is painted (into its
/// backbuffer, when it has one).
struct PaintCountingWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    paints: Rc<Cell<u32>>,
}

impl Widget for PaintCountingWidget {
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
        self.paints.set(self.paints.get() + 1);
        ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[test]
fn changing_backbuffer_opacity_recomposites_without_repainting_the_content() {
    let paints = Rc::new(Cell::new(0));
    let content = PaintCountingWidget {
        bounds: Rect::default(),
        children: Vec::new(),
        paints: Rc::clone(&paints),
    };
    let mut child = Window::new("Child", font(), Box::new(content))
        .with_bounds(Rect::new(0.0, 0.0, 120.0, 80.0))
        .with_backbuffer_opacity(0.5);
    child.layout(Size::new(200.0, 160.0));
    let mut ctx = RetainedLayerCtx::new();

    paint_subtree(&mut child, &mut ctx);
    assert_eq!(paints.get(), 1);
    let half_alpha = ctx.last_alpha;

    // A fade frame: only the composite changes, so the parent is asked to
    // draw and the child's cache is kept.
    crate::animation::clear_draw_request();
    child.set_backbuffer_opacity(0.25);
    assert!(
        crate::animation::wants_draw(),
        "the parent is asked to draw"
    );

    paint_subtree(&mut child, &mut ctx);
    assert_eq!(paints.get(), 1);
    assert!(
        ctx.last_alpha < half_alpha,
        "composited at {} after {half_alpha}",
        ctx.last_alpha
    );
}

#[test]
fn back_buffers_are_screen_aligned() {
    // make sure draw string and a text widget produce the same result when
    // drawn to the same spot
    let draw_string = label_image("test", 0.0, false, 100, 20);
    let text_widget = label_image("test", 0.0, true, 100, 20);
    assert!(draw_string.pixels() == text_widget.pixels());

    // make sure that a back buffer is always trying to draw 1:1 pixels to
    // the buffer above
    let max_error = 10.0;
    let offset_direct = label_image("test", 23.3, false, 100, 20);
    let offset_buffered = label_image("test", 23.3, true, 100, 20);
    let error = mean_squared_difference(&offset_buffered, &offset_direct);
    assert!(error < max_error, "least squares {error} at 23.3");

    // A container half a pixel in, holding text at 23.3, lands as text drawn
    // at 23.8.
    let direct = label_image("test", 23.8, false, 100, 20);
    let nested = label_image("test", 0.5 + 23.3, true, 100, 20);
    let error = mean_squared_difference(&nested, &direct);
    assert!(error < 5.0, "least squares {error} at 0.5 + 23.3");
}
