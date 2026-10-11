//! Whole-device-pixel grid lines — a port of agg-sharp's `GridPixels`
//! (`MarkdigAgg/Tables/GridPixels.cs`).
//!
//! Pixel snapping shared by table grids (the rich Markdown editor's table
//! layout in [`crate::widgets::markdown_editing`] and its painter), so every
//! grid draws lines a whole number of device pixels thick on whole device
//! pixels.

/// A grid line's thickness in whole device pixels: one pixel at device scale
/// 1, scaled and rounded above it (C# `(int)Math.Round`, banker's rounding,
/// so 2.5 gives 2).
pub fn line_thickness(device_scale: f64) -> i32 {
    // `as` saturates where C#'s `(int)` cast would be undefined; a device
    // scale never comes near either limit.
    (round_half_even(device_scale) as i32).max(1)
}

/// `value` moved to the nearest whole device pixel once the draw transform's
/// `translation` on that axis is applied, returned in drawing coordinates.
///
/// `floor(v + 0.5)`, not banker's rounding: banker's rounding sends some .5
/// edges down and others up (device scale 1.25 puts edges on .5), which made
/// columns differ by a pixel and let a line eat into a cell's padding.
/// Assumes a translation-only transform (no zoom or scale).
pub fn snap(value: f64, translation: f64) -> f64 {
    (value + translation + 0.5).floor() - translation
}

/// C# `Math.Round`: halves round to the even neighbour.  Hand-written because
/// `f64::round_ties_even` is newer than agg-gui's MSRV.
fn round_half_even(x: f64) -> f64 {
    let rounded = x.round();
    if (rounded - x).abs() == 0.5 {
        2.0 * (x / 2.0).round()
    } else {
        rounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_only_line_thickness_is_whole_pixels_at_least_one() {
        assert_eq!(line_thickness(0.5), 1);
        assert_eq!(line_thickness(1.0), 1);
        assert_eq!(line_thickness(1.25), 1);
        assert_eq!(line_thickness(1.5), 2);
        assert_eq!(line_thickness(2.0), 2);
        // C#'s Math.Round is banker's rounding.
        assert_eq!(line_thickness(2.5), 2);
        assert_eq!(line_thickness(3.5), 4);
    }

    #[test]
    fn rust_only_snap_rounds_halves_up_in_device_pixels() {
        assert_eq!(snap(10.5, 0.0), 11.0);
        assert_eq!(snap(11.5, 0.0), 12.0);
        assert_eq!(snap(10.4, 0.0), 10.0);
        // The translation decides which whole pixel; the result is in drawing
        // coordinates.
        assert_eq!(snap(10.0, 0.25), 9.75);
        assert_eq!(snap(10.3, 0.25), 10.75);
    }
}
