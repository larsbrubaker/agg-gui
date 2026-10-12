//! Port of agg-sharp `Agg.UI/WindowWidgetStyleTests.cs`: a `Window` can draw
//! itself as a rounded card with a soft drop shadow (`set_corner_radius`,
//! `set_shadow_style`), one that is not asked to keeps its square corner, an
//! opaque window fills only the ring around its flat middle, and a rounded
//! window is never buffered as LCD coverage.
//!
//! C#'s panel (`window.TitleBar.Parent`) is the visible window inside its
//! grab border; agg-gui's `Window` bounds are that panel, with the shadow in
//! a halo outside them (`Window::layer_outsets`). C#'s `GetPixel(x, y)` is
//! bottom-up, as `Framebuffer::pixels` is. C#'s device scale is a
//! `GfxCtx::scale` before the window paints, as `App::paint` applies it.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::tests::paint_recorder::PaintRecorder;
use crate::widget::{paint_subtree, BackbufferKind, Widget};
use crate::widgets::Container;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

const BACKGROUND: Color = Color::from_rgb8(200, 220, 240);

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("parse the test font"))
}

fn rgb(fb: &Framebuffer, x: i32, y: i32) -> [i32; 3] {
    let i = ((y as u32 * fb.width() + x as u32) * 4) as usize;
    let p = fb.pixels();
    [p[i] as i32, p[i + 1] as i32, p[i + 2] as i32]
}

fn background_rgb() -> [i32; 3] {
    [200, 220, 240]
}

/// C#'s `Render`: a window with `BACKGROUND` (and no border) on a white
/// 300×300 parent, at `scale`; returns the image and the panel's rectangle
/// (left, bottom, right, top) in image pixels.
///
/// C#'s title bar is transparent until it is given a colour, so the
/// window's background shows through it; agg-gui's body leaves the title
/// strip to the title bar, so the title bar is given the background colour.
fn render(scale: f64, style: impl FnOnce(&mut Window)) -> (Framebuffer, (f64, f64, f64, f64)) {
    let saved = crate::device_scale::device_scale();
    crate::device_scale::set_device_scale(scale);
    let size = (300.0 * scale) as u32;
    let mut fb = Framebuffer::new(size, size);
    let mut window = Window::new("Window", font(), Box::new(Container::new()))
        .with_bounds(Rect::new(50.0, 50.0, 200.0, 150.0));
    window.set_body_color(Some(BACKGROUND));
    window.set_title_bar_color(Some(BACKGROUND));
    window.set_border(0.0, None);
    style(&mut window);
    window.layout(Size::new(300.0, 300.0));
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.scale(scale, scale);
        let b = window.bounds();
        ctx.translate(b.x, b.y);
        paint_subtree(&mut window, &mut ctx);
    }
    crate::device_scale::set_device_scale(saved);
    let b = window.bounds();
    let panel = (
        b.x * scale,
        b.y * scale,
        (b.x + b.width) * scale,
        (b.y + b.height) * scale,
    );
    (fb, panel)
}

/// AddTitleBar put MatterCAD's mh.png icon in front of the title, and an app
/// without that icon threw. agg-gui's title bar never loads an app icon;
/// what the C# test pins is that the bar it builds still closes the window.
#[test]
fn add_title_bar_works_without_matter_cads_icon() {
    let closed = Rc::new(Cell::new(None));
    let seen = Rc::clone(&closed);
    let mut window = Window::new("Title", font(), Box::new(Container::new()))
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 100.0))
        .on_close(move |reason| seen.set(Some(reason)));
    window.layout(Size::new(400.0, 400.0));

    let close = window.close_center();
    window.on_event(&Event::MouseDown {
        pos: close,
        button: MouseButton::Left,
        modifiers: Default::default(),
    });
    assert_eq!(closed.get(), Some(CloseReason::CloseButton));
}

