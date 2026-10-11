//! Port of agg-sharp `BorderTests.cs`: a one-sided border paints exactly its
//! band and nothing else.
//!
//! C#'s `GuiWidget.Border` is a band just outside the child's bounds that its
//! parent fills in `BorderColor`; agg-gui's counterpart is
//! [`Container::with_border_sides`], a band per side inside the container's
//! bounds that insets its children like padding. The stretched bordered
//! widget fills the same 80 x 40 box either way, so the band lands on the
//! same pixels the C# test checks. The C# `BorderTestsVisualizer` is a
//! disabled manual viewer, not a test.

use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{Color, Container, HAnchor, Insets, Label, Size, VAnchor};

use super::support::{font, layout_at, pixel, render, RED};

const BORDER_SIZE: f64 = 1.0;
const WIDTH: u32 = 80;
const HEIGHT: u32 = 40;

enum Regions {
    Left,
    Bottom,
    Right,
    Top,
}

/// C# `GetBorderedWidget`: an 80 x 40 white container holding a stretched,
/// translucent-blue bordered widget and a centred label.
fn bordered_widget(border: Insets, name: &str) -> AbsoluteLayout {
    let widget = Container::new()
        .with_background(Color::rgba(0.0, 0.0, 1.0, 45.0 / 255.0))
        .with_border_sides(Color::rgba(1.0, 0.0, 0.0, 1.0), border)
        .with_h_anchor(HAnchor::STRETCH)
        .with_v_anchor(VAnchor::STRETCH);
    let label = Label::new(name, font())
        .with_h_anchor(HAnchor::CENTER)
        .with_v_anchor(VAnchor::CENTER);
    AbsoluteLayout::new()
        .add(Box::new(widget))
        .add(Box::new(label))
}

fn draw_border_on_surface(border: Insets, name: &str) -> agg_gui::Framebuffer {
    let mut container = bordered_widget(border, name);
    layout_at(&mut container, Size::new(WIDTH as f64, HEIGHT as f64));
    render(&mut container, WIDTH, HEIGHT)
}

fn assert_border_where_expected(region: Regions, border: Insets, fb: &agg_gui::Framebuffer) {
    let (w, h) = (fb.width() as f64, fb.height() as f64);
    // (left, bottom, right, top), C# `RectangleDouble`
    let b = match region {
        Regions::Left => (0.0, 0.0, border.left, h),
        Regions::Bottom => (0.0, 0.0, w, border.bottom),
        Regions::Right => (w - border.right, 0.0, w, h),
        Regions::Top => (0.0, h - border.top, w, h),
    };
    for x in 0..fb.width() {
        for y in 0..fb.height() {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            // C# RectangleDouble.Contains is inclusive on every edge
            let should_be_red = px >= b.0 && px <= b.2 && py >= b.1 && py <= b.3;
            let p = pixel(fb, x, y);
            if should_be_red {
                assert_eq!(p, RED, "pixel ({x}, {y}) should be the border's red");
            } else {
                assert_ne!(p, RED, "pixel ({x}, {y}) should not be red");
            }
        }
    }
}

#[test]
fn border_test_left() {
    let border = Insets::from_sides(BORDER_SIZE, 0.0, 0.0, 0.0);
    let surface = draw_border_on_surface(border, "left");
    assert_border_where_expected(Regions::Left, border, &surface);
}

#[test]
fn border_test_bottom() {
    let border = Insets::from_sides(0.0, 0.0, 0.0, BORDER_SIZE);
    let surface = draw_border_on_surface(border, "bottom");
    assert_border_where_expected(Regions::Bottom, border, &surface);
}

#[test]
fn border_test_right() {
    let border = Insets::from_sides(0.0, BORDER_SIZE, 0.0, 0.0);
    let surface = draw_border_on_surface(border, "right");
    assert_border_where_expected(Regions::Right, border, &surface);
}

#[test]
fn border_test_top() {
    let border = Insets::from_sides(0.0, 0.0, BORDER_SIZE, 0.0);
    let surface = draw_border_on_surface(border, "top");
    assert_border_where_expected(Regions::Top, border, &surface);
}
