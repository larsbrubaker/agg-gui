//! agg-sharp's `SvgModuleTests` (agg/Svg, stage 1: what the renderer draws for
//! each feature), ported 1:1. Renders are 100x100 of a 100x100 viewBox, so one
//! user unit is one pixel.
//!
//! The class's parse-only tests (`ParsesEveryColourNotation`,
//! `ParsesLengthsWithUnitsAndPercentages`, `TransformListsApplyRightmostFirst`,
//! `ViewBoxMeetCentresAndNoneStretches`, `StyleAttributeOverridesAndPropertiesInherit`,
//! `RoundedRectClampsItsRadiiAndTakesAMissingOneFromTheOther`,
//! `ASubpathAfterAClosepathStartsAtTheClosedSubpathsStart`) exercise agg-sharp's
//! own SVG parser (`SvgColor`, `SvgLength`, `SvgTransform`, `SvgViewport`,
//! `SvgStyle`, `SvgShapes`); agg-gui parses with usvg, so they have no
//! counterpart here.

use super::common::{render_document, render_root, Color, Image};

fn render(body: &str) -> Image {
    render_root(body, "viewBox=\"0 0 100 100\"")
}

#[test]
fn fills_in_svg_coordinates_top_down() {
    let image = render("<rect x=\"0\" y=\"0\" width=\"100\" height=\"20\" fill=\"#00f\"/>");
    assert_eq!(image.at(50, 10), Color::new(0, 0, 255, 255));
    assert_eq!(image.at(50, 80).alpha, 0);
}

#[test]
fn fill_opacity_comes_out_as_straight_alpha() {
    let image = render("<rect width=\"100\" height=\"100\" fill=\"green\" fill-opacity=\"0.5\"/>");
    assert_eq!(image.at(50, 50), Color::new(0, 128, 0, 128));
}

#[test]
fn group_opacity_composites_the_group_as_one_layer() {
    // Two overlapping opaque rects in a half-opaque group: the overlap is as transparent as the rest.
    let image = render("<g opacity=\"0.5\"><rect width=\"60\" height=\"60\" fill=\"red\"/><rect x=\"40\" y=\"40\" width=\"60\" height=\"60\" fill=\"blue\"/></g>");
    assert_eq!(image.at(50, 50), Color::new(0, 0, 255, 128));
    assert_eq!(image.at(10, 10), Color::new(255, 0, 0, 128));
}

#[test]
fn even_odd_leaves_the_inner_square_empty() {
    const SQUARES: &str = "M 0 0 H 100 V 100 H 0 Z M 25 25 H 75 V 75 H 25 Z";
    assert_eq!(
        render(&format!("<path d=\"{SQUARES}\" fill-rule=\"evenodd\"/>"))
            .at(50, 50)
            .alpha,
        0
    );
    assert_eq!(
        render(&format!("<path d=\"{SQUARES}\"/>")).at(50, 50).alpha,
        255
    );
}

/// The rasterizer treats Curve4 control points as line_to, so a cubic must be flattened before it is
/// rendered; unflattened, a circle of four cubics fills the polygon through its control points.
#[test]
fn cubics_are_flattened_to_their_curve() {
    // Kappa form circle: center (50,50), radius 40, control offset 0.5523 * 40 = 22.09.
    let image = render("<path d=\"M50 10 C72.09 10 90 27.91 90 50 C90 72.09 72.09 90 50 90 C27.91 90 10 72.09 10 50 C10 27.91 27.91 10 50 10 Z\"/>");

    // (79.5, 20.5) is 41.7 from the center: outside the circle, inside the control point polygon.
    assert!(image.alpha_at(79, 20) < 20);
    assert_eq!(image.alpha_at(50, 50), 255);
}

/// Curve flattening has to be fine enough for the output size, not the document's: a radius 7 circle
/// in a 16 unit document drawn at 1024 pixels must not facet inward by pixels.
#[test]
fn curve_flattening_follows_the_output_scale() {
    // Kappa form circle: center (8,8), radius 7.
    let image = render_document("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\"><path d=\"M8 1 C11.87 1 15 4.13 15 8 C15 11.87 11.87 15 8 15 C4.13 15 1 11.87 1 8 C1 4.13 4.13 1 8 1 Z\"/></svg>", 1024, 1024);

    // 6.95 of radius 7 is inside the true arc but outside a coarsely flattened one; 22 degrees is where
    // a coarse chord cuts furthest inside.
    for degrees in [22.0f64, 67.0] {
        let radians = degrees * std::f64::consts::PI / 180.0;
        let x = ((8.0 + 6.95 * radians.cos()) * 64.0) as u32;
        let y = ((8.0 + 6.95 * radians.sin()) * 64.0) as u32;
        assert!(image.alpha_at(x, y) > 200, "{degrees} degrees");
    }
}

#[test]
fn strokes_with_width_caps_and_dashes() {
    let butt = render(
        "<line x1=\"20\" y1=\"50\" x2=\"80\" y2=\"50\" stroke=\"black\" stroke-width=\"10\"/>",
    );
    assert_eq!(butt.at(50, 47).alpha, 255);
    assert_eq!(butt.at(15, 50).alpha, 0);

    let square = render("<line x1=\"20\" y1=\"50\" x2=\"80\" y2=\"50\" stroke=\"black\" stroke-width=\"10\" stroke-linecap=\"square\"/>");
    assert_eq!(square.at(17, 50).alpha, 255);

    let dashed = render("<line x1=\"0\" y1=\"50\" x2=\"100\" y2=\"50\" stroke=\"black\" stroke-width=\"10\" stroke-dasharray=\"20\"/>");
    assert_eq!(dashed.at(10, 50).alpha, 255);
    assert_eq!(dashed.at(30, 50).alpha, 0);
}

#[test]
fn use_draws_the_referenced_element_moved_and_defs_draw_nothing() {
    let image = render("<defs><rect id=\"box\" width=\"20\" height=\"20\" fill=\"green\"/></defs><use xlink:href=\"#box\" x=\"50\" y=\"50\"/><rect width=\"10\" height=\"10\" display=\"none\"/>");
    assert_eq!(image.at(60, 60), Color::new(0, 128, 0, 255));
    assert_eq!(image.at(10, 10).alpha, 0);
}

#[test]
fn view_box_scales_the_drawing() {
    // A 10x10 viewBox drawn at 100x100: the 5x5 rect fills the top-left quarter.
    let image = render_root(
        "<rect width=\"5\" height=\"5\" fill=\"red\"/>",
        "viewBox=\"0 0 10 10\"",
    );
    assert_eq!(image.at(45, 45).alpha, 255);
    assert_eq!(image.at(55, 55).alpha, 0);
}

#[test]
fn an_unsupported_paint_server_uses_its_fallback_colour() {
    let image = render("<rect width=\"100\" height=\"100\" fill=\"url(#missing) blue\"/>");
    assert_eq!(image.at(50, 50), Color::new(0, 0, 255, 255));
}
