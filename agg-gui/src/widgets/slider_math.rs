//! Pure value-mapping math for [`crate::widgets::slider::Slider`].
//!
//! This module holds the numeric core of the slider — everything that turns a
//! value into a normalized `[0, 1]` position and back — kept separate from the
//! widget's paint/event code so the mapping can be unit-tested in isolation and
//! so `slider.rs` stays under the project's 800-line limit.
//!
//! The logarithmic mapping (including ranges that span zero and infinity) and
//! the "smart aim" round-number finder are ported from egui's
//! `crates/egui/src/widgets/slider.rs` and `crates/emath/src/smart_aim.rs`
//! respectively (see `egui-reference/`).  Semantics are preserved so the
//! widget behaves like egui's; only the API shape (scalar `min`/`max` instead
//! of `RangeInclusive`) differs to avoid dragging range types through the
//! widget.

use std::cmp::Ordering;

/// Configuration for how a slider maps values to positions.
///
/// Mirrors egui's private `SliderSpec`.  For linear sliders only
/// `logarithmic == false` matters; the other two fields shape how logarithmic
/// sliders treat the special endpoints `0` and `∞`.
#[derive(Clone, Copy, Debug)]
pub struct SliderSpec {
    pub logarithmic: bool,
    /// For logarithmic sliders, the smallest positive value we care about.
    /// `1` for integer sliders, `1e-6` for reals by default.
    pub smallest_positive: f64,
    /// For logarithmic sliders, the largest positive value before the slider
    /// switches to `INFINITY` (when infinity is the high end). Default: `∞`.
    pub largest_finite: f64,
}

impl Default for SliderSpec {
    fn default() -> Self {
        Self {
            logarithmic: false,
            smallest_positive: 1e-6,
            largest_finite: f64::INFINITY,
        }
    }
}

const INFINITY: f64 = f64::INFINITY;

/// For an infinitely large logarithmic range (e.g. from zero), span this many
/// orders of magnitude.
const INF_RANGE_MAGNITUDE: f64 = 10.0;

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline]
fn remap(x: f64, from_lo: f64, from_hi: f64, to_lo: f64, to_hi: f64) -> f64 {
    let t = (x - from_lo) / (from_hi - from_lo);
    lerp(to_lo, to_hi, t)
}

#[inline]
fn remap_clamp(x: f64, from_lo: f64, from_hi: f64, to_lo: f64, to_hi: f64) -> f64 {
    if x <= from_lo.min(from_hi) {
        if from_lo <= from_hi {
            to_lo
        } else {
            to_hi
        }
    } else if x >= from_lo.max(from_hi) {
        if from_lo <= from_hi {
            to_hi
        } else {
            to_lo
        }
    } else {
        remap(x, from_lo, from_hi, to_lo, to_hi)
    }
}

/// Clamp `x` into `[min, max]`, tolerating a reversed (high-to-low) range and
/// NaN endpoints the same way egui's `clamp_value_to_range` does.
pub fn clamp_value_to_range(x: f64, min: f64, max: f64) -> f64 {
    let (mut lo, mut hi) = (min, max);
    if lo.total_cmp(&hi) == Ordering::Greater {
        std::mem::swap(&mut lo, &mut hi);
    }
    match x.total_cmp(&lo) {
        Ordering::Less | Ordering::Equal => lo,
        Ordering::Greater => match x.total_cmp(&hi) {
            Ordering::Greater | Ordering::Equal => hi,
            Ordering::Less => x,
        },
    }
}

