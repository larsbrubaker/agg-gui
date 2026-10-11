//! agg-sharp's `SvgGradientTests` (agg/Svg, stage 2: linear and radial
//! gradients), ported 1:1. Renders are 100x100 of a 100x100 viewBox, so one
//! user unit is one pixel; a pixel's colour is sampled at its centre, so pixel
//! x of a 0..100 ramp sits at x + 0.5.

use super::common::{assert_within, render, Color};

const WHITE_TO_BLACK: &str =
    "<stop offset=\"0\" stop-color=\"white\"/><stop offset=\"1\" stop-color=\"black\"/>";

/// The grey level expected `t` of the way along a white-to-black ramp
/// (C#'s `(int)Math.Round(255 * (1 - t))`, banker's rounding; spelled out
/// because `f64::round_ties_even` is newer than the crate's MSRV).
fn grey(t: f64) -> i32 {
    let v = 255.0 * (1.0 - t);
    let r = v.round();
    let r = if (v - v.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - v.signum()
    } else {
        r
    };
    r as i32
}

fn red(image: &super::common::Image, x: u32, y: u32) -> i32 {
    image.at(x, y).red as i32
}

#[test]
fn linear_gradient_runs_across_the_shapes_bounding_box() {
    let image = render(&format!(
        "<linearGradient id=\"g\">{WHITE_TO_BLACK}</linearGradient><rect x=\"20\" y=\"0\" width=\"60\" height=\"100\" fill=\"url(#g)\"/>"
    ));
    for x in [20u32, 50, 79] {
        assert_within(red(&image, x, 50), grey((x as f64 + 0.5 - 20.0) / 60.0), 2);
    }
    assert_eq!(image.at(10, 50).alpha, 0);
}

#[test]
fn user_space_gradient_transform_and_pad_spread() {
    // userSpaceOnUse from x 25 to 75, then rotated a quarter turn about (50, 50): the ramp runs top to bottom.
    let image = render(&format!(
        "<linearGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" x1=\"25\" x2=\"75\" gradientTransform=\"rotate(90 50 50)\">{WHITE_TO_BLACK}</linearGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>"
    ));
    assert_eq!(red(&image, 10, 5), 255);
    assert_within(red(&image, 10, 50), grey(0.51), 2);
    assert_eq!(red(&image, 90, 95), 0);
}

#[test]
fn reflect_and_repeat_spread_beyond_the_vector() {
    let gradient = |spread: &str| {
        format!("<linearGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" x2=\"50\" spreadMethod=\"{spread}\">{WHITE_TO_BLACK}</linearGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>")
    };

    // 10.5 is a fifth of the way along the vector; 60.5 is a fifth into its second period.
    let reflect = render(&gradient("reflect"));
    assert_within(red(&reflect, 60, 50), grey(0.79), 2);
    let repeat = render(&gradient("repeat"));
    assert_within(red(&repeat, 60, 50), grey(0.21), 2);
}

#[test]
fn href_inherits_stops_and_attributes() {
    let image = render(&format!(
        "<linearGradient id=\"base\" x1=\"1\" x2=\"0\">{WHITE_TO_BLACK}</linearGradient><linearGradient id=\"g\" xlink:href=\"#base\" spreadMethod=\"pad\"/><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>"
    ));

    // Inherited x1=1 x2=0 runs the ramp right to left.
    assert!(red(&image, 2, 50) < 10, "{}", red(&image, 2, 50));
    assert!(red(&image, 97, 50) > 245, "{}", red(&image, 97, 50));
}

#[test]
fn stop_opacity_and_fill_opacity_scale_alpha() {
    let image = render("<linearGradient id=\"g\"><stop offset=\"0\" stop-color=\"red\" stop-opacity=\".5\"/><stop offset=\"1\" stop-color=\"red\" stop-opacity=\".5\"/></linearGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\" fill-opacity=\".5\"/>");
    let pixel = image.at(50, 50);
    assert_within(pixel.alpha as i32, 64, 1);
    assert!(pixel.red > 250, "{pixel:?}");
}

