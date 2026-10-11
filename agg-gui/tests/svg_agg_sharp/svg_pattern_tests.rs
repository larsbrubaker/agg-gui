//! agg-sharp's `SvgPatternTests` (agg/Svg, stage 3: the `<pattern>` paint
//! server), ported 1:1. Renders are 100x100 of a 100x100 viewBox, so one user
//! unit is one pixel, and `at` takes SVG's y-down pixel coordinates.

use super::common::{assert_within, render, Color};

/// A 20x20 tile: red top-left quarter, blue bottom-right quarter, the rest transparent.
const CHECKER: &str = "<rect width=\"10\" height=\"10\" fill=\"red\"/><rect x=\"10\" y=\"10\" width=\"10\" height=\"10\" fill=\"blue\"/>";

fn is_red(c: Color) -> bool {
    c == Color::new(255, 0, 0, 255)
}

fn is_blue(c: Color) -> bool {
    c == Color::new(0, 0, 255, 255)
}

#[test]
fn user_space_tile_repeats_across_the_shape() {
    let image = render(&format!(
        "<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"20\" height=\"20\">{CHECKER}</pattern><rect width=\"100\" height=\"100\" fill=\"url(#p)\"/>"
    ));
    for (x, y) in [(5u32, 5u32), (25, 5), (85, 65), (45, 85)] {
        assert!(
            is_red(image.at(x, y)),
            "({x}, {y}) is in a tile's top-left quarter: {:?}",
            image.at(x, y)
        );
        assert!(
            is_blue(image.at(x + 10, y + 10)),
            "({}, {}) is in a tile's bottom-right quarter: {:?}",
            x + 10,
            y + 10,
            image.at(x + 10, y + 10)
        );
        assert_eq!(image.at(x + 10, y).alpha, 0);
    }
}

#[test]
fn tile_starts_at_x_and_y_and_only_inside_the_shape() {
    let image = render(&format!(
        "<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" x=\"5\" y=\"5\" width=\"20\" height=\"20\">{CHECKER}</pattern><rect x=\"0\" y=\"0\" width=\"50\" height=\"100\" fill=\"url(#p)\"/>"
    ));
    assert!(is_red(image.at(7, 7)), "{:?}", image.at(7, 7));
    assert!(
        is_blue(image.at(2, 2)),
        "the tile before (5, 5) wraps round to its blue quarter: {:?}",
        image.at(2, 2)
    );
    assert_eq!(image.at(60, 7).alpha, 0);
}

#[test]
fn object_bounding_box_units_scale_the_tile_to_the_shape() {
    // The default patternUnits: a quarter of the 40x40 box, with content in user space.
    let image = render(&format!(
        "<pattern id=\"p\" width=\".5\" height=\".5\">{CHECKER}</pattern><rect x=\"20\" y=\"20\" width=\"40\" height=\"40\" fill=\"url(#p)\"/>"
    ));
    assert!(is_red(image.at(25, 25)), "{:?}", image.at(25, 25));
    assert!(is_blue(image.at(35, 35)), "{:?}", image.at(35, 35));
    assert!(is_red(image.at(45, 45)), "{:?}", image.at(45, 45));
}

#[test]
fn view_box_fits_the_content_into_the_tile() {
    // A 40x40 viewBox squeezed into a 20x20 tile halves the checker.
    let image = render(&format!(
        "<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"20\" height=\"20\" viewBox=\"0 0 20 20\" patternContentUnits=\"objectBoundingBox\">{CHECKER}</pattern><pattern id=\"q\" patternUnits=\"userSpaceOnUse\" width=\"10\" height=\"10\" viewBox=\"0 0 20 20\">{CHECKER}</pattern><rect width=\"100\" height=\"50\" fill=\"url(#p)\"/><rect y=\"50\" width=\"100\" height=\"50\" fill=\"url(#q)\"/>"
    ));
    assert!(
        is_red(image.at(5, 5)),
        "with a viewBox patternContentUnits is ignored: {:?}",
        image.at(5, 5)
    );
    assert!(is_red(image.at(2, 52)), "{:?}", image.at(2, 52));
    assert!(is_blue(image.at(7, 57)), "{:?}", image.at(7, 57));
    assert!(is_red(image.at(12, 52)), "{:?}", image.at(12, 52));
}

#[test]
fn pattern_transform_turns_the_tiles() {
    // Turned a quarter about the origin, the tile's red quarter (x 0..10, y 0..10) lands on x -10..0: it
    // shows at x 10..20 of each period, where blue's quarter sits untransformed.
    let image = render(&format!(
        "<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"20\" height=\"20\" patternTransform=\"rotate(90)\">{CHECKER}</pattern><rect width=\"100\" height=\"100\" fill=\"url(#p)\"/>"
    ));
    assert!(is_red(image.at(15, 5)), "{:?}", image.at(15, 5));
    assert!(is_blue(image.at(5, 15)), "{:?}", image.at(5, 15));
}

#[test]
fn href_inherits_attributes_and_content() {
    let image = render(&format!(
        "<pattern id=\"base\" patternUnits=\"userSpaceOnUse\" width=\"20\" height=\"20\">{CHECKER}</pattern><pattern id=\"p\" xlink:href=\"#base\" x=\"10\"/><rect width=\"100\" height=\"100\" fill=\"url(#p)\"/>"
    ));
    assert!(
        is_red(image.at(15, 5)),
        "x=10 moves the inherited tile right: {:?}",
        image.at(15, 5)
    );
    assert!(is_blue(image.at(5, 15)), "{:?}", image.at(5, 15));
}

#[test]
fn fill_opacity_scales_the_pattern_and_strokes_take_it_too() {
    let image = render(&format!(
        "<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"20\" height=\"20\">{CHECKER}</pattern><rect width=\"50\" height=\"50\" fill=\"url(#p)\" fill-opacity=\".5\"/><line x1=\"60\" y1=\"5\" x2=\"100\" y2=\"5\" stroke=\"url(#p)\" stroke-width=\"6\"/>"
    ));
    let half = image.at(5, 5);
    assert_within(half.alpha as i32, 128, 1);
    assert!(half.red > 250, "{half:?}");
    assert!(is_red(image.at(65, 5)), "{:?}", image.at(65, 5));
}

#[test]
#[ignore = "usvg (0.45 and 0.48 alike) sets a self-referencing pattern paint to none, dropping its fallback colour"]
fn self_referencing_pattern_content_stops() {
    let image = render("<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"20\" height=\"20\"><rect width=\"10\" height=\"10\" fill=\"url(#p) red\"/></pattern><rect width=\"100\" height=\"100\" fill=\"url(#p)\"/>");
    assert!(
        is_red(image.at(5, 5)),
        "the innermost use falls back to its paint's colour: {:?}",
        image.at(5, 5)
    );
}
