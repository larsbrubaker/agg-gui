//! Port of agg-sharp's `LcdCoverageTests` (Tests/Agg.Tests/Agg/LcdCoverageTests.cs).
//!
//! Covers [`super::filter`]: the second stage of the LCD pipeline, which
//! collapses the 3×-horizontally-supersampled gray coverage buffer into the
//! packed 3-byte-per-pixel mask.  Every expected value is hand-computed, so
//! a regression shows up as a byte mismatch.  Same test names (snake_case),
//! values and tolerances as the C# class.
//!
//! C#'s `ArgumentException` / `ArgumentNullException` on a malformed gray
//! buffer become panics here (the filters are crate-internal and a bad size
//! is a caller bug); `null` has no Rust equivalent, so the null case of
//! `gray_buffer_shorter_than_all_its_rows_is_rejected` is covered by the type
//! system instead.

use super::filter::{apply_5_tap_filter_legacy, apply_5_tap_filter_with, apply_gray_collapse};

const DEFAULT_PRIMARY: f64 = 1.0 / 3.0;
const DEFAULT_GAMMA: f64 = 1.0;

fn assert_mask_bytes(expected: &[u8], data: &[u8]) {
    assert_eq!(data.len(), expected.len());
    for (i, (&got, &want)) in data.iter().zip(expected).enumerate() {
        assert_eq!(
            got,
            want,
            "mask byte {i} (pixel {}, channel {})",
            i / 3,
            ["R", "G", "B"][i % 3]
        );
    }
}

fn panics<F: FnOnce() -> Vec<u8> + std::panic::UnwindSafe>(f: F) -> bool {
    std::panic::catch_unwind(f).is_err()
}