#[test]
fn radial_gradient_runs_from_its_centre_and_focus() {
    let centred = render(&format!(
        "<radialGradient id=\"g\">{WHITE_TO_BLACK}</radialGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>"
    ));
    assert!(red(&centred, 50, 50) > 245, "{}", red(&centred, 50, 50));
    assert_within(red(&centred, 75, 50), grey(0.51), 3);
    assert_eq!(red(&centred, 2, 2), 0);

    // With the focus at (25, 50) the white point moves there.
    let focused = render(&format!(
        "<radialGradient id=\"g\" fx=\"25%\">{WHITE_TO_BLACK}</radialGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>"
    ));
    assert!(red(&focused, 25, 50) > 245, "{}", red(&focused, 25, 50));
    assert!(red(&focused, 50, 50) < 200, "{}", red(&focused, 50, 50));
}

#[test]
fn strokes_take_gradients_too() {
    let image = render(&format!(
        "<linearGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" x2=\"100\">{WHITE_TO_BLACK}</linearGradient><line x1=\"0\" y1=\"50\" x2=\"100\" y2=\"50\" stroke=\"url(#g)\" stroke-width=\"10\"/>"
    ));
    assert_within(red(&image, 80, 50), grey(0.805), 2);
    assert_eq!(image.at(80, 50).alpha, 255);
}

#[test]
fn a_focus_outside_the_circle_leaves_the_area_behind_the_cone_unpainted() {
    // resvg's focal-point-correction case: the cone from the focus to the circle does not reach the bottom
    // right corner, so it stays unpainted rather than the focus being pulled inside the circle.
    let image = render(&format!(
        "<radialGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" cx=\"10\" cy=\"10\" r=\"75\" fx=\"83.33\" fy=\"75\">{WHITE_TO_BLACK}</radialGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>"
    ));
    assert_eq!(image.at(95, 95).alpha, 0);
    assert_eq!(red(&image, 10, 10), 0);
    assert_eq!(image.at(10, 10).alpha, 255);
}

#[test]
fn a_linear_gradient_takes_no_geometry_from_a_radial_gradient_it_links_to() {
    // y2 on the radialGradient is ignored (usvg only copies x1..y2 from a linearGradient), spreadMethod is not.
    let image = render(&format!(
        "<radialGradient id=\"r\" y2=\"1\" spreadMethod=\"reflect\"/><linearGradient id=\"g\" xlink:href=\"#r\" x2=\"0.5\">{WHITE_TO_BLACK}</linearGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>"
    ));
    assert_within(red(&image, 30, 10), red(&image, 30, 90), 1);

    // Reflected about x 50: pixel 74 (centre 74.5) mirrors pixel 25 (centre 25.5).
    assert_within(red(&image, 74, 50), red(&image, 25, 50), 2);
}

#[test]
fn stop_color_takes_current_color_and_inherit() {
    let current = render("<linearGradient id=\"g\" color=\"lime\"><stop offset=\"0\" stop-color=\"lime\"/><stop offset=\"1\" stop-color=\"currentColor\"/></linearGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>");
    assert_eq!(current.at(50, 50), Color::new(0, 255, 0, 255));

    let inherited = render("<linearGradient id=\"g\" stop-color=\"lime\"><stop offset=\"0\" stop-color=\"lime\"/><stop offset=\"1\" stop-color=\"inherit\"/></linearGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>");
    assert_eq!(inherited.at(50, 50), Color::new(0, 255, 0, 255));
}

#[test]
fn equal_offsets_at_the_start_keep_the_first_stop_for_the_padded_area() {
    // resvg's stops-with-equal-offset-5: usvg nudges the second 0 up by an epsilon, so the area padded before
    // the vector is the first stop's black while the vector itself starts at lime.
    let image = render("<linearGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" x1=\"20\" x2=\"80\"><stop offset=\"0\" stop-color=\"black\"/><stop offset=\"0\" stop-color=\"lime\"/><stop offset=\"1\" stop-color=\"lime\"/></linearGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>");
    assert_eq!(image.at(10, 50), Color::new(0, 0, 0, 255));
    assert_eq!(image.at(50, 50), Color::new(0, 255, 0, 255));
}

