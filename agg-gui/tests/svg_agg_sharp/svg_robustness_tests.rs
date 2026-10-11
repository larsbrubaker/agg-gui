//! agg-sharp's `SvgRobustnessTests` (input that used to crash or hang agg/Svg:
//! each case is the smallest form of a resvg-test-suite case that threw or
//! timed out), ported 1:1. Renders are 100x100 of a 100x100 viewBox, so one
//! user unit is one pixel; `at` takes SVG's y-down pixel coordinates.
//!
//! The C# class's test-supplied decoder for the image case is a real PNG in
//! the data URL here (usvg decodes images itself).
//!
//! `morphology_with_a_huge_radius_is_quick` is ignored until agg-gui's SVG
//! renderer draws filter effects (`<filter>` is not rendered at all yet).

use super::common::{data_url, png_bytes, render, Color};

const BLUE: Color = Color::new(0, 0, 255, 255);

#[test]
fn a_gradient_fill_reaching_off_the_canvas_draws() {
    // painting/context/with-gradient-in-use: a paint-server fill whose shape runs past the canvas edges
    // indexed the target outside its rows.
    let image = render("<linearGradient id=\"lg\"><stop offset=\"0\" stop-color=\"blue\"/><stop offset=\"1\" stop-color=\"blue\"/></linearGradient><rect x=\"-50\" y=\"-50\" width=\"200\" height=\"200\" fill=\"url(#lg)\"/>");
    assert_eq!(image.at(50, 50), BLUE);
    assert_eq!(image.at(0, 99), BLUE);
}

#[test]
fn an_image_reaching_off_the_canvas_draws() {
    // painting/marker/with-an-image-child and structure/image/no-width-and-height-on-svg: an <image> whose
    // viewport runs past the canvas edges.
    let raster = data_url("image/png", &png_bytes(2, 2, &[0, 0, 255, 255].repeat(4)));
    let image = render(&format!(
        "<image x=\"-50\" y=\"-50\" width=\"200\" height=\"200\" xlink:href=\"{raster}\"/>"
    ));
    assert_eq!(image.at(50, 50), BLUE);
    assert_eq!(image.at(0, 99), BLUE);
}

#[test]
#[ignore = "agg-gui's SVG renderer does not draw filter effects yet; the filter step ports them"]
fn morphology_with_a_huge_radius_is_quick() {
    // filters/feMorphology/huge-radius timed out: each pixel walked all 2r + 1 pixels of its window.
    let filter = |operator: &str| {
        format!("<filter id=\"f\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"100\" height=\"100\"><feMorphology operator=\"{operator}\" radius=\"99999\"/></filter><rect x=\"40\" y=\"40\" width=\"20\" height=\"20\" fill=\"blue\" filter=\"url(#f)\"/>")
    };
    let watch = std::time::Instant::now();
    let dilated = render(&filter("dilate"));
    let eroded = render(&filter("erode"));
    let elapsed = watch.elapsed();

    // A window wider than the region takes in the whole region, and the transparency around it.
    assert_eq!(dilated.at(5, 95), BLUE);
    assert_eq!(eroded.at(50, 50).alpha, 0);
    assert!(elapsed.as_secs_f64() < 2.0, "{elapsed:?}");
}
