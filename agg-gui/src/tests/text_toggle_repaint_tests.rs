//! Port of agg-sharp `Agg.UI/TextToggleRepaintTests.cs` (the LCD half):
//! turning LCD subpixel text on must reach labels inside a rounded,
//! buffered `Window`, directly and inside a scrolled container.
//!
//! C#'s `WindowWidget` with `CornerRadius = 8`, a white background and
//! `DoubleBuffer` is an agg-gui `Window` with `with_corner_radius(8.0)` and a
//! white body; C#'s `LcdRenderSettings.Enabled` is
//! `font_settings::set_lcd_enabled`. The window paints through the real
//! `App` paint path into a software `GfxCtx`. C#'s `HasChroma` (|R - B| > 24
//! anywhere in the label's screen rect) is kept, scanned over the window's
//! content area below the title bar, where the labels are the only ink.
//! `TurningBaselineSnappingOffRepaintsACachedLabel` is skipped by the
//! agg-ui triage: MatterCAD never toggles baseline snapping at runtime.

use super::*;
use crate::geometry::Rect;
use crate::text::Font;
use crate::widgets::window::Window;
use crate::widgets::Label;
use crate::Stack;
use std::sync::Arc;

const ROOT_W: f64 = 300.0;
const ROOT_H: f64 = 200.0;
const WIN: Rect = Rect {
    x: 20.0,
    y: 20.0,
    width: 200.0,
    height: 128.0,
};

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn label(text: &str) -> Label {
    Label::new(text, font())
        .with_font_size(12.0)
        .with_color(Color::black())
}

fn rounded_window_app(content: Box<dyn Widget>) -> App {
    let mut win = Window::new("Window", font(), content)
        .with_bounds(WIN)
        .with_corner_radius(8.0);
    win.set_body_color(Some(Color::white()));
    let mut app = App::new(Box::new(Stack::new().add(Box::new(win))));
    app.layout(Size::new(ROOT_W, ROOT_H));
    app
}

fn draw(app: &mut App) -> Framebuffer {
    let mut fb = Framebuffer::new(ROOT_W as u32, ROOT_H as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        app.paint(&mut ctx);
    }
    fb
}

/// C#'s `HasChroma` over the window's content area (Y-up rows, inset 2 px
/// from the rounded edge and the title bar).
fn has_chroma(fb: &Framebuffer) -> bool {
    let x0 = WIN.x as u32 + 2;
    let x1 = (WIN.x + WIN.width) as u32 - 2;
    let y0 = WIN.y as u32 + 2;
    let y1 = (WIN.y + WIN.height - 28.0) as u32 - 2;
    (y0..y1).any(|y| {
        (x0..x1).any(|x| {
            let p = sample(fb, x, y);
            (p[0] as i32 - p[2] as i32).abs() > 24
        })
    })
}

fn chroma_off_then_on(app: &mut App, why_on: &str) {
    crate::device_scale::set_device_scale(1.0);
    crate::font_settings::set_lcd_enabled(false);
    let grey = has_chroma(&draw(app));
    crate::font_settings::set_lcd_enabled(true);
    let lcd = has_chroma(&draw(app));
    crate::font_settings::clear_lcd_enabled_override();
    assert!(!grey, "greyscale text has none");
    assert!(lcd, "{why_on}");
}

#[test]
fn lcd_text_inside_a_rounded_window_has_subpixel_colour() {
    let mut app = rounded_window_app(Box::new(label("Hello World")));
    chroma_off_then_on(
        &mut app,
        "black LCD text on white has coloured fringes where its stems cut through a pixel",
    );
}

#[test]
fn lcd_text_in_a_scrolled_container_inside_a_rounded_window_has_subpixel_colour() {
    let mut column = FlexColumn::new();
    for i in 0..12 {
        column.push(Box::new(label(&format!("Hello World {i}"))), 0.0);
    }
    let mut app = rounded_window_app(Box::new(ScrollView::new(Box::new(column))));
    chroma_off_then_on(
        &mut app,
        "a scrolled label over the window's opaque body can be subpixel",
    );
}