#[test]
#[ignore = "SVG 2's fr reaches the renderer only from usvg 0.48, whose MSRV (1.85) is above agg-gui's (1.70)"]
fn a_focal_radius_starts_the_ramp_at_its_circle() {
    // SVG 2's fr: the ramp runs from the focal circle (radius 20) out to r (50); inside it is the first stop.
    let gradient = |fr: &str| {
        format!("<radialGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" cx=\"50\" cy=\"50\" r=\"50\" fr=\"{fr}\">{WHITE_TO_BLACK}</radialGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>")
    };
    let inner = render(&gradient("20"));
    assert_eq!(red(&inner, 60, 50), 255);
    assert_within(red(&inner, 85, 50), grey((35.5 - 20.0) / 30.0), 3);

    // A focal circle larger than r: the ramp runs inward from 60 to 50, and past 60 is the first stop.
    let outer = render(&gradient("60"));
    assert_eq!(red(&outer, 50, 50), 0);
    assert_eq!(red(&outer, 1, 1), 255);
}

#[test]
fn a_server_that_cannot_paint_falls_back_to_the_colour_after_it() {
    // usvg: a gradient with no stops, or a bounding-box server on a shape with no area, is no server at all,
    // so the paint's fallback colour is used.
    let bodies = [
        "<linearGradient id=\"g\"/><rect width=\"100\" height=\"100\" fill=\"url(#g) green\"/>".to_string(),
        "<linearGradient id=\"g\"/><rect width=\"100\" height=\"100\" color=\"green\" fill=\"url(#g) currentColor\"/>".to_string(),
        format!("<linearGradient id=\"g\">{WHITE_TO_BLACK}</linearGradient><line x1=\"50\" y1=\"0\" x2=\"50\" y2=\"100\" stroke-width=\"20\" stroke=\"url(#g) green\"/>"),
        "<pattern id=\"p\"><rect width=\"50\" height=\"50\" fill=\"red\"/></pattern><line x1=\"50\" y1=\"0\" x2=\"50\" y2=\"100\" stroke-width=\"20\" stroke=\"url(#p) green\"/>".to_string(),
    ];
    for body in &bodies {
        assert_eq!(
            render(body).at(50, 50),
            Color::new(0, 128, 0, 255),
            "{body}"
        );
    }
}

#[test]
fn radial_alpha_is_the_exact_interpolation_rounded_once() {
    // resvg's lighting cases draw their height map with a radial alpha ramp, and surfaceScale magnifies any
    // level the ramp is off. Every pixel's alpha is 255 (1 - t), t its centre's distance over r, to within
    // the half level one rounding allows (a colour table quantising t, or flooring into it, lands a level low).
    let image = render("<radialGradient id=\"g\" gradientUnits=\"userSpaceOnUse\" cx=\"50\" cy=\"50\" r=\"50\"><stop offset=\"0\" stop-color=\"black\"/><stop offset=\"1\" stop-color=\"black\" stop-opacity=\"0\"/></radialGradient><rect width=\"100\" height=\"100\" fill=\"url(#g)\"/>");
    let mut worst: f64 = 0.0;
    for y in 0..100u32 {
        for x in 0..100u32 {
            let dx = x as f64 + 0.5 - 50.0;
            let dy = y as f64 + 0.5 - 50.0;
            let t = f64::min(1.0, (dx * dx + dy * dy).sqrt() / 50.0);
            worst = worst.max((image.at(x, y).alpha as f64 - 255.0 * (1.0 - t)).abs());
        }
    }
    assert!(worst <= 0.51, "worst {worst}");
}
