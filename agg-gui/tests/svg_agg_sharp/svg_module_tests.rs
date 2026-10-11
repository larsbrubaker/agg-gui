//! agg-sharp's `SvgModuleTests` (agg/Svg, stage 1: parsing of colours,
//! lengths, transforms and styles, shape outlines, and what the renderer draws
//! for each feature), ported 1:1. Renders are 100x100 of a 100x100 viewBox, so
//! one user unit is one pixel.
//!
//! The parsing tests read agg-sharp's own parser (`SvgColor`, `SvgLength`,
//! `SvgTransform`, `SvgViewport`, `SvgStyle`, `SvgShapes`); agg-gui parses with
//! usvg, so they draw a shape whose colour, position or size is the parsed
//! value and assert its pixels.

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

/// `colour` filling the whole render, its centre pixel.
fn fill(colour: &str) -> Color {
    render(&format!(
        "<rect width=\"100\" height=\"100\" fill=\"{colour}\"/>"
    ))
    .at(50, 50)
}

/// C#'s `ParsesEveryColourNotation` cases: each notation and the colour it fills with.
const COLOUR_NOTATIONS: [(&str, Color); 10] = [
    ("green", Color::new(0, 128, 0, 255)),
    ("RebeccaPurple", Color::new(0x66, 0x33, 0x99, 255)),
    ("#f00", Color::new(255, 0, 0, 255)),
    ("#00ff0080", Color::new(0, 255, 0, 128)),
    ("#123456", Color::new(0x12, 0x34, 0x56, 255)),
    ("rgb(0, 128, 0)", Color::new(0, 128, 0, 255)),
    // C# expects 127 for 50%; usvg rounds 127.5 half up (see SvgParsingTests'
    // colours_parse_as_resvg_renders_them).
    ("rgb(100%, 0%, 50%)", Color::new(255, 0, 128, 255)),
    ("rgba(10, 20, 30, 0.5)", Color::new(10, 20, 30, 128)),
    ("hsl(120, 100%, 25%)", Color::new(0, 128, 0, 255)),
    ("hsl(120, 100%, 25%, 0.5)", Color::new(0, 128, 0, 128)),
];

/// Asserts every notation in [`COLOUR_NOTATIONS`] `keep` accepts, then that invalid colours leave
/// the initial fill, opaque black (resvg-test-suite painting/fill/random-value).
fn assert_colour_notations(keep: impl Fn(&str) -> bool) {
    for (text, expected) in COLOUR_NOTATIONS.iter().filter(|(text, _)| keep(text)) {
        assert_eq!(fill(text), *expected, "{text}");
    }
    assert_eq!(fill("notacolor"), Color::new(0, 0, 0, 255));
    assert_eq!(fill("#12"), Color::new(0, 0, 0, 255));
}

#[test]
#[ignore = "usvg loses the colour: svgtypes 0.16's named colour table is SVG 1.1's, without CSS Color 4's rebeccapurple, so fill=\"RebeccaPurple\" is invalid and draws black; rust_only_parses_every_colour_notation_usvg_names runs the other cases"]
fn parses_every_colour_notation() {
    assert_colour_notations(|_| true);
}

#[test]
fn rust_only_parses_every_colour_notation_usvg_names() {
    assert_colour_notations(|text| text != "RebeccaPurple");
}

/// The rect's right edge: the first column at y 50 that `width` leaves empty.
fn right_edge(width: &str) -> u32 {
    let image = render(&format!("<rect width=\"{width}\" height=\"100\"/>"));
    (0..100)
        .find(|&x| image.at(x, 50).alpha == 0)
        .unwrap_or(100)
}

#[test]
fn parses_lengths_with_units_and_percentages() {
    assert_eq!(right_edge("10"), 10);
    assert_eq!(right_edge("10px"), 10);
    assert_eq!(right_edge("1in"), 96);
    assert_eq!(right_edge("25.4mm"), 96);
    // 50% of the 100 wide viewport (C# resolves it against 200).
    assert_eq!(right_edge("50%"), 50);
    assert_eq!(right_edge("1e1"), 10);
    // C#'s fallback for an unparsable length is its caller's default; a rect's is 0, which draws nothing.
    assert_eq!(right_edge("bogus"), 0);

    // A number list in the compact form "1,2 3-4.5.5" uses: 0,0 100,-0 100,50.5 -.5,50 is the top half.
    let image = render("<polygon points=\"0,0 100-0 100 50.5-.5 50\"/>");
    assert_eq!(image.at(50, 25).alpha, 255);
    assert_eq!(image.at(50, 75).alpha, 0);
}