fn a_rounded_window_has_a_round_corner_and_a_shadow_below_right(device_scale: f64) {
    let (image, panel) = render(device_scale, |window| {
        window.set_corner_radius(8.0);
        window.set_shadow_style(Some(WindowShadow {
            color: Color::rgba(0.0, 0.0, 0.0, 120.0 / 255.0),
            offset_x: 0.0,
            offset_y: -1.5,
            blur: 3.5,
        }));
    });
    let (left, bottom, right, top) = panel;
    let center = rgb(
        &image,
        ((left + right) / 2.0) as i32,
        ((bottom + top) / 2.0) as i32,
    );
    assert_eq!(
        center,
        background_rgb(),
        "the middle of the window is its background at {device_scale}x"
    );

    // The very corner pixel of the panel's bounding box is outside the rounded shape.
    let corner = rgb(&image, left as i32, top as i32 - 1);
    assert!(
        corner[0] > 240 && corner[1] > 240 && corner[2] > 240,
        "the top-left corner at {device_scale}x has to show the parent through the rounding, it was {corner:?}"
    );

    // Just outside the bottom and right edges, near the bottom-right corner,
    // the shadow falls, darker than the background. (The very corner is
    // rounded off, so the probes stand clear of the arc.)
    let below_bottom = rgb(
        &image,
        (right - 16.0 * device_scale) as i32,
        (bottom - 2.0 * device_scale) as i32,
    );
    let right_of_right = rgb(
        &image,
        (right + 1.0 * device_scale) as i32,
        (bottom + 16.0 * device_scale) as i32,
    );
    for (pixel, place) in [
        (below_bottom, "below the bottom edge"),
        (right_of_right, "right of the right edge"),
    ] {
        assert!(
            pixel[0] < background_rgb()[0],
            "just {place} has to be in a shadow darker than the window's background at {device_scale}x, it was {pixel:?}"
        );
    }
}

#[test]
fn a_rounded_window_has_a_round_corner_and_a_shadow_below_right_1() {
    a_rounded_window_has_a_round_corner_and_a_shadow_below_right(1.0);
}

#[test]
fn a_rounded_window_has_a_round_corner_and_a_shadow_below_right_2() {
    a_rounded_window_has_a_round_corner_and_a_shadow_below_right(2.0);
}

/// C#'s unstyled window is square with no shadow; agg-gui's default look is
/// rounded with a halo, so "unstyled" is radius 0 and the shadow off.
#[test]
fn an_unstyled_window_draws_its_square_corner_as_before() {
    let (image, panel) = render(1.0, |window| {
        window.set_corner_radius(0.0);
        window.set_shadow(false);
    });
    let corner = rgb(&image, panel.0 as i32, panel.3 as i32 - 1);
    assert_eq!(
        corner,
        background_rgb(),
        "an unstyled window keeps its square background corner, it was {corner:?}"
    );
}

/// The shadow is drawn in stacked layers the size of the whole window, and
/// the body is one more window-sized fill, but under an opaque body only a
/// thin ring around the edge needs a real fill: the middle is one flat
/// colour. On an LCD backbuffer every fill costs a 3x raster, a filter and a
/// per-pixel composite over its whole area.
///
/// C# counts `OnDrawBackground` (shadow and background) on an LCD buffer the
/// size of the window with its grab border. Here the whole window paint is
/// counted (title bar included), on a target the size of the panel and its
/// shadow halo, with no compositing layers (as `LcdGfxCtx`).
#[test]
fn an_opaque_window_only_fills_the_ring_around_its_flat_middle() {
    let mut window = Window::new("Window", font(), Box::new(Container::new()))
        .with_bounds(Rect::new(0.0, 0.0, 400.0, 300.0))
        .with_corner_radius(8.0)
        .with_shadow_style(WindowShadow {
            color: Color::rgba(0.0, 0.0, 0.0, 80.0 / 255.0),
            offset_x: 0.5,
            offset_y: -1.5,
            blur: 3.5,
        });
    window.set_body_color(Some(BACKGROUND));
    window.layout(Size::new(1000.0, 1000.0));
    let (l, b, r, t) = Window::layer_outsets();
    let (width, height) = (400.0 + l + r, 300.0 + b + t);
    let mut recorder = PaintRecorder::with_target(width, height);
    recorder.translate(l, b);
    paint::paint_window(&mut window, &mut recorder);

    // Four shadow layers and the background, each filled only in bands a
    // radius and a little wide around the edge, come to less than the
    // window's area together; filling each one whole was nearly five.
    let window_area = width * height;
    assert!(
        recorder.filled_area < window_area,
        "only the edge of the opaque window should be filled; {:.2} window areas were filled",
        recorder.filled_area / window_area
    );
    assert_eq!(recorder.clears.len(), 1, "the flat middle is cleared once");
}

