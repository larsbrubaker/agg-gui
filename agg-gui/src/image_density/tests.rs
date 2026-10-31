//! Tests for `image_density.rs`: the density a picture file says, read from
//! its bytes, and the PNG writer that says one.

use super::*;

/// A 4x2 PNG with a `pHYs` chunk of `dims`, or none.
fn png_with(dims: Option<png::PixelDimensions>) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 4, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_pixel_dims(dims);
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(&[200u8; 4 * 2 * 4]).expect("data");
    }
    out
}

fn per_metre(value: u32, unit: png::Unit) -> Option<png::PixelDimensions> {
    Some(png::PixelDimensions {
        xppu: value,
        yppu: value,
        unit,
    })
}

#[test]
fn a_144_ppi_png_says_two_pixels_per_point() {
    assert_eq!(
        image_pixel_density(&png_with(per_metre(5669, png::Unit::Meter))),
        2
    );
}

#[test]
fn ordinary_screen_resolutions_and_no_chunk_read_as_one() {
    // 72 ppi and 96 ppi: what pictures saved without a thought for density say.
    assert_eq!(
        image_pixel_density(&png_with(per_metre(2835, png::Unit::Meter))),
        1
    );
    assert_eq!(
        image_pixel_density(&png_with(per_metre(3780, png::Unit::Meter))),
        1
    );
    assert_eq!(image_pixel_density(&png_with(None)), 1);
    // An aspect ratio only (unit unknown) says nothing about density.
    assert_eq!(
        image_pixel_density(&png_with(per_metre(5669, png::Unit::Unspecified))),
        1
    );
}

#[test]
fn print_resolutions_read_as_one() {
    // 120, 150 and 300 dpi from an image editor describe paper, not a screen.
    for ppm in [4724, 5906, 11811, 23622] {
        assert_eq!(
            image_pixel_density(&png_with(per_metre(ppm, png::Unit::Meter))),
            1,
            "{ppm} px/m"
        );
    }
    // Just outside 1 ppi of 144 (146 ppi) is no screen density either.
    assert_eq!(density_from_pixels_per_metre(5748, 5748), 1);
}

#[test]
fn exact_screen_densities_read_as_themselves() {
    // 216 ppi and 288 ppi.
    assert_eq!(density_from_pixels_per_metre(8504, 8504), 3);
    assert_eq!(density_from_pixels_per_metre(11339, 11339), 4);
    // 143.5 ppi, within 1 ppi of 144.
    assert_eq!(density_from_pixels_per_metre(5650, 5650), 2);
}

#[test]
fn axes_that_disagree_read_as_one() {
    let dims = Some(png::PixelDimensions {
        xppu: 5669,
        yppu: 2835,
        unit: png::Unit::Meter,
    });
    assert_eq!(image_pixel_density(&png_with(dims)), 1);
    assert_eq!(density_from_pixels_per_metre(5669, 8504), 1);
}

#[test]
fn non_png_and_broken_bytes_read_as_one() {
    assert_eq!(image_pixel_density(b"GIF89a"), 1);
    assert_eq!(image_pixel_density(&[0xFF, 0xD8, 0xFF, 0xE0]), 1);
    assert_eq!(image_pixel_density(b"\x89PNG\r\n\x1a\nbroken"), 1);
    assert_eq!(image_pixel_density(&[]), 1);
}

#[test]
fn a_png_written_at_a_density_reads_back_as_it_and_keeps_its_pixels() {
    let rgba: Vec<u8> = (0..6 * 3 * 4).map(|i| (i * 7 % 256) as u8).collect();
    for density in 1..=MAX_IMAGE_DENSITY {
        let bytes = encode_png_rgba_at_density(&rgba, 6, 3, density).expect("encodes");
        assert_eq!(image_pixel_density(&bytes), density, "density {density}");
        let decoded = image::load_from_memory(&bytes).expect("decodes").to_rgba8();
        assert_eq!(decoded.dimensions(), (6, 3));
        assert_eq!(decoded.into_raw(), rgba);
    }
    assert_eq!(pixels_per_metre_for_density(2), 5669);
    assert!(encode_png_rgba_at_density(&rgba, 7, 3, 2).is_err());
}
