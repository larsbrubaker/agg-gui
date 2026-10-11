//! agg-sharp's `SvgMarkerTests` (agg/Svg: markers, placed and oriented as
//! resvg places them), ported 1:1. Renders are 100x100 of a 100x100 viewBox.

use super::common::{render_document, Image};

/// A 4 by 4 square marker centred on its vertex, one user unit per unit, unclipped.
const SQUARE: &str = "<marker id=\"m\" markerUnits=\"userSpaceOnUse\" markerWidth=\"4\" markerHeight=\"4\" refX=\"2\" refY=\"2\"><rect width=\"4\" height=\"4\" fill=\"#00ff00\"/></marker>";

/// A 10-long, 2-wide bar from its vertex along the marker's +x: shows which way a marker faces.
fn bar(orient: &str) -> String {
    format!("<marker id=\"b\" markerUnits=\"userSpaceOnUse\" markerWidth=\"10\" markerHeight=\"2\" refY=\"1\" orient=\"{orient}\"><rect width=\"10\" height=\"2\" fill=\"#00ff00\"/></marker>")
}

/// The C# class's document has no xlink namespace.
fn render(body: &str) -> Image {
    render_document(
        &format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">{body}</svg>"),
        100,
        100,
    )
}

#[test]
fn start_mid_and_end_markers_draw_at_their_vertices() {
    let all = render(&format!("{SQUARE}<path d=\"M10 50 L50 50 L90 50\" fill=\"none\" marker-start=\"url(#m)\" marker-mid=\"url(#m)\" marker-end=\"url(#m)\"/>"));
    assert_eq!(all.alpha_at(10, 50), 255);
    assert_eq!(all.alpha_at(50, 50), 255);
    assert_eq!(all.alpha_at(90, 50), 255);
    assert_eq!(all.alpha_at(30, 50), 0);

    let end_only = render(&format!(
        "{SQUARE}<path d=\"M10 50 L50 50 L90 50\" fill=\"none\" marker-end=\"url(#m)\"/>"
    ));
    assert_eq!(end_only.alpha_at(10, 50), 0);
    assert_eq!(end_only.alpha_at(50, 50), 0);
    assert_eq!(end_only.alpha_at(90, 50), 255);
}

#[test]
fn the_marker_shorthand_in_css_sets_all_three_and_is_inherited() {
    let image = render(&format!("<style>g {{ marker: url(#m) }}</style>{SQUARE}<g><polyline points=\"10 50 50 50 90 50\" fill=\"none\"/></g>"));
    assert_eq!(image.alpha_at(10, 50), 255);
    assert_eq!(image.alpha_at(50, 50), 255);
    assert_eq!(image.alpha_at(90, 50), 255);
}

#[test]
fn orient_auto_follows_the_path_and_auto_start_reverse_flips_the_start() {
    // The path runs down: auto turns the bar down, auto-start-reverse turns the start's up.
    let auto = render(&format!(
        "{}<path d=\"M50 30 L50 70\" fill=\"none\" marker-start=\"url(#b)\"/>",
        bar("auto")
    ));
    assert_eq!(auto.alpha_at(50, 36), 255);
    assert_eq!(auto.alpha_at(50, 24), 0);
    assert_eq!(auto.alpha_at(56, 30), 0);

    let reversed = render(&format!(
        "{}<path d=\"M50 30 L50 70\" fill=\"none\" marker-start=\"url(#b)\"/>",
        bar("auto-start-reverse")
    ));
    assert_eq!(reversed.alpha_at(50, 24), 255);
    assert_eq!(reversed.alpha_at(50, 36), 0);

    // A fixed angle ignores the path: a quarter turn points down whichever way the path goes.
    let angle = render(&format!(
        "{}<path d=\"M50 30 L90 30\" fill=\"none\" marker-start=\"url(#b)\"/>",
        bar("0.25turn")
    ));
    assert_eq!(angle.alpha_at(50, 36), 255);
    assert_eq!(angle.alpha_at(56, 30), 0);
}

#[test]
fn a_mid_marker_bisects_the_corner_it_sits_on() {
    // In along +x, out along +y: the bar points down-right, at 45 degrees.
    let image = render(&format!(
        "{}<path d=\"M10 20 L50 20 L50 60\" fill=\"none\" marker-mid=\"url(#b)\"/>",
        bar("auto")
    ));
    assert_eq!(image.alpha_at(55, 25), 255);
    assert_eq!(image.alpha_at(57, 20), 0);
    assert_eq!(image.alpha_at(50, 27), 0);
}

#[test]
fn stroke_width_units_scale_the_marker_and_overflow_is_clipped_to_its_size() {
    // Default markerUnits (strokeWidth) at stroke-width 4 make the default 3x3 marker 12x12; the content
    // runs 30 wide but is clipped to the marker's width.
    let image = render("<marker id=\"s\"><rect width=\"30\" height=\"3\" fill=\"#00ff00\"/></marker><path d=\"M20 20 L20 90\" stroke-width=\"4\" fill=\"none\" marker-start=\"url(#s)\"/>");
    assert_eq!(image.alpha_at(30, 30), 255);
    assert_eq!(image.alpha_at(36, 26), 0);
    assert_eq!(image.alpha_at(30, 34), 0);

    let visible = render("<marker id=\"s\" overflow=\"visible\"><rect width=\"30\" height=\"3\" fill=\"#00ff00\"/></marker><path d=\"M20 20 L20 90\" stroke-width=\"4\" fill=\"none\" marker-start=\"url(#s)\"/>");
    assert_eq!(visible.alpha_at(60, 26), 255);
}

#[test]
fn a_view_box_fits_the_content_to_the_marker_size() {
    let image = render("<marker id=\"v\" markerUnits=\"userSpaceOnUse\" markerWidth=\"20\" markerHeight=\"20\" viewBox=\"0 0 1 1\"><rect width=\"1\" height=\"1\" fill=\"#00ff00\"/></marker><path d=\"M40 40 L90 90\" fill=\"none\" marker-start=\"url(#v)\"/>");
    assert_eq!(image.alpha_at(58, 58), 255);
    assert_eq!(image.alpha_at(62, 50), 0);
}

#[test]
fn a_marker_without_size_or_that_uses_itself_does_not_hang() {
    let zero = render("<marker id=\"z\" markerWidth=\"0\"><rect width=\"3\" height=\"3\"/></marker><path d=\"M40 40 L90 90\" fill=\"none\" marker-start=\"url(#z)\"/>");
    assert_eq!(zero.alpha_at(41, 41), 0);

    let recursive = render("<marker id=\"r\" markerUnits=\"userSpaceOnUse\" markerWidth=\"10\" markerHeight=\"10\" overflow=\"visible\"><path d=\"M0 0 L5 5\" stroke=\"#00ff00\" marker-start=\"url(#r)\"/><rect width=\"4\" height=\"4\" fill=\"#00ff00\"/></marker><path d=\"M40 40 L90 90\" fill=\"none\" marker-start=\"url(#r)\"/>");
    assert_eq!(recursive.alpha_at(42, 42), 255);
}
