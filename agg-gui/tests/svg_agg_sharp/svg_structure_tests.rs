//! agg-sharp's `SvgStructureTests` (agg/Svg structure and group compositing,
//! checked against resvg's semantics), ported 1:1. Renders are 100x100 of a
//! 100x100 viewBox, so one user unit is one pixel. C#'s `[Arguments]` theories
//! run each case in a loop under the theory's name.
//!
//! `a_root_with_no_size_draws_nothing`: usvg rejects a zero or negative size,
//! so agg-gui's render returns an error where agg-sharp returns a blank image;
//! the test checks that no image is drawn (an error, or a blank one).

use super::common::{render_root, try_render_root, Color, Image};

const VIEW_BOX: &str = "viewBox=\"0 0 100 100\"";

fn render(body: &str) -> Image {
    render_root(body, VIEW_BOX)
}

#[test]
fn transform_origin_moves_the_transforms_centre() {
    // usvg: the origin's percentages and keywords are of the viewport, not the element's box.
    let center = render("<rect x=\"40\" y=\"40\" width=\"20\" height=\"20\" fill=\"green\" transform=\"scale(2)\" transform-origin=\"center\"/>");
    assert_eq!(center.at(35, 35).alpha, 255);

    let corner = render("<rect x=\"80\" y=\"80\" width=\"20\" height=\"20\" fill=\"green\" transform=\"scale(2)\" transform-origin=\"bottom right\"/>");
    assert_eq!(corner.at(65, 65).alpha, 255);

    // One keyword that names a side: the other axis is centred. "top" is (50%, 0).
    let top = render("<rect x=\"40\" y=\"0\" width=\"20\" height=\"20\" fill=\"green\" transform=\"scale(2)\" style=\"transform-origin:top\"/>");
    assert_eq!(top.at(35, 35).alpha, 255);

    let px = render("<rect x=\"40\" y=\"40\" width=\"20\" height=\"20\" fill=\"green\" transform=\"scale(2)\" transform-origin=\"50px 50%\"/>");
    assert_eq!(px.at(35, 35).alpha, 255);
}

#[test]
fn switch_draws_only_its_first_child_whose_conditions_pass() {
    // usvg's switch: the first element child (unknown elements are not SVG) with no requiredExtensions, only
    // known requiredFeatures and a systemLanguage matching "en" (a region suffix ignored).
    let cases = [
        "<rect requiredExtensions=\"http://example.org/bogus\" width=\"100\" height=\"100\" fill=\"red\"/><rect width=\"100\" height=\"100\" fill=\"green\"/><rect width=\"100\" height=\"100\" fill=\"red\"/>",
        "<unknown/><rect width=\"100\" height=\"100\" fill=\"green\"/><rect width=\"100\" height=\"100\" fill=\"red\"/>",
        "<rect systemLanguage=\"ru-RU\" width=\"100\" height=\"100\" fill=\"red\"/><rect systemLanguage=\"ru, en-GB\" width=\"100\" height=\"100\" fill=\"green\"/><rect width=\"100\" height=\"100\" fill=\"red\"/>",
        "<rect requiredFeatures=\"http://www.w3.org/TR/SVG11/feature#Shape\" width=\"100\" height=\"100\" fill=\"green\"/><rect width=\"100\" height=\"100\" fill=\"red\"/>",
    ];
    for children in cases {
        assert_eq!(
            render(&format!("<switch>{children}</switch>")).at(50, 50),
            Color::new(0, 128, 0, 255),
            "{children}"
        );
    }
}

#[test]
fn a_failed_condition_hides_an_element_outside_a_switch() {
    assert_eq!(
        render("<rect width=\"100\" height=\"100\" fill=\"red\" systemLanguage=\"ru\"/>")
            .at(50, 50)
            .alpha,
        0
    );
    assert_eq!(
        render_root(
            "<rect width=\"100\" height=\"100\" fill=\"red\"/>",
            "viewBox=\"0 0 100 100\" systemLanguage=\"ru\""
        )
        .at(50, 50)
        .alpha,
        0
    );
}

#[test]
fn a_symbol_draws_only_through_a_use() {
    // Unused, a symbol draws nothing; used, its children draw with its opacity (its SVG 2 transform ignored).
    assert_eq!(
        render("<symbol id=\"s\"><rect width=\"100\" height=\"100\" fill=\"green\"/></symbol>")
            .at(50, 50)
            .alpha,
        0
    );
    let used = render("<symbol id=\"s\" opacity=\".5\" transform=\"translate(50 0)\"><rect x=\"10\" y=\"10\" width=\"20\" height=\"20\" fill=\"#008000\"/></symbol><use xlink:href=\"#s\"/>");
    assert_eq!(used.at(20, 20), Color::new(0, 128, 0, 128));
    assert_eq!(used.at(70, 20).alpha, 0);
}

#[test]
fn a_symbol_is_a_new_viewport_at_the_uses_rectangle() {
    // The use's x/y/width/height are the viewport: the viewBox fits into it, and what falls outside is clipped.
    // -50 -50 150 150 into 100x100 at (50, 50): scale 2/3, content (0, 0) lands at (50 + 33.3, 50 + 33.3).
    let fitted = render("<symbol id=\"s\" viewBox=\"-50 -50 150 150\"><rect width=\"150\" height=\"150\" fill=\"green\"/></symbol><use xlink:href=\"#s\" x=\"25\" y=\"25\" width=\"50\" height=\"50\"/>");
    assert_eq!(fitted.at(45, 45).alpha, 255);
    assert_eq!(fitted.at(40, 40).alpha, 0);
    assert_eq!(fitted.at(80, 80).alpha, 0);

    // No viewBox: the viewport is the use's rectangle (100% by default), moved with the use's transform.
    let clipped = render("<symbol id=\"s\"><rect x=\"-20\" y=\"-20\" width=\"200\" height=\"200\" fill=\"green\"/></symbol><use xlink:href=\"#s\" transform=\"translate(10 10)\" width=\"50\" height=\"50\"/>");
    assert_eq!(clipped.at(5, 5).alpha, 0);
    assert_eq!(clipped.at(30, 30).alpha, 255);
    assert_eq!(clipped.at(70, 70).alpha, 0);

    // overflow="visible" does not clip.
    let visible = render("<symbol id=\"s\" overflow=\"visible\"><rect x=\"-20\" y=\"-20\" width=\"200\" height=\"200\" fill=\"green\"/></symbol><use xlink:href=\"#s\" transform=\"translate(10 10)\" width=\"50\" height=\"50\"/>");
    assert_eq!(visible.at(5, 5).alpha, 255);
}

