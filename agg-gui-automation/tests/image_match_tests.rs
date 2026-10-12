//! Rust-only tests of `find_least_squares_match` (the port of agg-sharp
//! `ImageBuffer.FindLeastSquaresMatch`): where it finds a needle, how its
//! error limit decides a match, and the C# bug it does not port (a needle
//! larger than the haystack reported as found under the default limit).

use agg_gui::Framebuffer;
use agg_gui_automation::find_least_squares_match;

/// A `width` × `height` image of one opaque grey level.
fn grey(width: u32, height: u32, level: u8) -> Framebuffer {
    let mut image = Framebuffer::new(width, height);
    for pixel in image.pixels_mut().as_chunks_mut::<4>().0 {
        pixel.copy_from_slice(&[level, level, level, 255]);
    }
    image
}

/// Set the pixel at (x, Y-up row) to a grey level.
fn set(image: &mut Framebuffer, x: u32, y: u32, level: u8) {
    let offset = ((y * image.width() + x) * 4) as usize;
    image.pixels_mut()[offset..offset + 4].copy_from_slice(&[level, level, level, 255]);
}

#[test]
fn rust_only_an_exact_match_reports_its_lower_left_corner_in_y_up_rows() {
    let mut haystack = grey(20, 10, 255);
    for y in 6..8 {
        for x in 3..5 {
            set(&mut haystack, x, y, 0);
        }
    }
    let needle = grey(2, 2, 0);

    let found = find_least_squares_match(&haystack, &needle, f64::MAX);

    assert!(found.found);
    assert_eq!(found.best_position, (3, 6));
    assert_eq!(found.best_least_squares, 0.0);
}

#[test]
fn rust_only_the_error_limit_decides_whether_a_near_match_counts() {
    // One channel-wide difference of 5 in each of R, G and B: 3 * 25 = 75.
    let haystack = grey(4, 4, 100);
    let needle = grey(1, 1, 105);

    let within = find_least_squares_match(&haystack, &needle, 75.0);
    assert!(within.found, "an error equal to the limit is a match");
    assert_eq!(within.best_least_squares, 75.0);

    let over = find_least_squares_match(&haystack, &needle, 50.0);
    assert!(
        !over.found,
        "an error over the runner's match limit (50) is not a match"
    );
}

#[test]
fn rust_only_a_needle_larger_than_the_haystack_is_never_found() {
    // C# compares bestLeastSquares (MaxValue, nothing compared) with the
    // default limit (MaxValue) and reports a match.
    let haystack = grey(4, 4, 0);
    let needle = grey(5, 2, 0);

    let found = find_least_squares_match(&haystack, &needle, f64::MAX);

    assert!(!found.found);
    assert_eq!(found.best_position, (0, 0));
    assert_eq!(found.best_least_squares, f64::MAX);
}
