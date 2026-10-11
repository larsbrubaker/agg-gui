//! agg-sharp's `SvgParsingTests` (agg/Svg's reading of a document: the XML
//! around the SVG and the value syntaxes it parses), ported 1:1 where they
//! render: the gzip-compressed (svgz) image cases.
//!
//! The parse-only tests (`AnInternalEntityExpandsInAnAttributeValue`,
//! `AnInternalEntityExpandsToElementsEachTimeItIsUsed`,
//! `ColoursParseAsResvgRendersThem`,
//! `AnRgbColourMixingNumbersAfterANumberWithPercentagesIsInvalid`) exercise
//! agg-sharp's own document model and `SvgColor`; agg-gui parses with usvg,
//! so they have no counterpart here. C#'s `ResourceResolver` is usvg's
//! `resources_dir`.

use super::common::{data_url, Color};
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
