//! agg-sharp's `SvgImageTests` (agg/Svg, stage 3: the `<image>` element),
//! ported 1:1. Renders are 100x100 of a 100x100 viewBox, so one user unit is
//! one pixel, and `at` takes SVG's y-down pixel coordinates.
//!
//! The C# class hands its 2x2 raster (red, blue on the top row, green, white
//! below) out through a test-supplied `ImageDecoder` for a placeholder
//! `data:image/png;base64,AAAA`; usvg decodes images itself, so here the
//! data URL carries that raster as a real PNG, and a placeholder that is not
//! a PNG stands for "no decoder". C#'s `ResourceResolver` is usvg's
//! `resources_dir`: the external image is a file the href reaches from it,
//! as written.
//!
//! `pixelated_image_rendering_is_not_smoothed` is ignored: usvg drops
//! `image-rendering="pixelated"` given as an attribute (it keeps the CSS
//! values only from styles); `rust_only_pixelated_image_rendering_in_a_style_is_not_smoothed`
//! checks the renderer with the value in a style.

use super::common::{data_url, png_bytes, Color, Image};
use agg_gui::framebuffer::unpremultiply_rgba_inplace;

/// The 2x2 raster: red, blue on its top row, green, white below.
fn quad_png() -> Vec<u8> {
    png_bytes(
        2,
        2,
        &[
            255, 0, 0, 255, 0, 0, 255, 255, //
            0, 255, 0, 255, 255, 255, 255, 255,
        ],
    )
}

fn any_raster() -> String {
    data_url("image/png", &quad_png())
}

fn document(body: &str) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"0 0 100 100\">{body}</svg>")
}

fn to_image(fb: agg_gui::Framebuffer) -> Image {
    let mut pixels = fb.pixels().to_vec();
    unpremultiply_rgba_inplace(&mut pixels);
    Image {
        width: fb.width(),
        height: fb.height(),
        pixels,
    }
}

fn render(body: &str) -> Image {
    to_image(
        agg_gui::render_svg_to_framebuffer_at_size(document(body).as_bytes(), 100, 100)
            .expect("the SVG renders"),
    )
}

fn is(c: Color, red: u8, green: u8, blue: u8) -> bool {
    c == Color::new(red, green, blue, 255)
}

