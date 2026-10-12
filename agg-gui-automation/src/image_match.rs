//! `find_least_squares_match`: find one image inside another — the port of
//! agg-sharp `ImageBuffer.FindLeastSquaresMatch` (`agg/Image/ImageBuffer.cs`).
//!
//! The runner's image searches (`runner/images.rs`: `image_exists`,
//! `wait_for_image`) look for a needle in a captured screen with this, under
//! the runner's `match_limit`. Both images are agg-gui [`Framebuffer`]s:
//! RGBA8 with row 0 at the bottom, the layout of agg-sharp's `ImageBuffer`,
//! so a match position is (x, row) in the same Y-up space C# reports.

use agg_gui::Framebuffer;

/// The answer of [`find_least_squares_match`]: C#'s return value and its two
/// `out` parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeastSquaresMatch {
    /// Whether a placement within the error limit was found (C#'s return).
    pub found: bool,
    /// The lower-left corner of the best placement seen, in the haystack's
    /// pixels (x, Y-up row); (0, 0) when no placement fits (C#
    /// `bestPosition`).
    pub best_position: (i32, i32),
    /// The summed squared byte difference of that placement; `f64::MAX` when
    /// no placement fits (C# `bestLeastSquares`). A placement abandoned for
    /// exceeding the limit records its partial sum, as C# does.
    pub best_least_squares: f64,
}

/// Bytes per pixel of a [`Framebuffer`] (RGBA8): C#'s `BitDepth / 8`, the
/// same for every framebuffer, so C#'s bit-depth check always passes.
const BYTES_PER_PIXEL: usize = 4;

/// Slide `image_to_find` over every placement inside `haystack` and keep the
/// one whose summed squared byte difference (all four channels) is least.
///
/// `max_error` is the most error a match may have (C#'s default is
/// `f64::MAX`). Each placement is abandoned as soon as its running sum
/// exceeds the limit, and the limit tightens to the best sum found so far;
/// an exact match (zero error) ends the search at once. A needle larger than
/// the haystack has no placement and is never found. The search runs
/// rows bottom-up and, within a row, left to right, so of equal placements
/// the first in that order wins.
pub fn find_least_squares_match(
    haystack: &Framebuffer,
    image_to_find: &Framebuffer,
    mut max_error: f64,
) -> LeastSquaresMatch {
    let mut best_position = (0, 0);
    let mut best_least_squares = f64::MAX;

    let width = haystack.width() as usize;
    let height = haystack.height() as usize;
    let find_width = image_to_find.width() as usize;
    let find_height = image_to_find.height() as usize;

    if width >= find_width && height >= find_height {
        let this_buffer = haystack.pixels();
        let contained_buffer = image_to_find.pixels();
        for match_y in 0..=height - find_height {
            for match_x in 0..=width - find_width {
                let mut current_least_squares = 0.0;

                'rows: for image_to_find_y in 0..find_height {
                    let mut this_buffer_offset =
                        ((match_y + image_to_find_y) * width + match_x) * BYTES_PER_PIXEL;
                    let mut image_to_find_buffer_offset =
                        image_to_find_y * find_width * BYTES_PER_PIXEL;
                    for _ in 0..find_width {
                        for byte_index in 0..BYTES_PER_PIXEL {
                            let a_byte = this_buffer[this_buffer_offset + byte_index];
                            let b_byte = contained_buffer[image_to_find_buffer_offset + byte_index];
                            let difference = i32::from(a_byte) - i32::from(b_byte);
                            current_least_squares += f64::from(difference * difference);
                        }

                        this_buffer_offset += BYTES_PER_PIXEL;
                        image_to_find_buffer_offset += BYTES_PER_PIXEL;
                        if current_least_squares > max_error {
                            // stop checking we have too much error.
                            break 'rows;
                        }
                    }
                }

                if current_least_squares < best_least_squares {
                    best_position = (match_x as i32, match_y as i32);
                    best_least_squares = current_least_squares;
                    if max_error > current_least_squares {
                        max_error = current_least_squares;
                        if current_least_squares == 0.0 {
                            return LeastSquaresMatch {
                                found: true,
                                best_position,
                                best_least_squares,
                            };
                        }
                    }
                }
            }
        }
    }

    // C# answers `bestLeastSquares <= maxError` here even when the needle is
    // larger than the haystack and nothing was compared, which with the
    // default limit (MaxValue <= MaxValue) reports a match that does not
    // exist; a search with no placement finds nothing.
    let any_placement = width >= find_width && height >= find_height;
    LeastSquaresMatch {
        found: any_placement && best_least_squares <= max_error,
        best_position,
        best_least_squares,
    }
}