/// Map a value to its normalized `[0, 1]` slider position.
pub fn normalized_from_value(value: f64, min: f64, max: f64, spec: &SliderSpec) -> f64 {
    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        0.5 // empty range, show center of slider
    } else if min > max {
        1.0 - normalized_from_value(value, max, min, spec)
    } else if value <= min {
        0.0
    } else if value >= max {
        1.0
    } else if spec.logarithmic {
        if max <= 0.0 {
            // non-positive range
            normalized_from_value(-value, -min, -max, spec)
        } else if 0.0 <= min {
            let (min_log, max_log) = range_log10(min, max, spec);
            let value_log = value.log10();
            remap_clamp(value_log, min_log, max_log, 0.0, 1.0)
        } else {
            let zero_cutoff = logarithmic_zero_cutoff(min, max);
            if value < 0.0 {
                remap(
                    normalized_from_value(value, min, 0.0, spec),
                    0.0,
                    1.0,
                    0.0,
                    zero_cutoff,
                )
            } else {
                remap(
                    normalized_from_value(value, 0.0, max, spec),
                    0.0,
                    1.0,
                    zero_cutoff,
                    1.0,
                )
            }
        }
    } else {
        remap_clamp(value, min, max, 0.0, 1.0)
    }
}

/// Inverse of [`normalized_from_value`]: map a normalized `[0, 1]` position
/// back to a value.
pub fn value_from_normalized(normalized: f64, min: f64, max: f64, spec: &SliderSpec) -> f64 {
    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        min
    } else if min > max {
        value_from_normalized(1.0 - normalized, max, min, spec)
    } else if normalized <= 0.0 {
        min
    } else if normalized >= 1.0 {
        max
    } else if spec.logarithmic {
        if max <= 0.0 {
            // non-positive range
            -value_from_normalized(normalized, -min, -max, spec)
        } else if 0.0 <= min {
            let (min_log, max_log) = range_log10(min, max, spec);
            let log = lerp(min_log, max_log, normalized);
            10.0_f64.powf(log)
        } else {
            let zero_cutoff = logarithmic_zero_cutoff(min, max);
            if normalized < zero_cutoff {
                // negative
                value_from_normalized(remap(normalized, 0.0, zero_cutoff, 0.0, 1.0), min, 0.0, spec)
            } else {
                // positive
                value_from_normalized(remap(normalized, zero_cutoff, 1.0, 0.0, 1.0), 0.0, max, spec)
            }
        }
    } else {
        lerp(min, max, normalized.clamp(0.0, 1.0))
    }
}

fn range_log10(min: f64, max: f64, spec: &SliderSpec) -> (f64, f64) {
    debug_assert!(spec.logarithmic, "spec must be logarithmic");
    debug_assert!(min <= max, "min must be <= max, got min={min} max={max}");

    if min == 0.0 && max == INFINITY {
        (spec.smallest_positive.log10(), INF_RANGE_MAGNITUDE)
    } else if min == 0.0 {
        if spec.smallest_positive < max {
            (spec.smallest_positive.log10(), max.log10())
        } else {
            (max.log10() - INF_RANGE_MAGNITUDE, max.log10())
        }
    } else if max == INFINITY {
        if min < spec.largest_finite {
            (min.log10(), spec.largest_finite.log10())
        } else {
            (min.log10(), min.log10() + INF_RANGE_MAGNITUDE)
        }
    } else {
        (min.log10(), max.log10())
    }
}

/// Where to place the zero crossing for a logarithmic slider whose range spans
/// zero (negative min, positive max).
fn logarithmic_zero_cutoff(min: f64, max: f64) -> f64 {
    debug_assert!(
        min < 0.0 && 0.0 < max,
        "min must be negative and max positive, got min={min} max={max}"
    );

    let min_magnitude = if min == -INFINITY {
        INF_RANGE_MAGNITUDE
    } else {
        min.abs().log10().abs()
    };
    let max_magnitude = if max == INFINITY {
        INF_RANGE_MAGNITUDE
    } else {
        max.log10().abs()
    };

    let cutoff = min_magnitude / (min_magnitude + max_magnitude);
    debug_assert!(
        (0.0..=1.0).contains(&cutoff),
        "Bad cutoff {cutoff:?} for min {min:?} and max {max:?}"
    );
    cutoff
}

// ---------------------------------------------------------------------------
// Smart aim — find the "roundest" number in a range so dragging snaps to nice
// values.  Ported from egui's `emath::smart_aim`.
// ---------------------------------------------------------------------------

