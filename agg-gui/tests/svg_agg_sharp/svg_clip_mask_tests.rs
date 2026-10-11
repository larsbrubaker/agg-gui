//! agg-sharp's `SvgClipMaskTests` (agg/Svg, stage 5: clip-path and mask),
//! ported 1:1. Renders are 100x100 of a 100x100 viewBox, one user unit per pixel.

use super::common::render;

const GREEN: &str = "<rect width=\"100\" height=\"100\" fill=\"#00ff00\" {0}/>";

fn filled(attributes: &str) -> String {
    GREEN.replace("{0}", attributes)
}

#[test]
fn a_clip_path_keeps_only_what_its_shapes_cover() {
    let image = render(&format!(
        "<clipPath id=\"c\"><rect x=\"20\" y=\"20\" width=\"30\" height=\"30\"/><circle cx=\"80\" cy=\"80\" r=\"10\"/></clipPath>{}",
        filled("clip-path=\"url(#c)\"")
    ));
    assert_eq!(image.alpha_at(35, 35), 255);
    assert_eq!(image.alpha_at(80, 80), 255);
    assert_eq!(image.alpha_at(10, 10), 0);
    assert_eq!(image.alpha_at(60, 35), 0);
}

#[test]
fn clip_rule_transforms_and_bounding_box_units_shape_the_clip() {
    // evenodd leaves the inner square a hole; the clip's transform moves it right by 50.
    let even_odd = render(&format!(
        "<clipPath id=\"c\" transform=\"translate(50 0)\"><path clip-rule=\"evenodd\" d=\"M0 0H40V40H0Z M10 10H30V30H10Z\"/></clipPath>{}",
        filled("clip-path=\"url(#c)\"")
    ));
    assert_eq!(even_odd.alpha_at(55, 5), 255);
    assert_eq!(even_odd.alpha_at(70, 20), 0);
    assert_eq!(even_odd.alpha_at(5, 5), 0);

    // objectBoundingBox: the clip's unit square is the shape's box (here x 50..100, y 0..50).
    let bbox = render("<clipPath id=\"c\" clipPathUnits=\"objectBoundingBox\"><rect width=\".5\" height=\".5\"/></clipPath><rect x=\"50\" width=\"50\" height=\"50\" fill=\"#00ff00\" clip-path=\"url(#c)\"/>");
    assert_eq!(bbox.alpha_at(60, 10), 255);
    assert_eq!(bbox.alpha_at(90, 10), 0);
    assert_eq!(bbox.alpha_at(60, 40), 0);
}

#[test]
fn nested_clip_paths_intersect() {
    let image = render(&format!(
        "<clipPath id=\"outer\"><rect width=\"50\" height=\"100\"/></clipPath><clipPath id=\"inner\" clip-path=\"url(#outer)\"><rect width=\"100\" height=\"50\"/></clipPath>{}",
        filled("clip-path=\"url(#inner)\"")
    ));
    assert_eq!(image.alpha_at(25, 25), 255);
    assert_eq!(image.alpha_at(75, 25), 0);
    assert_eq!(image.alpha_at(25, 75), 0);
}

#[test]
fn a_missing_clip_is_ignored() {
    // One that clips itself is covered by a_recursive_clip_or_mask_link_is_dropped.
    assert_eq!(
        render(&filled("clip-path=\"url(#missing)\"")).alpha_at(50, 50),
        255
    );
}

#[test]
fn a_mask_scales_by_its_luminance_within_its_region() {
    // White keeps everything, black nothing; the default region is the box grown by 10% each side.
    let image = render("<mask id=\"m\"><rect width=\"50\" height=\"100\" fill=\"white\"/><rect x=\"50\" width=\"50\" height=\"100\" fill=\"black\"/></mask><rect x=\"10\" y=\"10\" width=\"80\" height=\"80\" fill=\"#00ff00\" mask=\"url(#m)\"/>");
    assert_eq!(image.alpha_at(30, 50), 255);
    assert_eq!(image.alpha_at(70, 50), 0);

    // A grey of luminance 50% halves the alpha; mask-type alpha uses the mask's alpha instead.
    let grey = render(&format!(
        "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"rgb(128,128,128)\"/></mask>{}",
        filled("mask=\"url(#m)\"")
    ));
    assert_eq!(grey.alpha_at(50, 50), 128);
    let alpha = render(&format!(
        "<mask id=\"m\" mask-type=\"alpha\"><rect width=\"100\" height=\"100\" fill=\"black\" fill-opacity=\".5\"/></mask>{}",
        filled("mask=\"url(#m)\"")
    ));
    assert_eq!(alpha.alpha_at(50, 50), 128);
}

#[test]
fn mask_units_and_content_units_place_the_mask() {
    // userSpaceOnUse region x 0..50: the white content only counts there.
    let region = render(&format!(
        "<mask id=\"m\" maskUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"50\" height=\"100\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>{}",
        filled("mask=\"url(#m)\"")
    ));
    assert_eq!(region.alpha_at(25, 50), 255);
    assert_eq!(region.alpha_at(75, 50), 0);

    // objectBoundingBox content: a unit-square half is half the shape's box.
    let content = render("<mask id=\"m\" maskContentUnits=\"objectBoundingBox\"><rect width=\".5\" height=\"1\" fill=\"white\"/></mask><rect x=\"20\" y=\"20\" width=\"60\" height=\"60\" fill=\"#00ff00\" mask=\"url(#m)\"/>");
    assert_eq!(content.alpha_at(30, 50), 255);
    assert_eq!(content.alpha_at(70, 50), 0);
}

#[test]
fn clip_paths_apply_to_groups() {
    let image = render("<clipPath id=\"c\"><rect width=\"50\" height=\"50\"/></clipPath><g clip-path=\"url(#c)\" transform=\"translate(10 10)\"><rect width=\"100\" height=\"100\" fill=\"#00ff00\"/></g>");
    assert_eq!(image.alpha_at(30, 30), 255);
    assert_eq!(image.alpha_at(70, 30), 0);
    assert_eq!(image.alpha_at(5, 5), 0);
}

#[test]
fn a_recursive_clip_or_mask_link_is_dropped() {
    // usvg sets a clip-path (or mask) that leads back to its own clipPath (or mask) to none, so the rest of
    // the clip still applies.
    let cases = [
        ("<clipPath id=\"c\" clip-path=\"url(#c)\"><rect x=\"20\" y=\"20\" width=\"60\" height=\"60\"/></clipPath>", "clip-path=\"url(#c)\""),
        ("<clipPath id=\"c\"><rect x=\"20\" y=\"20\" width=\"60\" height=\"60\" clip-path=\"url(#c)\"/></clipPath>", "clip-path=\"url(#c)\""),
        ("<clipPath id=\"c\"><rect x=\"20\" y=\"20\" width=\"60\" height=\"60\" clip-path=\"url(#d)\"/></clipPath><clipPath id=\"d\"><rect width=\"100\" height=\"100\" clip-path=\"url(#c)\"/></clipPath>", "clip-path=\"url(#c)\""),
        ("<mask id=\"m\" mask=\"url(#m)\"><rect x=\"20\" y=\"20\" width=\"60\" height=\"60\" fill=\"white\"/></mask>", "mask=\"url(#m)\""),
    ];
    for (defs, reference) in cases {
        let image = render(&format!("{defs}{}", filled(reference)));
        assert_eq!(image.alpha_at(50, 50), 255, "{defs}");
        assert_eq!(image.alpha_at(10, 10), 0, "{defs}");
    }
}