#[test]
fn a_nested_svg_is_a_new_viewport_clipped_to_its_rectangle() {
    // Its x/y/width/height clip what it draws, and percentages inside are of its own width/height.
    let clipped = render("<svg x=\"20\" y=\"20\" width=\"40\" height=\"40\"><rect width=\"100\" height=\"100\" fill=\"green\"/></svg>");
    assert_eq!(clipped.at(30, 30).alpha, 255);
    assert_eq!(clipped.at(70, 70).alpha, 0);

    let percent = render("<svg width=\"50\" height=\"50\" overflow=\"visible\"><rect width=\"100%\" height=\"100%\" fill=\"green\"/></svg>");
    assert_eq!(percent.at(40, 40).alpha, 255);
    assert_eq!(percent.at(60, 60).alpha, 0);

    // Only a viewBox and no rectangle: not clipped.
    let view_box_only = render_root(
        "<svg viewBox=\"0 0 100 100\"><rect x=\"-20\" y=\"-20\" width=\"50\" height=\"50\" fill=\"green\"/></svg>",
        "viewBox=\"-50 -50 100 100\"",
    );
    assert_eq!(view_box_only.at(35, 35).alpha, 255);
}

#[test]
fn a_uses_width_and_height_replace_the_svgs_it_references() {
    // The use's width (30) replaces the svg's 80; its height stays 80. Only the nearest use's size counts.
    let used = render("<defs><svg id=\"s\" width=\"80\" height=\"80\"><rect width=\"100\" height=\"100\" fill=\"green\"/></svg></defs><use xlink:href=\"#s\" width=\"30\"/>");
    assert_eq!(used.at(20, 70).alpha, 255);
    assert_eq!(used.at(40, 20).alpha, 0);
    assert_eq!(used.at(20, 90).alpha, 0);

    let nested = render("<defs><svg id=\"s\" width=\"80\" height=\"80\"><rect width=\"100\" height=\"100\" fill=\"green\"/></svg><use id=\"u\" xlink:href=\"#s\" height=\"30\"/></defs><use xlink:href=\"#u\" width=\"10\"/>");
    assert_eq!(nested.at(70, 20).alpha, 255);
    assert_eq!(nested.at(20, 40).alpha, 0);
}

#[test]
fn a_root_with_no_size_draws_nothing() {
    // usvg rejects a document whose width or height is zero or negative; resvg draws nothing.
    for root_attributes in [
        "width=\"0\" height=\"0\"",
        "width=\"-50\" height=\"-100\"",
        "viewBox=\"0 0 100 100\" width=\"100\" height=\"0\"",
    ] {
        let drawn = try_render_root(
            "<rect width=\"100\" height=\"100\" fill=\"green\"/>",
            root_attributes,
        )
        .map(|image| image.at(50, 50).alpha)
        .unwrap_or(0);
        assert_eq!(drawn, 0, "{root_attributes}");
    }
}

#[test]
fn elements_outside_the_svg_namespace_are_not_drawn() {
    // usvg skips an element (and all it holds) whose namespace is not SVG's, whatever its local name.
    let image = render("<s:g xmlns=\"http://www.example.org/notsvg\" xmlns:s=\"http://www.w3.org/2000/svg\"><s:rect width=\"100\" height=\"100\" fill=\"green\"/><rect width=\"100\" height=\"100\" fill=\"red\"/></s:g>");
    assert_eq!(image.at(50, 50), Color::new(0, 128, 0, 255));
}

#[test]
fn shape_rendering_without_anti_aliasing_draws_whole_pixels() {
    // resvg draws crispEdges and optimizeSpeed (inherited) with anti-aliasing off: every pixel is covered or not.
    for body in [
        "<circle cx=\"50\" cy=\"50\" r=\"40.3\" fill=\"green\" shape-rendering=\"crispEdges\"/>",
        "<g shape-rendering=\"optimizeSpeed\"><circle cx=\"50\" cy=\"50\" r=\"40.3\" fill=\"none\" stroke=\"green\" stroke-width=\"3.3\"/></g>",
    ] {
        let image = render(body);
        let mut partial = 0;
        for y in 0..100 {
            for x in 0..100 {
                let alpha = image.at(x, y).alpha;
                partial += i32::from(alpha != 0 && alpha != 255);
            }
        }
        assert_eq!(partial, 0, "{body}");
        assert_eq!(image.at(50, 10).alpha, 255, "{body}");
    }
}

#[test]
fn shape_rendering_does_not_affect_text() {
    let image = render(
        "<text x=\"5\" y=\"70\" font-size=\"60\" shape-rendering=\"crispEdges\">Text</text>",
    );
    let partial = (0..100).any(|y| {
        (0..100).any(|x| {
            let alpha = image.at(x, y).alpha;
            alpha != 0 && alpha != 255
        })
    });
    assert!(partial);
}