const NUM_DECIMALS: usize = 16;

#[inline]
fn fast_midpoint(a: f64, b: f64) -> f64 {
    (a + b) / 2.0
}

#[inline]
fn u8_midpoint(a: u8, b: u8) -> u8 {
    ((a as u16 + b as u16) / 2) as u8
}

/// Find the "simplest" number in the closed range `[min, max]` — the one with
/// the fewest decimal digits.  E.g. `[0.83, 1.354] -> 1.0`, `[0.37, 0.48] -> 0.4`.
pub fn best_in_range_f64(min: f64, max: f64) -> f64 {
    // Avoid NaN if we can:
    if min.is_nan() {
        return max;
    }
    if max.is_nan() {
        return min;
    }

    if max < min {
        return best_in_range_f64(max, min);
    }
    if min == max {
        return min;
    }
    if min <= 0.0 && 0.0 <= max {
        return 0.0; // always prefer zero
    }
    if min < 0.0 {
        return -best_in_range_f64(-max, -min);
    }

    debug_assert!(0.0 < min && min < max, "Logic bug");

    // Prefer finite numbers:
    if !max.is_finite() {
        return min;
    }

    let min_exponent = min.log10();
    let max_exponent = max.log10();

    if min_exponent.floor() != max_exponent.floor() {
        // Different orders of magnitude — pick the geometric center of the two:
        let exponent = fast_midpoint(min_exponent, max_exponent);
        return 10.0_f64.powi(exponent.round() as i32);
    }

    if is_integer(min_exponent) {
        return 10.0_f64.powf(min_exponent);
    }
    if is_integer(max_exponent) {
        return 10.0_f64.powf(max_exponent);
    }

    // Find the proper scale, then convert to integers:
    let scale = NUM_DECIMALS as i32 - max_exponent.floor() as i32 - 1;
    let scale_factor = 10.0_f64.powi(scale);

    let min_str = to_decimal_string((min * scale_factor).round() as u64);
    let max_str = to_decimal_string((max * scale_factor).round() as u64);

    // Two positive integers of the same length. Find the first non-matching
    // ("deciding") digit; everything before it matches, everything after is
    // zero, and the deciding digit is a "smart average".
    let mut ret_str = [0u8; NUM_DECIMALS];

    for i in 0..NUM_DECIMALS {
        if min_str[i] == max_str[i] {
            ret_str[i] = min_str[i];
        } else {
            let mut deciding_digit_min = min_str[i];
            let deciding_digit_max = max_str[i];

            debug_assert!(deciding_digit_min < deciding_digit_max, "Bug in smart aim");

            let rest_of_min_is_zeroes = min_str[i + 1..].iter().all(|&c| c == 0);

            if !rest_of_min_is_zeroes {
                // There are more digits after `deciding_digit_min`, so we can't
                // pick it — the true selectable min is one greater:
                deciding_digit_min += 1;
            }

            let deciding_digit = if deciding_digit_min == 0 {
                0
            } else if deciding_digit_min <= 5 && 5 <= deciding_digit_max {
                5 // 5 is the roundest number in the range
            } else {
                u8_midpoint(deciding_digit_min, deciding_digit_max)
            };

            ret_str[i] = deciding_digit;

            return from_decimal_string(ret_str) as f64 / scale_factor;
        }
    }

    min // All digits the same (already handled earlier, but be safe).
}

fn is_integer(f: f64) -> bool {
    f.round() == f
}

fn to_decimal_string(v: u64) -> [u8; NUM_DECIMALS] {
    let mut ret = [0u8; NUM_DECIMALS];
    let mut value = v;
    for i in (0..NUM_DECIMALS).rev() {
        ret[i] = (value % 10) as u8;
        value /= 10;
    }
    ret
}

