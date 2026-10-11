//! agg-sharp's `SvgParsingTests` (agg/Svg's reading of a document: the XML
//! around the SVG and the value syntaxes it parses), ported as render tests.
//! C# reads agg-sharp's own document model and `SvgColor`; agg-gui parses with
//! usvg, so the entity and colour cases draw the parsed value and assert its
//! pixels. C#'s `ResourceResolver` is usvg's `resources_dir`.

use super::common::{data_url, render, render_document, Color};
use agg_gui::framebuffer::unpremultiply_rgba_inplace;

const BLUE_SQUARE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\" fill=\"blue\"/></svg>";

/// IEEE CRC-32, as gzip's trailer carries it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// `text` as a gzip stream of one stored (uncompressed) deflate block; C#
/// compresses, but any valid gzip stream is an svgz.
fn gzip(text: &str) -> Vec<u8> {
    let data = text.as_bytes();
    let len = u16::try_from(data.len()).expect("one stored block holds the text");
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    out.push(1); // BFINAL, stored
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// The centre pixel of a 100x100 render of a document holding one 100x100
/// `<image>` of `href`, resolved from `resources_dir` when given.
fn centre(href: &str, resources_dir: Option<&std::path::Path>) -> Color {
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"0 0 100 100\"><image width=\"100\" height=\"100\" xlink:href=\"{href}\"/></svg>");
    let fb = match resources_dir {
        Some(dir) => {
            agg_gui::render_svg_to_framebuffer_at_size_with_resources(svg.as_bytes(), 100, 100, dir)
        }
        None => agg_gui::render_svg_to_framebuffer_at_size(svg.as_bytes(), 100, 100),
    }
    .expect("the SVG renders");
    let mut pixels = fb.pixels().to_vec();
    unpremultiply_rgba_inplace(&mut pixels);
    let i = ((50 * fb.width() + 50) * 4) as usize;
    Color::new(pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3])
}

#[test]
fn an_embedded_svgz_image_is_unzipped() {
    // structure/image/embedded-svgz
    assert_eq!(
        centre(&data_url("image/svg+xml", &gzip(BLUE_SQUARE)), None),
        Color::new(0, 0, 255, 255)
    );
}

#[test]
fn an_external_svgz_image_is_unzipped() {
    // structure/image/external-svgz
    let dir = std::env::temp_dir().join(format!("agg-gui-svgz-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the resources directory is created");
    std::fs::write(dir.join("image.svgz"), gzip(BLUE_SQUARE)).expect("the svgz is written");
    let colour = centre("image.svgz", Some(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(colour, Color::new(0, 0, 255, 255));
}

#[test]
fn an_svg_image_that_is_not_xml_draws_nothing() {
    // usvg skips an image it cannot parse; the document around it still draws.
    assert_eq!(
        centre(&data_url("image/svg+xml", b"<svg><rect></svg>"), None).alpha,
        0
    );
}

const DOCTYPE: &str = "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1 Basic//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11-basic.dtd\" [\n";

#[test]
fn an_internal_entity_expands_in_an_attribute_value() {
    // structure/svg/attribute-value-via-ENTITY-reference
    let image = render_document(
        &format!("{DOCTYPE}<!ENTITY fill_value \"green\">\n]>\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><rect id=\"r\" width=\"100\" height=\"100\" fill=\"&fill_value;\"/></svg>"),
        100,
        100,
    );
    assert_eq!(image.at(50, 50), Color::new(0, 128, 0, 255));
}

#[test]
fn an_internal_entity_expands_to_elements_each_time_it_is_used() {
    // structure/svg/elements-via-ENTITY-reference-2, with the geometry a render needs: each use
    // draws the 20 wide rect and the circle, in its own group.
    let image = render_document(
        &format!("{DOCTYPE}<!ENTITY Rect \"\n<rect width='20' height='20'/>\n<circle cx='35' cy='10' r='5'/>\n\">\n]>\n<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><g id=\"g1\">&Rect;</g><g id=\"g2\" transform=\"translate(50 50)\">&Rect;</g></svg>"),
        100,
        100,
    );
    for (dx, dy) in [(0, 0), (50, 50)] {
        assert_eq!(image.at(dx + 10, dy + 10).alpha, 255, "rect at +{dx}");
        assert_eq!(image.at(dx + 19, dy + 10).alpha, 255, "rect width at +{dx}");
        assert_eq!(image.at(dx + 25, dy + 10).alpha, 0, "gap at +{dx}");
        assert_eq!(image.at(dx + 35, dy + 10).alpha, 255, "circle at +{dx}");
    }
}

/// `fill` drawn over the whole 100x100 render, its centre pixel.
fn fill(colour: &str) -> Color {
    render(&format!(
        "<rect width=\"100\" height=\"100\" fill=\"{colour}\"/>"
    ))
    .at(50, 50)
}

#[test]
fn colours_parse_as_resvg_renders_them() {
    // C# expects 127 for every 50% channel, as resvg-test-suite's reference PNGs hold; usvg's
    // svgtypes rounds 127.5 half up to 128 (its own rgb_percentage test pins 128), as CSS Color 4's
    // rounding toward +infinity does; resvg passes those references within its tolerance.
    for (text, expected) in [
        ("rgb(0%, 50%, 0%)", Color::new(0, 128, 0, 255)), // painting/fill/rgb-color-with-percentage-values
        ("RGB(0%, 50%, 0%)", Color::new(0, 128, 0, 255)), // painting/fill/uppercase-rgb-color
        ("rgb(-10%, 50%, 120%)", Color::new(0, 128, 255, 255)), // painting/fill/rgb-color-with-percentage-overflow
        ("rgb(0%, 45.5%, 0%)", Color::new(0, 116, 0, 255)),
    ] {
        assert_eq!(fill(text), expected, "{text}");
    }
    // painting/fill/transparent: nothing is drawn.
    assert_eq!(fill("transparent").alpha, 0);
}

#[test]
fn an_rgb_colour_mixing_numbers_after_a_number_with_percentages_is_invalid() {
    // painting/fill/rgba-0-50percent-0-0.5: the first channel being a number makes the rest numbers,
    // so the fill is invalid and the rect takes the initial fill, opaque black, as resvg's reference does.
    assert_eq!(fill("rgba(0, 50%, 0, 0.5)"), Color::new(0, 0, 0, 255));
}