/// A rounded window clips its panel, so nothing in it can be subpixel text,
/// and painting it as LCD coverage ran the whole window through the CPU LCD
/// pipeline. agg-gui's `Window` never asks for a software LCD buffer: its
/// retained buffer is a GPU layer (`BackbufferKind::GlFbo`) in which a square
/// window's text stays subpixel over its opaque body, and a rounded one is
/// composited through its rounded clip.
#[test]
fn a_rounded_window_is_not_buffered_as_lcd_coverage() {
    let mut window = Window::new("Window", font(), Box::new(Container::new()))
        .with_bounds(Rect::new(0.0, 0.0, 200.0, 100.0))
        .with_corner_radius(0.0);
    window.set_body_color(Some(BACKGROUND));

    let square = window.backbuffer_spec();
    assert_eq!(
        square.kind,
        BackbufferKind::GlFbo,
        "a square window's buffer is a GPU layer, where its text can be subpixel"
    );
    assert_eq!(square.rounded_clip, None);

    window.set_corner_radius(8.0);
    let rounded = window.backbuffer_spec();
    assert_ne!(
        rounded.kind,
        BackbufferKind::SoftwareLcd,
        "nothing in a rounded window can be subpixel, so LCD planes are only cost"
    );
    assert_eq!(rounded.kind, BackbufferKind::GlFbo);
    assert_eq!(rounded.rounded_clip, Some(8.0));
}

/// agg-sharp's `SolidInterior` turned a rect narrower than twice its radius
/// plus a pixel inside out and called the result covered; such a rect has no
/// interior.
#[test]
fn rust_only_a_window_narrower_than_its_corners_has_no_solid_interior() {
    let mut fb = Framebuffer::new(40, 40);
    let ctx = GfxCtx::new(&mut fb);
    assert_eq!(
        opaque_fill::solid_interior(&ctx, 0.0, 0.0, 10.0, 30.0, 8.0),
        None
    );
    assert_eq!(
        opaque_fill::solid_interior(&ctx, 0.0, 0.0, 30.0, 30.0, 8.0),
        Some((9.0, 9.0, 21.0, 21.0))
    );
}

/// Filling only the bands around an opaque body's flat middle draws the same
/// pixels as filling every shadow layer and the body whole.
#[test]
fn rust_only_banded_window_fills_match_whole_fills() {
    let (w, h) = (180.0, 120.0);
    let visuals = crate::theme::current_visuals();
    let mut style = chrome::ChromeStyle::from_visuals(&visuals);
    style.body_color = BACKGROUND;
    let paint = |banded: bool| {
        let mut fb = Framebuffer::new(240, 200);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            ctx.clear(Color::white());
            ctx.translate(30.0, 40.0);
            if banded {
                let skip = opaque_fill::solid_interior(&ctx, 0.0, 0.0, w, h - 28.0, 8.0);
                paint::paint_shadow(&mut ctx, w, h, &style, None, skip);
                paint::paint_body(&mut ctx, w, h, &style, false, true);
            } else {
                chrome::paint_chrome_shadow(&mut ctx, w, h, &style);
                chrome::paint_chrome_body(&mut ctx, w, h, &style, false);
            }
        }
        fb
    };
    let (banded, whole) = (paint(true), paint(false));
    let worst = banded
        .pixels()
        .iter()
        .zip(whole.pixels())
        .map(|(a, b)| (*a as i32 - *b as i32).abs())
        .max()
        .unwrap_or(0);
    assert!(
        worst <= 1,
        "banded fills differ from whole fills by {worst}"
    );
}