/// C#'s `Uri.EscapeDataString` for the ASCII SVG text these tests embed.
fn escape_data_string(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[test]
fn raster_is_centred_and_scaled_to_meet_its_viewport() {
    // 2x2 met into 100x50 is 50x50, centred at x 25..75.
    let image = render(&format!(
        "<image x=\"0\" y=\"0\" width=\"100\" height=\"50\" xlink:href=\"{}\"/>",
        any_raster()
    ));
    assert!(is(image.at(30, 5), 255, 0, 0), "{:?}", image.at(30, 5));
    assert!(is(image.at(70, 5), 0, 0, 255), "{:?}", image.at(70, 5));
    assert!(is(image.at(30, 45), 0, 255, 0), "{:?}", image.at(30, 45));
    assert_eq!(image.at(10, 25).alpha, 0);
    assert_eq!(image.at(50, 60).alpha, 0);
}

#[test]
fn preserve_aspect_ratio_none_stretches_and_slice_covers() {
    let none = render(&format!(
        "<image width=\"100\" height=\"50\" preserveAspectRatio=\"none\" xlink:href=\"{}\"/>",
        any_raster()
    ));
    assert!(is(none.at(2, 2), 255, 0, 0), "{:?}", none.at(2, 2));
    assert!(is(none.at(97, 47), 255, 255, 255), "{:?}", none.at(97, 47));

    // Sliced, 2x2 covers 100x50 at 100x100, its middle showing: the top row's lower half and the bottom's upper
    // (their centres fall on the viewport's edges, so smoothing tints the pixels beside them a little).
    let slice = render(&format!(
        "<image width=\"100\" height=\"50\" preserveAspectRatio=\"xMidYMid slice\" xlink:href=\"{}\"/>",
        any_raster()
    ));
    assert!(slice.at(2, 2).red > 240, "{:?}", slice.at(2, 2));
    assert!(slice.at(2, 47).green > 240, "{:?}", slice.at(2, 47));
    assert_eq!(slice.at(2, 47).alpha, 255);
    assert_eq!(slice.at(50, 60).alpha, 0);
}

#[test]
fn missing_size_is_the_images_own() {
    let image = render(&format!(
        "<image x=\"10\" y=\"10\" xlink:href=\"{}\"/>",
        any_raster()
    ));
    assert!(is(image.at(10, 10), 255, 0, 0), "{:?}", image.at(10, 10));
    assert!(
        is(image.at(11, 11), 255, 255, 255),
        "{:?}",
        image.at(11, 11)
    );
    assert_eq!(image.at(12, 12).alpha, 0);
}

#[test]
#[ignore = "usvg 0.45 drops image-rendering's CSS values (smooth, high-quality, crisp-edges, pixelated) given as an XML attribute (svgtree/parse.rs), so the image arrives as optimizeQuality; rust_only_pixelated_image_rendering_in_a_style_is_not_smoothed covers the renderer"]
fn pixelated_image_rendering_is_not_smoothed() {
    // 2x2 over 100x100: x 45 is inside the red texel but near enough blue that smoothing blends it.
    let smooth = render(&format!(
        "<image width=\"100\" height=\"100\" xlink:href=\"{}\"/>",
        any_raster()
    ));
    let pixelated = render(&format!(
        "<g image-rendering=\"pixelated\"><image width=\"100\" height=\"100\" xlink:href=\"{}\"/></g>",
        any_raster()
    ));
    assert!(
        is(pixelated.at(45, 10), 255, 0, 0),
        "{:?}",
        pixelated.at(45, 10)
    );
    assert!(smooth.at(45, 10).blue > 0, "{:?}", smooth.at(45, 10));
}

/// The same check with `pixelated` in a style, where usvg keeps it.
#[test]
fn rust_only_pixelated_image_rendering_in_a_style_is_not_smoothed() {
    let smooth = render(&format!(
        "<image width=\"100\" height=\"100\" xlink:href=\"{}\"/>",
        any_raster()
    ));
    let pixelated = render(&format!(
        "<g style=\"image-rendering:pixelated\"><image width=\"100\" height=\"100\" xlink:href=\"{}\"/></g>",
        any_raster()
    ));
    assert!(
        is(pixelated.at(45, 10), 255, 0, 0),
        "{:?}",
        pixelated.at(45, 10)
    );
    assert!(smooth.at(45, 10).blue > 0, "{:?}", smooth.at(45, 10));
}

#[test]
fn without_a_decoder_or_resolver_nothing_is_drawn() {
    let image = render("<image width=\"100\" height=\"100\" xlink:href=\"data:image/png;base64,AAAA\"/><image width=\"100\" height=\"100\" xlink:href=\"pic.png\"/>");
    assert_eq!(image.at(50, 50).alpha, 0);
}

#[test]
fn external_href_is_read_through_the_resolver_as_written() {
    // The href, as written, from the resources directory: <root>/svg/../images/pic.png.
    let root = std::env::temp_dir().join(format!("agg-gui-svg-image-{}", std::process::id()));
    std::fs::create_dir_all(root.join("svg")).expect("the resources directory is created");
    std::fs::create_dir_all(root.join("images")).expect("the images directory is created");
    std::fs::write(root.join("images/pic.png"), quad_png()).expect("the image is written");
    let fb = agg_gui::render_svg_to_framebuffer_at_size_with_resources(
        document("<image width=\"100\" height=\"100\" xlink:href=\"../images/pic.png\"/>")
            .as_bytes(),
        100,
        100,
        &root.join("svg"),
    );
    let _ = std::fs::remove_dir_all(&root);
    let image = to_image(fb.expect("the SVG renders"));
    assert!(is(image.at(5, 5), 255, 0, 0), "{:?}", image.at(5, 5));
}

#[test]
fn svg_image_is_drawn_as_a_vector_document() {
    // A percent-encoded (not base64) SVG with a 10x10 viewBox and a red left half, met into 50x50 at (50, 50).
    let nested = escape_data_string("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><rect width='5' height='10' fill='red'/></svg>");
    let image = render(&format!("<image x=\"50\" y=\"50\" width=\"50\" height=\"50\" xlink:href=\"data:image/svg+xml,{nested}\"/>"));
    assert!(is(image.at(60, 75), 255, 0, 0), "{:?}", image.at(60, 75));
    assert_eq!(image.at(90, 75).alpha, 0);
    assert_eq!(image.at(20, 75).alpha, 0);
}

#[test]
fn svg_image_is_clipped_to_its_viewport() {
    // A red square three times the nested viewBox, centred on it: only the image's own 50x50 at (50, 50) shows.
    let nested = escape_data_string("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><rect x='-10' y='-10' width='30' height='30' fill='red'/></svg>");
    let image = render(&format!("<image x=\"50\" y=\"50\" width=\"50\" height=\"50\" xlink:href=\"data:image/svg+xml,{nested}\"/>"));
    assert!(is(image.at(50, 50), 255, 0, 0), "{:?}", image.at(50, 50));
    assert!(is(image.at(99, 99), 255, 0, 0), "{:?}", image.at(99, 99));
    assert_eq!(image.at(49, 75).alpha, 0);
    assert_eq!(image.at(75, 49).alpha, 0);
}

#[test]
fn an_svg_image_draws_no_images_of_its_own() {
    // As in usvg, an SVG image's own <image> elements are not drawn (resvg's recursive-2: a document including
    // itself shows once, with an empty frame inside). The nested document's red rect shows; its raster does not.
    let nested = escape_data_string(&format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'><rect width='5' height='10' fill='red'/><image x='5' width='5' height='10' href='{}'/></svg>", any_raster()));
    let image = render(&format!(
        "<image width=\"100\" height=\"100\" xlink:href=\"data:image/svg+xml,{nested}\"/>"
    ));
    assert!(is(image.at(25, 50), 255, 0, 0), "{:?}", image.at(25, 50));
    assert_eq!(image.at(75, 50).alpha, 0);
}
