//! SVG image and pattern integration tests.

use std::path::Path;

use base64::Engine;

use super::*;

#[test]
fn embedded_svg_image_preserves_left_to_right_orientation() {
    let embedded = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="2" height="1">
            <rect x="0" y="0" width="1" height="1" fill="#ff0000"/>
            <rect x="1" y="0" width="1" height="1" fill="#0000ff"/>
        </svg>
    "##;
    let encoded = base64::engine::general_purpose::STANDARD.encode(embedded);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg"
                  xmlns:xlink="http://www.w3.org/1999/xlink"
                  width="4" height="2">
                <image x="0" y="0" width="4" height="2"
                       xlink:href="data:image/svg+xml;base64,{encoded}"/>
              </svg>"##
    );

    let fb = render_svg_to_framebuffer(svg.as_bytes()).expect("SVG image should render");
    let left = ((fb.width() + 0) * 4) as usize;
    let right = ((fb.width() + 3) * 4) as usize;
    assert!(fb.pixels()[left] > fb.pixels()[left + 2]);
    assert!(fb.pixels()[right + 2] > fb.pixels()[right]);
}

#[test]
fn external_raster_image_resolves_from_resources_dir() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agg-gui crate should live under workspace root");
    let svg_path = workspace.join(
        "tests/resvg-test-suite/tests/structure/image/raster-image-and-size-with-odd-numbers.svg",
    );
    let svg = std::fs::read(&svg_path).expect("test SVG should exist");
    let resources_dir = svg_path.parent().expect("test SVG should have parent");

    let fb = render_svg_to_framebuffer_at_size_with_resources(&svg, 80, 80, resources_dir)
        .expect("external image should resolve");

    let has_image_pixels = fb
        .pixels()
        .chunks_exact(4)
        .any(|px| px[3] > 0 && (px[0] > 64 || px[1] > 64 || px[2] > 64));
    assert!(
        has_image_pixels,
        "external raster image should draw non-empty pixels"
    );
}

#[test]
fn object_bounding_box_pattern_gets_non_tiny_tile() {
    let svg = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
            <pattern id="p" width="0.5" height="0.5" viewBox="0 0 10 10">
                <rect x="0" y="0" width="10" height="10" fill="#00ff00"/>
            </pattern>
            <rect x="2" y="2" width="16" height="16" fill="url(#p)"/>
        </svg>
    "##;

    let fb = render_svg_to_framebuffer(svg).expect("pattern should render");
    let colored_pixels = fb
        .pixels()
        .chunks_exact(4)
        .filter(|px| px[3] > 0 && (px[1] > 0 || px[2] > 0))
        .count();
    assert!(
        colored_pixels > 64,
        "pattern fill should cover a visible area"
    );
}
