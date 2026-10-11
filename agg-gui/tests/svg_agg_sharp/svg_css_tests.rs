//! agg-sharp's `SvgCssTests` (agg/Svg, stage 5: `<style>` sheets and the
//! cascade they join), ported as render behaviour tests. C# reads the cascaded
//! attribute back from agg-sharp's own parsed document
//! (`GetElementById(..)["fill"]`); agg-gui parses with usvg, so each test draws
//! the C# elements with geometry added and asserts the colour the cascade
//! paints them. A C# `IsNull()` fill is SVG's initial fill, opaque black.

use super::common::{render, render_document, Color};

#[test]
fn a_sheet_styles_the_render() {
    let image = render_document("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><style type=\"text/css\">.g { fill: #00ff00 }</style><rect class=\"g\" width=\"100\" height=\"100\" fill=\"red\"/></svg>", 10, 10);
    assert_eq!(image.get_pixel(5, 5), Color::new(0, 255, 0, 255));
}

const BLACK: Color = Color::new(0, 0, 0, 255);

#[test]
fn type_class_and_id_selectors_apply_by_their_specificity() {
    // r, s and t side by side; t's stroke is widened by attribute so it can be sampled.
    let image = render(
        "<style>#r { fill: blue } rect.a { fill: green } .a { fill: red } rect { fill: yellow; stroke: black }</style>\
         <rect id=\"r\" class=\"a\" fill=\"gray\" x=\"5\" y=\"10\" width=\"20\" height=\"80\"/>\
         <rect id=\"s\" class=\"b a\" x=\"35\" y=\"10\" width=\"20\" height=\"80\"/>\
         <rect id=\"t\" x=\"65\" y=\"10\" width=\"30\" height=\"80\" stroke-width=\"6\"/>",
    );
    assert_eq!(image.at(15, 50), Color::new(0, 0, 255, 255));
    assert_eq!(image.at(45, 50), Color::new(0, 128, 0, 255));
    assert_eq!(image.at(80, 50), Color::new(255, 255, 0, 255));
    assert_eq!(image.at(63, 50), BLACK);
}

#[test]
fn the_later_of_two_equal_rules_wins_and_the_style_attribute_beats_both() {
    let image = render(
        "<style>.a { fill: red } .b { fill: green }</style>\
         <rect id=\"r\" class=\"b a\" width=\"50\" height=\"100\"/>\
         <rect id=\"s\" class=\"a\" style=\"fill:blue\" x=\"50\" width=\"50\" height=\"100\"/>",
    );
    assert_eq!(image.at(25, 50), Color::new(0, 128, 0, 255));
    assert_eq!(image.at(75, 50), Color::new(0, 0, 255, 255));
}

#[test]
fn important_rules_beat_the_style_attribute() {
    let image = render(
        "<style>rect { fill: green !important } #r { fill: red }</style>\
         <rect id=\"r\" style=\"fill:blue\" width=\"100\" height=\"100\"/>",
    );
    assert_eq!(image.at(50, 50), Color::new(0, 128, 0, 255));
}

#[test]
fn descendant_child_and_universal_combinators_match_their_trees() {
    // The universal stroke is widened by attribute on `lone` so it can be told from its fill.
    let image = render(
        "<style>/* a comment */ g rect { fill: green } g > circle { fill: blue } * { stroke: black } \
         @media print { rect { fill: red } } rect[x] { fill: red }</style>\
         <g><g><rect id=\"deep\" width=\"40\" height=\"40\"/><circle id=\"child\" cx=\"70\" cy=\"20\" r=\"15\"/></g></g>\
         <rect id=\"top\" x=\"60\" y=\"55\" width=\"35\" height=\"40\"/>\
         <circle id=\"lone\" cx=\"20\" cy=\"75\" r=\"15\" stroke-width=\"6\"/>",
    );
    assert_eq!(image.at(20, 20), Color::new(0, 128, 0, 255));
    assert_eq!(image.at(70, 20), Color::new(0, 0, 255, 255));
    // C# expects `top`'s fill unset because agg-sharp's SvgCss drops attribute selectors; resvg
    // matches them (resvg-test-suite structure/style/attribute-selector draws `[x]` green), so
    // `rect[x]` paints `top` red here. The `@media print` block is skipped in both.
    assert_eq!(image.at(77, 75), Color::new(255, 0, 0, 255));
    assert_eq!(image.at(20, 75), BLACK);
    // 16.5 from lone's centre: outside its radius 15, inside the 6 wide stroke.
    assert_eq!(image.at(36, 75), BLACK);
}

#[test]
fn only_presentation_properties_are_taken_from_styles() {
    // usvg applies a style's declaration only when it names a presentation attribute (plus the marker
    // shorthand): geometry such as height can be styled only in SVG 2, which it does not follow.
    let image = render(
        "<style>#s { height: 160px; fill: blue }</style>\
         <rect id=\"r\" style=\"height:160px; fill: green\" width=\"40\" height=\"20\"/>\
         <rect id=\"s\" x=\"50\" width=\"40\" height=\"20\"/>",
    );
    assert_eq!(image.at(20, 10), Color::new(0, 128, 0, 255));
    assert_eq!(image.at(20, 50).alpha, 0);
    assert_eq!(image.at(70, 10), Color::new(0, 0, 255, 255));
    assert_eq!(image.at(70, 50).alpha, 0);
}

#[test]
fn comments_in_a_style_attribute_are_ignored() {
    let image = render(
        "<rect id=\"r\" x=\"20\" y=\"20\" width=\"60\" height=\"60\" stroke-width=\"6\" \
         style=\"/*text*/fill:green/*text*/;stroke:/*a;b*/red\"/>",
    );
    assert_eq!(image.at(50, 50), Color::new(0, 128, 0, 255));
    assert_eq!(image.at(18, 50), Color::new(255, 0, 0, 255));
}
