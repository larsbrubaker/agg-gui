//! SVG rendering checks for `render_svg_to_framebuffer_at_size`: the even-odd
//! fill rule (MatterCAD's `SvgRenderingTests`, ported 1:1 against agg-gui's
//! renderer, which plays the part of agg-sharp's `SvgDocument.RenderToImage`)
//! and full coverage reaching alpha 255 (MatterCAD's
//! `AppliedIconSvgTests.InternetIconDrawsBlackUntinted` draws `internet.svg`
//! at 16x16 and needs opaque ink, as agg-sharp produces).

use agg_gui::render_svg_to_framebuffer_at_size;

/// A 16x16 "none" glyph: one compound path, a circle plus the two arms of an X.
/// A fixture for the even-odd fill rule only.
const NONE_GLYPH_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  <path fill="#000000" fill-rule="evenodd" d="M 8 1 a 7 7 0 1 0 0.0001 0 Z M 4.3 5.7 L 5.7 4.3 L 11.7 10.3 L 10.3 11.7 Z M 10.3 4.3 L 11.7 5.7 L 5.7 11.7 L 4.3 10.3 Z"/>
</svg>"##;

/// MatterCAD's `static-data/Icons/internet.svg` (original MatterCAD artwork,
/// Lars Brubaker / MatterHackers, BSD-2), copied verbatim as a fixture.
const INTERNET_SVG: &str = include_str!("fixtures/internet.svg");

/// RGBA of the pixel at (x, y) in the framebuffer's Y-up coordinates.
fn pixel(fb: &agg_gui::Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

/// Verifies that the none glyph renders as a black circle with transparent X
/// cutouts through the center. Where the arms overlap the circle, the even-odd
/// rule makes those pixels transparent.
#[test]
fn none_icon_renders_black_circle_with_transparent_x() {
    // Render at 32x32 (SVG is 16x16, so scale factor is 2). The framebuffer is
    // Y-up with SVG y pointing down, so C#'s At(x, y) == SVG(x/2, y/2) reads
    // row height - 1 - y, as C# does on its Y-up image.
    let fb = render_svg_to_framebuffer_at_size(NONE_GLYPH_SVG.as_bytes(), 32, 32)
        .expect("none glyph renders");
    let at = |x: u32, y: u32| pixel(&fb, x, fb.height() - 1 - y);

    // Outside the circle should be transparent (alpha 0).
    assert_eq!(at(0, 0)[3], 0);

    // Inside circle, left side away from both X arms: solid black.
    let black = at(4, 16);
    assert!(black[0] < 50, "red {}", black[0]);
    assert!(black[3] > 200, "alpha {}", black[3]);

    // Inside circle, inside one X arm (but not both): even-odd cutout.
    let cutout = at(10, 10);
    assert!(cutout[3] < 50, "alpha {}", cutout[3]);
}

/// Fully covered pixels of a black icon are opaque black (alpha 255), as
/// agg-sharp draws them; anti-aliased edges stay black with partial alpha.
#[test]
fn rust_only_internet_icon_full_coverage_reaches_alpha_255() {
    let fb = render_svg_to_framebuffer_at_size(INTERNET_SVG.as_bytes(), 16, 16)
        .expect("internet.svg renders");
    let drawn: Vec<[u8; 4]> = fb
        .pixels()
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]])
        .filter(|p| p[3] > 0)
        .collect();
    assert!(drawn.len() > 20, "drawn {}", drawn.len());
    let max_alpha = drawn.iter().map(|p| p[3]).max().unwrap_or(0);
    assert_eq!(max_alpha, 255, "the globe's ring must draw opaque ink");
    assert!(drawn.iter().all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
}

/// A rectangle covering whole pixels draws them at alpha 255.
#[test]
fn rust_only_pixel_aligned_rect_is_fully_opaque() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect x="2" y="2" width="4" height="4" fill="#000"/></svg>"##;
    let fb = render_svg_to_framebuffer_at_size(svg.as_bytes(), 8, 8).expect("rect renders");
    assert_eq!(pixel(&fb, 3, 3), [0, 0, 0, 255]);
    assert_eq!(pixel(&fb, 0, 0)[3], 0);
}

/// A circle drawn far larger than its viewBox is flattened to half a device
/// pixel, not half a viewBox unit: a point just inside the true circle midway
/// between flattening vertices is still fully covered.
#[test]
fn rust_only_upscaled_circle_is_not_faceted() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><circle cx="1" cy="1" r="1" fill="#000"/></svg>"##;
    let fb = render_svg_to_framebuffer_at_size(svg.as_bytes(), 400, 400).expect("circle renders");
    // Radius 200 px; sample pixels whose far corner is 3 px inside the true
    // edge, every 5 degrees round the circle.
    for step in 0..72 {
        let angle = (step as f64) * 5.0_f64.to_radians();
        let r = 200.0 - 3.0 - std::f64::consts::SQRT_2;
        let x = (200.0 + r * angle.cos()).floor() as u32;
        let y = (200.0 + r * angle.sin()).floor() as u32;
        assert_eq!(
            pixel(&fb, x, y)[3],
            255,
            "pixel ({x}, {y}) at {} degrees",
            step * 5
        );
    }
}
