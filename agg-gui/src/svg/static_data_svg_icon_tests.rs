//! Port of agg-sharp `Agg.UI/StaticDataSvgIconTests.cs`.
//!
//! MatterCAD loads its SVG icons (`static-data/Icons`) through
//! [`render_svg_to_framebuffer_at_size`] (mattercad-app's `static_icons::render_svg`), so these
//! tests pin the presentation features app icons are drawn with: stroke opacity, caps, joins,
//! attributes inherited from a group, and document paint order. C# rendered through
//! `StaticDataBase.LoadIcon` at DeviceScale 1; the Rust counterpart of that step is the same
//! rasterisation at `size x size`. `FillOpacityFadesTheFill` duplicates
//! `core_tests.rs::multiplies_group_and_paint_opacity` and is not repeated here.
//!
//! Pixels are sampled in viewBox units, y down from the top as SVG counts it; the framebuffer
//! is Y-up, so rows are flipped as C# flips its `GetPixel` row.

use super::*;
use crate::framebuffer::Framebuffer;

fn load_svg_icon(body: &str, size: u32) -> Framebuffer {
    let svg =
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">{body}</svg>");
    render_svg_to_framebuffer_at_size(svg.as_bytes(), size, size).expect("SVG should render")
}

/// The RGBA (premultiplied) at viewBox coordinates (x, y).
fn at(image: &Framebuffer, x: f64, y: f64) -> [u8; 4] {
    let (w, h) = (image.width() as i32, image.height() as i32);
    // C# `(int)(x / 100 * width)` truncates.
    let px = (w - 1).min((x / 100.0 * f64::from(w)) as i32);
    let py = (h - 1).min((y / 100.0 * f64::from(h)) as i32);
    let row = h - 1 - py;
    let i = ((row * w + px) * 4) as usize;
    let p = image.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

fn alpha_at(image: &Framebuffer, x: f64, y: f64) -> i32 {
    i32::from(at(image, x, y)[3])
}

#[test]
fn stroke_opacity_fades_the_stroke() {
    let image = load_svg_icon(
        "<path d=\"M0 50 H100\" fill=\"none\" stroke=\"#000\" stroke-width=\"40\" stroke-opacity=\".32\"/>",
        16,
    );
    let alpha = alpha_at(&image, 50.0, 50.0);
    assert!((74..=90).contains(&alpha), "alpha {alpha}");
}

#[test]
fn round_linecap_reaches_past_the_endpoint() {
    let line = |cap: &str| {
        format!(
            "<path d=\"M30 50 H70\" fill=\"none\" stroke=\"#000\" stroke-width=\"20\" stroke-linecap=\"{cap}\"/>"
        )
    };
    let round = load_svg_icon(&line("round"), 64);
    let butt = load_svg_icon(&line("butt"), 64);

    // Five units past the end, inside the round cap's radius of ten.
    assert!(
        alpha_at(&round, 75.0, 50.0) > 200,
        "round {}",
        alpha_at(&round, 75.0, 50.0)
    );
    assert!(
        alpha_at(&butt, 75.0, 50.0) < 20,
        "butt {}",
        alpha_at(&butt, 75.0, 50.0)
    );
}

#[test]
fn round_linejoin_cuts_the_miter_tip() {
    // The apex's miter ratio is 1/sin(26.6 deg) = 2.24, under the default limit of 4, so the miter
    // tip reaches about 11 units above the apex; a round join reaches only the half width, 5.
    let chevron = |join: &str| {
        format!(
            "<path d=\"M10 90 L50 20 L90 90\" fill=\"none\" stroke=\"#000\" stroke-width=\"10\" stroke-linejoin=\"{join}\"/>"
        )
    };
    let miter = load_svg_icon(&chevron("miter"), 64);
    let round = load_svg_icon(&chevron("round"), 64);

    assert!(
        alpha_at(&miter, 50.0, 11.0) > 150,
        "miter {}",
        alpha_at(&miter, 50.0, 11.0)
    );
    assert!(
        alpha_at(&round, 50.0, 11.0) < 20,
        "round {}",
        alpha_at(&round, 50.0, 11.0)
    );
}

#[test]
fn path_inherits_presentation_attributes_from_its_group() {
    let image = load_svg_icon(
        "<g fill=\"none\" stroke=\"#000\" stroke-width=\"8\"><path d=\"M20 20 H80 V80 H20 Z\"/></g>",
        64,
    );

    // fill="none" inherited: the inside stays clear; the inherited stroke draws the edge.
    assert!(alpha_at(&image, 50.0, 50.0) < 20);
    assert!(alpha_at(&image, 20.0, 50.0) > 200);
}

#[test]
fn later_elements_paint_over_earlier_ones() {
    let image = load_svg_icon(
        "<circle cx=\"50\" cy=\"50\" r=\"40\" fill=\"#f00\"/><path d=\"M0 0 H100 V100 H0 Z\" fill=\"#00f\"/>",
        16,
    );
    let center = at(&image, 50.0, 50.0);
    assert!(i32::from(center[2]) > 200, "blue {center:?}");
    assert!(i32::from(center[0]) < 50, "red {center:?}");
}