#[test]
fn integer_path_matches_hand_computed_bytes() {
    let gray = [10u8, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
    let mask = apply_5_tap_filter_legacy(&gray, 12, 4, 1);
    let expected = [
        11, 20, 30, // px 0: R (3*10+2*20+30)/9=11, G 180/9, B 270/9
        40, 50, 60, // px 1: 360/9, 450/9, 540/9
        70, 80, 90, // px 2: 630/9, 720/9, 810/9
        100, 95, 75, // px 3: 900/9, 860/9 truncated, 680/9 truncated
    ];
    assert_mask_bytes(&expected, &mask);
}

/// A uniform mid-gray 128 weighted by [1,2,3,2,1] sums to 1152, a clean 128
/// after the integer divide by 9; the float formulation lands at 127.999…
#[test]
fn integer_path_keeps_mid_gray_exact() {
    let gray = [128u8; 12];
    let mask = apply_5_tap_filter_legacy(&gray, 12, 4, 1);
    for px in 1..=2usize {
        assert_eq!(&mask[px * 3..px * 3 + 3], &[128, 128, 128]);
    }
}

/// Each channel is centred on its own physical subpixel, so one lit
/// subpixel spreads asymmetrically across R/G/B.
#[test]
fn impulse_spreads_with_per_channel_phase_offsets() {
    let mut gray = [0u8; 15];
    gray[7] = 255;
    let mask = apply_5_tap_filter_legacy(&gray, 15, 5, 1);
    let expected = [
        0, 0, 0, // px 0: impulse is outside every window
        0, 0, 28, // px 1: blue's +4 tap only, 255/9
        56, 85, 56, // px 2: weights 2, 3, 2
        28, 0, 0, // px 3: red's -2 tap only
        0, 0, 0, // px 4
    ];
    assert_mask_bytes(&expected, &mask);
}

/// Out-of-range taps read 0, never a clamped edge sample.
#[test]
fn out_of_range_taps_read_zero_rather_than_clamping() {
    let mut gray = [0u8; 6];
    gray[0] = 255;
    let mask = apply_5_tap_filter_legacy(&gray, 6, 2, 1);
    let expected = [
        85, 56, 28, // px 0: weights 3, 2, 1 on the single in-range tap
        0, 0, 0, // px 1
    ];
    assert_mask_bytes(&expected, &mask);
}

/// The gray sibling box-averages each triple with `(r + g + b + 1) / 3` and
/// replicates it, so the mask carries no chroma.
#[test]
fn gray_collapse_averages_triples_with_no_chroma() {
    let gray = [
        0u8, 0, 0, // 1/3 -> 0
        1, 1, 1, // 4/3 -> 1
        10, 20, 31, // 62/3 -> 20
        255, 255, 255, // 766/3 -> 255, the maximum
    ];
    let mask = apply_gray_collapse(&gray, 12, 4, 1);
    let expected = [0, 0, 0, 1, 1, 1, 20, 20, 20, 255, 255, 255];
    assert_mask_bytes(&expected, &mask);
    for px in mask.chunks(3) {
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
    }
}

/// Rows advance by `gray_w` (not `mask_w * 3`), and mask row 0 comes from
/// gray row 0.
#[test]
fn legacy_path_walks_rows_by_gray_stride() {
    let gray = [
        9u8, 18, 27, 36, 45, 54, 63, 72, // row 0 (bottom): ascending ramp
        72, 63, 54, 45, 36, 27, 18, 9, // row 1: the same ramp descending
    ];
    let mask = apply_5_tap_filter_legacy(&gray, 8, 2, 2);
    let expected = [
        10, 18, 27, // row 0 px 0
        36, 45, 54, // row 0 px 1: B reaches the padding at 6 and 7
        44, 54, 54, // row 1 px 0
        45, 36, 27, // row 1 px 1
    ];
    assert_mask_bytes(&expected, &mask);
}

/// The same row-addressing contract for the gray collapse path: the trailing
/// padding byte (99) is skipped.
#[test]
fn gray_collapse_walks_rows_by_gray_stride() {
    let gray = [
        10u8, 20, 30, 40, 50, 60, 99, // row 0 (bottom), trailing 99 is padding
        0, 0, 1, 255, 255, 255, 99, // row 1
    ];
    let mask = apply_gray_collapse(&gray, 7, 2, 2);
    let expected = [20, 20, 20, 50, 50, 50, 0, 0, 0, 255, 255, 255];
    assert_mask_bytes(&expected, &mask);
}

/// The float path rounds halves away from zero (`f64::round`): a primary
/// weight of 2/9 makes the taps [1,2,2,2,1]/8, so 20 under an outer tap is
/// exactly 2.5 -> 3.
#[test]
fn float_path_rounds_halves_away_from_zero() {
    let mut gray = [0u8; 9];
    gray[4] = 20;
    let mask = apply_5_tap_filter_with(&gray, 9, 3, 1, 2.0 / 9.0, DEFAULT_GAMMA);
    let expected = [
        0, 0, 3, // px 0: only blue's +4 tap (1/8) -> 2.5 -> 3
        5, 5, 5, // px 1: every channel on a 2/8 tap -> 5
        3, 0, 0, // px 2: only red's -2 tap -> 2.5 -> 3
    ];
    assert_mask_bytes(&expected, &mask);
}

/// The default parameters route to the byte-exact integer path, and the
/// dispatch is load bearing: just past the 1e-6 epsilon the float path
/// rounds 860/9 = 95.5 up to 96.
#[test]
fn default_parameters_dispatch_to_the_integer_path() {
    let gray = [10u8, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
    let legacy = apply_5_tap_filter_legacy(&gray, 12, 4, 1);
    let defaulted = apply_5_tap_filter_with(&gray, 12, 4, 1, DEFAULT_PRIMARY, DEFAULT_GAMMA);
    assert_mask_bytes(&legacy, &defaulted);

    let parameterized = apply_5_tap_filter_with(&gray, 12, 4, 1, (1.0 / 3.0) + 1e-5, DEFAULT_GAMMA);
    assert_eq!(parameterized[3 * 3 + 1], 96);
    assert_eq!(legacy[3 * 3 + 1], 95);
}

/// Gamma applies to the per-channel coverage after the filter sum.
#[test]
fn gamma_is_applied_after_the_filter_sum() {
    let mut gray = [0u8; 15];
    gray[7] = 255;
    let mask = apply_5_tap_filter_with(&gray, 15, 5, 1, DEFAULT_PRIMARY, 2.0);
    assert_eq!(mask[2 * 3], 120);
    // Green's 3/9 tap: 85 -> sqrt(85/255) * 255 = 147.
    assert_eq!(mask[2 * 3 + 1], 147);
}

/// A gray row narrower than `mask_w * 3` is rejected by every entry point.
#[test]
fn gray_row_narrower_than_the_mask_footprint_is_rejected() {
    let gray = [0u8; 12];
    // gray_w 5 against a mask_w * 3 of 6.
    assert!(panics(|| apply_5_tap_filter_legacy(&gray, 5, 2, 1)));
    assert!(panics(|| apply_5_tap_filter_with(
        &gray,
        5,
        2,
        1,
        DEFAULT_PRIMARY,
        DEFAULT_GAMMA
    )));
    assert!(panics(|| apply_gray_collapse(&gray, 5, 2, 1)));
    // The parameterised path validates on its own, past the fast path's dispatch.
    assert!(panics(|| apply_5_tap_filter_with(
        &gray,
        5,
        2,
        1,
        2.0 / 9.0,
        DEFAULT_GAMMA
    )));
}

/// The stride can be wide enough and the buffer still too short to hold
/// `gray_w * mask_h` bytes.
#[test]
fn gray_buffer_shorter_than_all_its_rows_is_rejected() {
    // 6 * 2 = 12 bytes required, 11 supplied.
    assert!(panics(|| apply_5_tap_filter_legacy(&[0u8; 11], 6, 2, 2)));
    assert!(panics(|| apply_gray_collapse(&[0u8; 11], 6, 2, 2)));
}
