//! Tests ported 1:1 from agg-sharp
//! `Tests/Agg.Tests/MarkdigAgg/MarkdownLinkIconTests.cs`, plus Rust-only
//! checks of the recolouring.
//!
//! C# wrote a black-globe `internet.svg` under a temporary
//! `StaticData.RootPath` and read the frames of the placeholder's
//! `ResponsiveImageSequenceWidget`; agg-gui has no static-data root, so the
//! same SVG is handed to [`MarkdownView::with_link_placeholder_icon`] and
//! the colour is read from the painted pixels: on a mid-gray page under a
//! dark theme, the globe's opaque ink must show the text colour exactly,
//! and nothing darker than the page (the icon file's black) may appear.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::Size;
use crate::gfx_ctx::GfxCtx;
use crate::icon_image::IconImage;
use crate::text::Font;
use crate::theme::Visuals;
use crate::widget::Widget;

use super::placeholder_icon::gray_to_color;
use super::MarkdownView;

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");

const BLACK_GLOBE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\"><circle cx=\"8\" cy=\"8\" r=\"6\" fill=\"#000\"/></svg>";

const DARK_THEME_TEXT: [u8; 3] = [230, 230, 230];
const PAGE: u8 = 100;

fn byte_color(rgb: [u8; 3]) -> Color {
    Color::rgb(
        f32::from(rgb[0]) / 255.0,
        f32::from(rgb[1]) / 255.0,
        f32::from(rgb[2]) / 255.0,
    )
}

/// Paints `![a picture](pic.png)` with the black-globe icon under a dark
/// theme whose text is [`DARK_THEME_TEXT`], on a [`PAGE`]-gray page.
fn paint_placeholder(with_icon: bool) -> Framebuffer {
    let (w, h) = (300u32, 60u32);
    crate::widget::set_current_viewport(Size::new(f64::from(w), f64::from(h)));
    let mut view = MarkdownView::new(
        "![a picture](pic.png)",
        Arc::new(Font::from_slice(TEST_FONT).expect("test font")),
    );
    if with_icon {
        let icon = IconImage::from_svg_data(BLACK_GLOBE.as_bytes(), Size::new(16.0, 16.0))
            .expect("globe svg");
        view = view.with_link_placeholder_icon(icon);
    }
    view.layout(Size::new(f64::from(w), f64::from(h)));
    let saved = crate::theme::current_visuals();
    let mut visuals = Visuals::dark();
    visuals.text_color = byte_color(DARK_THEME_TEXT);
    // The alt text draws dimmed: a tone of its own, so it is not mistaken for the globe.
    visuals.text_dim = byte_color([180, 180, 180]);
    crate::theme::set_visuals(visuals);
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(byte_color([PAGE, PAGE, PAGE]));
        ctx.translate(0.0, f64::from(h) - view.bounds().height);
        view.paint(&mut ctx);
    }
    crate::theme::set_visuals(saved);
    fb
}

fn count(fb: &Framebuffer, test: impl Fn(&[u8]) -> bool) -> usize {
    fb.pixels().chunks_exact(4).filter(|p| test(p)).count()
}

#[test]
fn the_image_placeholder_globe_is_drawn_in_the_theme_text_color() {
    let fb = paint_placeholder(true);

    let ink = count(&fb, |p| p[..3] == DARK_THEME_TEXT);
    assert!(ink > 0, "the globe's opaque ink is the theme text colour");
    let black = count(&fb, |p| p[0] < PAGE - 10);
    assert_eq!(
        black, 0,
        "the globe's opaque ink must be the theme text colour, not the icon file's black"
    );
}

#[test]
fn rust_only_without_an_icon_the_placeholder_is_the_shaded_alt_text_box() {
    let fb = paint_placeholder(false);

    assert_eq!(count(&fb, |p| p[..3] == DARK_THEME_TEXT), 0);
    // The box is shaded lighter than the page.
    assert!(count(&fb, |p| p[0] > PAGE && p[0] < PAGE + 10) > 0);
}

#[test]
fn rust_only_gray_to_color_recolours_gray_keeps_colour_and_alpha() {
    let mut pixels = vec![
        0, 0, 0, 255, // black ink
        255, 255, 255, 128, // white, half transparent
        200, 30, 30, 255, // a coloured pixel
        10, 20, 30, 0, // fully transparent
    ];

    gray_to_color(&mut pixels, byte_color(DARK_THEME_TEXT));

    assert_eq!(
        pixels,
        vec![230, 230, 230, 255, 230, 230, 230, 128, 200, 30, 30, 255, 0, 0, 0, 0]
    );
}

#[test]
fn rust_only_the_icon_widens_the_placeholder() {
    let font = || Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    crate::widget::set_current_viewport(Size::new(300.0, 60.0));
    let mut plain = MarkdownView::new("![a picture](pic.png)", font());
    plain.layout(Size::new(300.0, 60.0));
    let icon = IconImage::from_svg_data(BLACK_GLOBE.as_bytes(), Size::new(16.0, 16.0)).unwrap();
    let mut with_icon =
        MarkdownView::new("![a picture](pic.png)", font()).with_link_placeholder_icon(icon);
    with_icon.layout(Size::new(300.0, 60.0));

    let (a, b) = (plain.image_widths()[0], with_icon.image_widths()[0]);
    assert!((b - a - 20.0).abs() < 1e-9, "{a} -> {b}");
}
