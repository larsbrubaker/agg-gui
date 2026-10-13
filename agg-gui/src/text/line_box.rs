//! The box a one-line text widget occupies once its bounds snap to whole
//! device pixels, as agg-sharp's `TextWidget` has it.
//!
//! agg-sharp sets `GuiWidget.DefaultEnforceIntegerBounds = true`, so a
//! `TextWidget`'s `LocalBounds` (the `TypeFacePrinter`'s one-em box, its
//! bottom `LineBoxBottomInPixels` below the baseline) is rounded outward:
//! `Floor` on the left and bottom, `Ceiling` on the right and top, in device
//! pixels. This module is that rule, in layout units: [`snapped_line_box`] for
//! the height and where the baseline sits in it, [`snapped_text_width`] for the
//! width. `Label` and `TextField` apply it under
//! [`LineBox::EmSnapped`](crate::font_settings::LineBox::EmSnapped); apps that
//! draw their own text can call it directly.

/// A one-line text box: its height and the baseline's height above its bottom,
/// both in layout units (device pixels divided by the device scale).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnappedLineBox {
    /// Floor(bottom) to Ceiling(top) in device pixels, over the scale.
    pub height: f64,
    /// Where the text's baseline sits above the box's bottom: `-Floor(bottom)`
    /// in device pixels, over the scale.
    pub baseline: f64,
}

/// The snapped line box for a face with `ascent` and `descent` (both
/// positive, as [`TextMetrics`](super::TextMetrics) and
/// [`Font::descender_px`](super::Font::descender_px) give them) at `size`, with
/// all three in layout units and `scale` the device scale.
///
/// The unsnapped one-em box starts at `LineBoxBottomInPixels =
/// (ascent - descent - size) / 2` from the baseline (centred on the ascent and
/// descent span; Liberation Sans 1.07 gives -0.2 em); a face whose span is
/// zero or less starts at its descent, as `TypeFacePrinter` does.
pub fn snapped_line_box(ascent: f64, descent: f64, size: f64, scale: f64) -> SnappedLineBox {
    let bottom = if ascent + descent <= 0.0 {
        -descent
    } else {
        (ascent - descent - size) / 2.0
    };
    let device_bottom = (bottom * scale).floor();
    let device_top = ((bottom + size) * scale).ceil();
    SnappedLineBox {
        height: (device_top - device_bottom) / scale,
        baseline: -device_bottom / scale,
    }
}

/// `width` (layout units) rounded up to whole device pixels: a `TextWidget`'s
/// right edge is `Ceiling`'d.
pub fn snapped_text_width(width: f64, scale: f64) -> f64 {
    (width * scale).ceil() / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_of_one_em_snaps_outward_on_both_ends() {
        // Liberation Sans 1.07 at 16 px: ascent 12.797, descent 3.203.
        let b = snapped_line_box(16.0 * 1638.0 / 2048.0, 16.0 * 410.0 / 2048.0, 16.0, 1.0);
        // bottom -3.2 -> -4, top 12.8 -> 13.
        assert_eq!(b.height, 17.0);
        assert_eq!(b.baseline, 4.0);
    }

    #[test]
    fn a_face_with_no_span_starts_at_its_descent() {
        let b = snapped_line_box(0.0, 0.0, 10.0, 1.0);
        assert_eq!((b.height, b.baseline), (10.0, 0.0));
    }

    #[test]
    fn widths_round_up_in_device_pixels() {
        assert_eq!(snapped_text_width(10.1, 1.0), 11.0);
        assert_eq!(snapped_text_width(10.1, 2.0), 10.5);
    }
}
