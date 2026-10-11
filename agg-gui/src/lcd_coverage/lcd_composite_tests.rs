//! Port of agg-sharp's `LcdCompositeTests` (Tests/Agg.Tests/Agg/LcdCompositeTests.cs).
//!
//! Covers [`composite_lcd_mask`]: the third stage of the LCD pipeline, where
//! the draw colour meets the destination and each subpixel blends by its own
//! coverage.  Same test names (snake_case) and expected bytes as the C#
//! class.  C# composites into a BGRA `ImageBuffer` read through
//! `ImageBuffer.Order*`; agg-gui's destination is a Y-up RGBA byte slice, so
//! the helpers here read and write that layout directly.  C# colours are
//! bytes; agg-gui's [`Color`] is `f32`, built here from the same bytes / 255.

use super::*;
use agg_rust::basics::PATH_FLAGS_NONE;
use agg_rust::path_storage::PathStorage;

/// An agg-gui colour from the C# test's byte channels.
pub(super) fn c8(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::rgba(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

/// The deliberately fractional triangle `LcdMaskBuilderTests.FractionalTriangle`
/// rasters, shared so both stages are pinned against one path.
pub(super) fn fractional_triangle() -> PathStorage {
    let mut p = PathStorage::new();
    p.move_to(10.25, 10.5);
    p.line_to(14.75, 11.25);
    p.line_to(12.5, 14.75);
    p.close_polygon(PATH_FLAGS_NONE);
    p
}

struct Dest {
    data: Vec<u8>,
    w: u32,
    h: u32,
}

impl Dest {
    fn new(w: u32, h: u32, fill: [u8; 4]) -> Self {
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            data.extend_from_slice(&fill);
        }
        Self { data, w, h }
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    fn composite(&mut self, mask: &LcdMask, src: Color, x: i32, y: i32) {
        composite_lcd_mask(&mut self.data, self.w, self.h, mask, src, x, y);
    }

    fn assert_pixel(&self, expected: [u8; 4], x: u32, y: u32) {
        assert_eq!(self.pixel(x, y), expected, "pixel at ({x}, {y})");
    }
}

fn triangle_mask(width: u32, height: u32) -> LcdMask {
    let mut builder = LcdMaskBuilder::new(width, height);
    let mut path = fractional_triangle();
    builder.with_paths(&identity_xform(), |add| add(&mut path));
    builder.finalize()
}

/// The whole formula on one pixel: `cover = mask / 255 * srcAlpha`, then
/// `dst = src * cover + dst * (1 - cover)` per channel, half-up round.  The
/// differing coverages double as the byte-order check.
#[test]
fn single_pixel_blends_each_channel_by_its_own_coverage() {
    let mut d = Dest::new(1, 1, [10, 200, 250, 137]);
    let mask = LcdMask {
        data: vec![200, 100, 0],
        width: 1,
        height: 1,
    };
    d.composite(&mask, c8(255, 128, 64, 255), 0, 0);
    // R: 0.792772 -> 202; G: 0.673587 -> 172; B: coverage 0 keeps 250.
    d.assert_pixel([202, 172, 250, 137], 0, 0);
}

/// Source alpha scales every channel's coverage; destination alpha is never
/// written.
#[test]
fn source_alpha_scales_coverage_while_destination_alpha_survives() {
    let full = LcdMask {
        data: vec![255, 255, 255],
        width: 1,
        height: 1,
    };
    let mut opaque = Dest::new(1, 1, [255, 255, 255, 137]);
    opaque.composite(&full, c8(0, 0, 0, 255), 0, 0);
    opaque.assert_pixel([0, 0, 0, 137], 0, 0);

    // Half alpha over white: cover = 128/255 -> 127/255 -> 127.
    let mut half = Dest::new(1, 1, [255, 255, 255, 137]);
    half.composite(&full, c8(0, 0, 0, 128), 0, 0);
    half.assert_pixel([127, 127, 127, 137], 0, 0);

    // Zero alpha zeroes every coverage: nothing is written.
    let mut transparent = Dest::new(1, 1, [255, 255, 255, 137]);
    transparent.composite(&full, c8(0, 0, 0, 0), 0, 0);
    transparent.assert_pixel([255, 255, 255, 137], 0, 0);
}

/// Over mid-gray, dark ink may only darken a channel and light ink only
/// lighten one, and each must move something.
#[test]
fn dark_and_light_ink_move_only_toward_the_source_color() {
    let mask = triangle_mask(20, 20);
    const BACKGROUND: u8 = 128;
    let bg = [BACKGROUND, BACKGROUND, BACKGROUND, 255];
    let mut dark_on_gray = Dest::new(20, 20, bg);
    dark_on_gray.composite(&mask, c8(0, 0, 0, 255), 0, 0);
    let mut light_on_gray = Dest::new(20, 20, bg);
    light_on_gray.composite(&mask, c8(255, 255, 255, 255), 0, 0);

    let mut darkened = false;
    let mut lightened = false;
    for y in 0..mask.height {
        for x in 0..mask.width {
            let dark = dark_on_gray.pixel(x, y);
            let light = light_on_gray.pixel(x, y);
            for ch in 0..3 {
                let name = ["R", "G", "B"][ch];
                assert!(
                    dark[ch] <= BACKGROUND,
                    "dark ink brightened {name} at ({x}, {y})"
                );
                assert!(
                    light[ch] >= BACKGROUND,
                    "light ink darkened {name} at ({x}, {y})"
                );
                darkened |= dark[ch] < BACKGROUND;
                lightened |= light[ch] > BACKGROUND;
            }
        }
    }
    assert!(darkened, "dark ink must darken some channel");
    assert!(lightened, "light ink must lighten some channel");
}

/// A mask hanging off the destination is clipped: only the overlap
/// composites, and a placement entirely off it writes nothing.
#[test]
fn mask_overlapping_the_destination_edge_composites_only_the_overlap() {
    let full = LcdMask {
        data: vec![255; 4 * 4 * 3],
        width: 4,
        height: 4,
    };
    let white = [255, 255, 255, 255];
    let black = [0, 0, 0, 255];

    // Bottom-left overhang: only the mask's upper-right 2x2 lands.
    let mut d = Dest::new(4, 4, white);
    d.composite(&full, c8(0, 0, 0, 255), -2, -2);
    for y in 0..4 {
        for x in 0..4 {
            let expected = if x < 2 && y < 2 { black } else { white };
            d.assert_pixel(expected, x, y);
        }
    }

    // Top-right overhang: one pixel of overlap.
    let mut corner = Dest::new(4, 4, white);
    corner.composite(&full, c8(0, 0, 0, 255), 3, 3);
    corner.assert_pixel(black, 3, 3);
    corner.assert_pixel(white, 2, 3);
    corner.assert_pixel(white, 3, 2);

    // Entirely off the destination, in both directions.
    let mut missed = Dest::new(4, 4, white);
    missed.composite(&full, c8(0, 0, 0, 255), -10, 0);
    missed.composite(&full, c8(0, 0, 0, 255), 0, 100);
    for y in 0..4 {
        for x in 0..4 {
            missed.assert_pixel(white, x, y);
        }
    }
}

/// End to end: a neutral colour over a neutral background still leaves
/// coloured pixels, and the bounded mask's reported origin lands it in
/// place (the far corners stay untouched).
#[test]
fn bounded_mask_composites_fringing_at_the_reported_origin() {
    let mut d = Dest::new(20, 20, [255, 255, 255, 255]);
    let mut path = fractional_triangle();
    let (mask, ox, oy) = build_bounded_mask(
        20,
        20,
        &mut path,
        &identity_xform(),
        None,
        crate::draw_ctx::FillRule::NonZero,
    )
    .expect("the triangle lies on the destination");
    d.composite(&mask, c8(0, 0, 0, 255), ox, oy);

    let saw_fringing = (0..20)
        .flat_map(|y| (0..20).map(move |x| (x, y)))
        .any(|(x, y)| {
            let p = d.pixel(x, y);
            p[0] != p[1] || p[1] != p[2]
        });
    assert!(
        saw_fringing,
        "a neutral color composited through an LCD mask must leave per-channel color fringing"
    );
    d.assert_pixel([255, 255, 255, 255], 0, 0);
    d.assert_pixel([255, 255, 255, 255], 19, 19);
}