#[test]
fn transform_lists_apply_rightmost_first() {
    // translate(10) scale(2): scale first, so the 1..6 square goes to 2..12 and then 12..22.
    let image = render(
        "<rect x=\"1\" y=\"1\" width=\"5\" height=\"5\" transform=\"translate(10) scale(2)\"/>",
    );
    assert_eq!(image.at(12, 5).alpha, 255);
    assert_eq!(image.at(21, 5).alpha, 255);
    assert_eq!(image.at(11, 5).alpha, 0);
    assert_eq!(image.at(22, 5).alpha, 0);

    // rotate about a centre turns clockwise on screen (y down): a square around (60, 50) lands around (50, 60).
    let image = render(
        "<rect x=\"55\" y=\"45\" width=\"10\" height=\"10\" transform=\"rotate(90 50 50)\"/>",
    );
    assert_eq!(image.at(50, 60).alpha, 255);
    assert_eq!(image.at(60, 50).alpha, 0);

    // skewX(45) moves (0, 10) to (10, 10): at y 12.5 the 0..5 square spans 12.5..17.5.
    let image = render("<rect y=\"10\" width=\"5\" height=\"5\" transform=\"skewX(45)\"/>");
    assert_eq!(image.at(15, 12).alpha, 255);
    assert_eq!(image.at(2, 12).alpha, 0);

    // matrix(1 0 0 1 5 6) moves (1, 1) to (6, 7).
    let image = render(
        "<rect x=\"1\" y=\"1\" width=\"10\" height=\"10\" transform=\"matrix(1 0 0 1 5 6)\"/>",
    );
    assert_eq!(image.at(6, 7).alpha, 255);
    assert_eq!(image.at(15, 16).alpha, 255);
    assert_eq!(image.at(5, 7).alpha, 0);
    assert_eq!(image.at(16, 17).alpha, 0);

    // An invalid list is ignored, as the spec says.
    let image =
        render("<rect x=\"10\" y=\"10\" width=\"10\" height=\"10\" transform=\"scale(1,2,3)\"/>");
    assert_eq!(image.at(15, 15).alpha, 255);
    assert_eq!(image.at(15, 25).alpha, 0);
}

#[test]
fn view_box_meet_centres_and_none_stretches() {
    // A 100x50 viewBox in a 100x100 viewport; a 10x10 square at the viewBox corner (x, y).
    let square = |x: u32, y: u32, aspect: &str| {
        render_root(
            &format!("<rect x=\"{x}\" y=\"{y}\" width=\"10\" height=\"10\"/>"),
            &format!("width=\"100\" height=\"100\" viewBox=\"0 0 100 50\" {aspect}"),
        )
    };

    // Default xMidYMid meet: (0, 0) goes to (0, 25).
    let image = square(0, 0, "");
    assert_eq!(image.at(5, 30).alpha, 255);
    assert_eq!(image.at(5, 24).alpha, 0);
    assert_eq!(image.at(5, 35).alpha, 0);

    // none stretches: (100, 50) goes to (100, 100), so the last square covers 90..100 x 80..100.
    let image = square(90, 40, "preserveAspectRatio=\"none\"");
    assert_eq!(image.at(95, 81).alpha, 255);
    assert_eq!(image.at(95, 79).alpha, 0);

    // xMinYMax meet: (0, 0) goes to (0, 50).
    let image = square(0, 0, "preserveAspectRatio=\"xMinYMax meet\"");
    assert_eq!(image.at(5, 55).alpha, 255);
    assert_eq!(image.at(5, 49).alpha, 0);
}

#[test]
fn style_attribute_overrides_and_properties_inherit() {
    let image = render("<g fill=\"red\" opacity=\"0.5\" color=\"blue\" stroke-width=\"3\"><rect id=\"r\" x=\"20\" y=\"20\" width=\"60\" height=\"60\" fill=\"green\" style=\"fill: yellow; stroke: currentColor\"/></g>");
    // The style's yellow fill, at the group's opacity once: opacity is not inherited.
    assert_eq!(image.at(50, 50), Color::new(255, 255, 0, 128));
    // The stroke is the inherited colour, blue, and its inherited width 3 spans 18.5..21.5.
    assert_eq!(image.at(19, 50), Color::new(0, 0, 255, 128));
    let edge = image.at(18, 50).alpha;
    assert!(
        edge > 0 && edge < 128,
        "half-covered stroke edge, got {edge}"
    );
    assert_eq!(image.at(17, 50).alpha, 0);
}

#[test]
fn rounded_rect_clamps_its_radii_and_takes_a_missing_one_from_the_other() {
    // rx 200 clamps to 48 and ry takes it, then clamps to 24: the 96x48 rect is an ellipse.
    let image = render("<rect x=\"2\" y=\"26\" width=\"96\" height=\"48\" rx=\"200\"/>");
    assert_eq!(image.at(3, 50).alpha, 255);
    assert_eq!(image.at(96, 50).alpha, 255);
    assert_eq!(image.at(50, 27).alpha, 255);
    assert_eq!(image.at(50, 72).alpha, 255);
    // Outside the ellipse but inside the rect with 24 radius round corners ry alone would give.
    assert_eq!(image.at(10, 34).alpha, 0);
    assert_eq!(image.at(3, 27).alpha, 0);

    // A zero width rect and a zero rx ellipse draw nothing, even stroked.
    let image = render("<rect x=\"50\" y=\"10\" width=\"0\" height=\"10\" stroke=\"black\" stroke-width=\"10\"/>\
                        <ellipse cx=\"50\" cy=\"50\" rx=\"0\" ry=\"5\" stroke=\"black\" stroke-width=\"10\"/>");
    assert_eq!(image.at(50, 15).alpha, 0);
    assert_eq!(image.at(50, 50).alpha, 0);
}

#[test]
fn a_subpath_after_a_closepath_starts_at_the_closed_subpaths_start() {
    // After z the pen is back at (10, 50), so the stroked "L 60 60" runs (10, 50)..(60, 60), not from (50, 10).
    let image = render("<path d=\"M 10 50 L 10 10 L 50 10 z L 60 60\" fill=\"none\" stroke=\"black\" stroke-width=\"4\"/>");
    assert_eq!(image.at(35, 55).alpha, 255);
    assert_eq!(image.at(55, 35).alpha, 0);
}