fn from_decimal_string(s: [u8; NUM_DECIMALS]) -> u64 {
    let mut value = 0u64;
    for &c in &s {
        debug_assert!(c <= 9, "Bad number");
        value = value * 10 + c as u64;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_spec() -> SliderSpec {
        SliderSpec {
            logarithmic: true,
            smallest_positive: 1e-6,
            largest_finite: f64::INFINITY,
        }
    }

    #[test]
    fn linear_mapping_roundtrips() {
        let spec = SliderSpec::default();
        for &(v, n) in &[(0.0, 0.0), (5.0, 0.5), (10.0, 1.0), (2.5, 0.25)] {
            assert!((normalized_from_value(v, 0.0, 10.0, &spec) - n).abs() < 1e-9);
            assert!((value_from_normalized(n, 0.0, 10.0, &spec) - v).abs() < 1e-9);
        }
    }

    #[test]
    fn linear_clamps_outside_range() {
        let spec = SliderSpec::default();
        assert_eq!(normalized_from_value(-5.0, 0.0, 10.0, &spec), 0.0);
        assert_eq!(normalized_from_value(50.0, 0.0, 10.0, &spec), 1.0);
    }

    #[test]
    fn logarithmic_midpoint_is_geometric_mean() {
        let spec = log_spec();
        // For 1..=100 log slider, the halfway position should be ~10.
        let mid = value_from_normalized(0.5, 1.0, 100.0, &spec);
        assert!((mid - 10.0).abs() < 1e-6, "got {mid}");
        // And the inverse holds.
        let n = normalized_from_value(10.0, 1.0, 100.0, &spec);
        assert!((n - 0.5).abs() < 1e-9, "got {n}");
    }

    #[test]
    fn logarithmic_reversed_range() {
        let spec = log_spec();
        // High-to-low range: normalized 0 -> max value.
        let v = value_from_normalized(0.0, 100.0, 1.0, &spec);
        assert!((v - 100.0).abs() < 1e-9, "got {v}");
    }

    #[test]
    fn logarithmic_spanning_zero_puts_zero_at_cutoff() {
        let spec = log_spec();
        // Symmetric range around zero -> cutoff at 0.5, value there is ~0.
        let cutoff = logarithmic_zero_cutoff(-1000.0, 1000.0);
        assert!((cutoff - 0.5).abs() < 1e-9, "got {cutoff}");
        let v = value_from_normalized(0.5, -1000.0, 1000.0, &spec);
        assert!(v.abs() < 1.0, "expected near zero, got {v}");
    }

    #[test]
    fn logarithmic_spanning_infinity() {
        let spec = log_spec();
        // 0..=∞ maps normalized 1.0 to ∞ and 0.0 to 0.0.
        assert_eq!(value_from_normalized(1.0, 0.0, f64::INFINITY, &spec), f64::INFINITY);
        assert_eq!(value_from_normalized(0.0, 0.0, f64::INFINITY, &spec), 0.0);
        // A midpoint stays finite and positive.
        let mid = value_from_normalized(0.5, 0.0, f64::INFINITY, &spec);
        assert!(mid.is_finite() && mid > 0.0, "got {mid}");
    }

    /// Regression: the Sliders demo's range sliders span `-∞..=∞` (logarithmic)
    /// and the demo slider itself may be rebuilt with an infinite bound. The
    /// initial displayed value/position must be finite — never `NaN` — for the
    /// exact values the demo starts with. (egui's Sliders demo shows finite
    /// values, never NaN.)
    #[test]
    fn infinite_range_initial_mapping_is_finite() {
        let spec = log_spec();
        // Demo slider default: value 10 in 0..=10000 (log).
        let n = normalized_from_value(10.0, 0.0, 10000.0, &spec);
        assert!(n.is_finite() && (0.0..=1.0).contains(&n), "demo n={n}");

        // Range sliders: both bounds infinite. The current min (0) and max
        // (10000) must map to a finite, in-range position.
        for &v in &[0.0, 10000.0] {
            let n = normalized_from_value(v, -f64::INFINITY, f64::INFINITY, &spec);
            assert!(n.is_finite() && (0.0..=1.0).contains(&n), "v={v} n={n}");
        }
        // A finite normalized position maps back to a finite value.
        let mid = value_from_normalized(0.5, -f64::INFINITY, f64::INFINITY, &spec);
        assert!(mid.is_finite(), "mid={mid}");
    }

    /// One infinite bound (e.g. `min..=∞` after the user drags a range slider):
    /// a finite value in the range still maps to a finite, in-range position,
    /// and round-trips.
    #[test]
    fn one_infinite_bound_roundtrips_finite() {
        let spec = log_spec();
        let n = normalized_from_value(10.0, -f64::INFINITY, 10000.0, &spec);
        assert!(n.is_finite() && (0.0..=1.0).contains(&n), "n={n}");
        let v = value_from_normalized(n, -f64::INFINITY, 10000.0, &spec);
        assert!(v.is_finite(), "v={v}");

        let n = normalized_from_value(10.0, 0.0, f64::INFINITY, &spec);
        assert!(n.is_finite() && (0.0..=1.0).contains(&n), "n={n}");
    }

    #[test]
    fn clamp_handles_reversed_range() {
        assert_eq!(clamp_value_to_range(5.0, 10.0, 0.0), 5.0);
        assert_eq!(clamp_value_to_range(-1.0, 10.0, 0.0), 0.0);
        assert_eq!(clamp_value_to_range(11.0, 10.0, 0.0), 10.0);
    }

    // Smart-aim cases ported from egui's `smart_aim::test_aim`.
    #[test]
    fn smart_aim_round_numbers() {
        assert_eq!(best_in_range_f64(-0.2, 0.0), 0.0);
        assert_eq!(best_in_range_f64(-10_004.23, 3.14), 0.0);
        assert_eq!(best_in_range_f64(7.8, 17.8), 10.0);
        assert_eq!(best_in_range_f64(99.0, 300.0), 100.0);
        assert_eq!(best_in_range_f64(-99.0, -300.0), -100.0);
        assert_eq!(best_in_range_f64(0.4, 0.9), 0.5);
        assert_eq!(best_in_range_f64(14.1, 19.99), 15.0);
        assert_eq!(best_in_range_f64(12.3, 65.9), 50.0);
        assert_eq!(best_in_range_f64(493.0, 879.0), 500.0);
        assert_eq!(best_in_range_f64(0.37, 0.48), 0.40);
        assert_eq!(best_in_range_f64(7.5, 16.3), 10.0);
        assert_eq!(best_in_range_f64(7.5, 763.3), 100.0);
        assert_eq!(best_in_range_f64(7.5, 123_456.0), 1000.0);
        assert_eq!(best_in_range_f64(9.9999, 99.999), 10.0);
        assert_eq!(best_in_range_f64(10.001, 99.999), 50.0);
    }

    #[test]
    fn smart_aim_integers() {
        assert_eq!(best_in_range_f64(99.0, 300.0), 100.0);
        assert_eq!(best_in_range_f64(4.0, 9.0), 5.0);
        assert_eq!(best_in_range_f64(14.0, 19.0), 15.0);
        assert_eq!(best_in_range_f64(12.0, 65.0), 50.0);
        assert_eq!(best_in_range_f64(37.0, 48.0), 40.0);
        assert_eq!(best_in_range_f64(12345.0, 12780.0), 12500.0);
    }

    #[test]
    fn smart_aim_nan_and_infinity() {
        assert!(best_in_range_f64(f64::NAN, f64::NAN).is_nan());
        assert_eq!(best_in_range_f64(f64::NAN, 1.2), 1.2);
        assert_eq!(best_in_range_f64(1.2, f64::INFINITY), 1.2);
        assert_eq!(best_in_range_f64(f64::NEG_INFINITY, 1.2), 0.0);
        assert_eq!(best_in_range_f64(f64::NEG_INFINITY, -2.7), -2.7);
        assert_eq!(best_in_range_f64(f64::NEG_INFINITY, f64::INFINITY), 0.0);
    }
}
