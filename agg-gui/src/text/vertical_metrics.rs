//! Per-font vertical-metrics override: the ascent, descent, line gap and cap
//! height a [`Font`] reports, in font units, replacing the ones read from the
//! face's tables.
//!
//! Every place that centres or baselines text works from these numbers:
//! [`Font::ascender_px`], [`Font::descender_px`] and [`Font::line_height_px`]
//! feed [`TextMetrics`](super::TextMetrics) (through
//! [`measure_text_metrics`](super::measure_text_metrics) and every `DrawCtx`'s
//! `measure_text`), and `Label`, `Button`, `TextField`, the line boxes and the
//! ellipsized paths all centre on [`TextMetrics::centered_baseline_y`](super::TextMetrics::centered_baseline_y).
//! Overriding the metrics here moves all of them together.
//!
//! The override exists for apps that must place text where another renderer
//! placed the same face: agg-sharp reads Liberation Sans 1.07 from an SVG font
//! whose ascent and descent (1638 / -410 units) differ from the 2.x TrueType
//! face's `hhea` (1854 / -434), so the same face centred in the same box sits
//! at a different height. It is opt-in: a font that is not given one keeps the
//! face's own metrics, and nothing else changes. Shaping, advances and glyph
//! outlines never depend on these values, so the glyph and shape caches stay
//! valid for both.

use super::Font;

/// A face's vertical metrics in font units (Y-up, so `descent` is negative
/// below the baseline, as `hhea` and SVG fonts store it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerticalMetrics {
    /// Height of the ascent above the baseline.
    pub ascent: i16,
    /// Depth of the descent: negative below the baseline.
    pub descent: i16,
    /// Extra space between one line's descent and the next line's ascent.
    pub line_gap: i16,
    /// Height of a flat capital letter (H) above the baseline; 0 when the face
    /// gives none.
    pub cap_height: i16,
}

impl Font {
    /// This font with its vertical metrics replaced by `metrics` (font
    /// units of this face's em). Everything that centres or baselines text
    /// with this font then uses them; see the module docs.
    ///
    /// ```ignore
    /// // agg-sharp's Liberation Sans 1.07 SVG face on the 2.x TrueType outlines.
    /// let font = Font::from_slice(LIBERATION_SANS)?.with_vertical_metrics(VerticalMetrics {
    ///     ascent: 1638, descent: -410, line_gap: 0, cap_height: 1409,
    /// });
    /// ```
    pub fn with_vertical_metrics(mut self, metrics: VerticalMetrics) -> Self {
        self.ascender = metrics.ascent;
        self.descender = metrics.descent;
        self.line_gap = metrics.line_gap;
        self.cap_height = metrics.cap_height;
        self
    }

    /// The vertical metrics in force: the override when one was given,
    /// otherwise the face's own.
    pub fn vertical_metrics(&self) -> VerticalMetrics {
        VerticalMetrics {
            ascent: self.ascender,
            descent: self.descender,
            line_gap: self.line_gap,
            cap_height: self.cap_height,
        }
    }

    /// Cap height in pixels at the given font size (0 when the face gives
    /// none and no override supplies one).
    pub fn cap_height_px(&self, size: f64) -> f64 {
        self.cap_height as f64 * size / self.units_per_em as f64
    }

    /// How far `(above, below)` the baseline this font's ink can reach, in
    /// pixels (both positive): the larger of the metrics in force and the
    /// face's own ascender and descender. An override may describe a line box
    /// tighter than the face's glyphs (agg-sharp's Liberation Sans 1.07
    /// metrics span exactly an em, and its descenders reach below that
    /// descent), so whatever clips or buffers text reaches this far, never
    /// only as far as the metrics, and no ink is cut. Without an override it
    /// is [`Font::ascender_px`] and [`Font::descender_px`].
    pub fn ink_extent_px(&self, size: f64) -> (f64, f64) {
        let scale = size / self.units_per_em as f64;
        let above = self.ascender.max(self.face_ascender) as f64 * scale;
        let below = self.descender.min(self.face_descender).unsigned_abs() as f64 * scale;
        (above, below)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::measure_text_metrics;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

    /// agg-sharp's Liberation Sans 1.07 numbers, used as an arbitrary override.
    const OVERRIDE: VerticalMetrics = VerticalMetrics {
        ascent: 1638,
        descent: -410,
        line_gap: 0,
        cap_height: 1409,
    };

    #[test]
    fn without_an_override_the_face_metrics_are_used() {
        let font = Font::from_slice(FONT_BYTES).unwrap();
        let face = ttf_parser::Face::parse(FONT_BYTES, 0).unwrap();
        assert_eq!(
            font.vertical_metrics(),
            VerticalMetrics {
                ascent: face.ascender(),
                descent: face.descender(),
                line_gap: face.line_gap(),
                cap_height: face.capital_height().unwrap_or(0),
            }
        );
        let upem = face.units_per_em() as f64;
        assert_eq!(font.ascender_px(32.0), face.ascender() as f64 * 32.0 / upem);
        assert_eq!(
            font.line_height_px(32.0),
            (face.ascender() - face.descender() + face.line_gap()) as f64 * 32.0 / upem
        );
    }

    #[test]
    fn an_override_replaces_every_vertical_metric() {
        let font = Font::from_slice(FONT_BYTES)
            .unwrap()
            .with_vertical_metrics(OVERRIDE);
        let upem = font.units_per_em() as f64;
        assert_eq!(font.vertical_metrics(), OVERRIDE);
        assert_eq!(font.ascender_px(upem), 1638.0);
        assert_eq!(font.descender_px(upem), 410.0);
        assert_eq!(font.line_height_px(upem), 2048.0);
        assert_eq!(font.cap_height_px(upem), 1409.0);
        // The measured metrics every widget centres on follow it.
        let m = measure_text_metrics(&font, "Hg", upem);
        assert_eq!(
            (m.ascent, m.descent, m.line_height),
            (1638.0, 410.0, 2048.0)
        );
        // A tabular variant of the same face keeps the override.
        assert_eq!(
            font.variant_with_tabular_digits().vertical_metrics(),
            OVERRIDE
        );
    }

    #[test]
    fn the_ink_extent_covers_both_the_override_and_the_face() {
        let face = ttf_parser::Face::parse(FONT_BYTES, 0).unwrap();
        let plain = Font::from_slice(FONT_BYTES).unwrap();
        let upem = plain.units_per_em() as f64;
        assert_eq!(
            plain.ink_extent_px(upem),
            (plain.ascender_px(upem), plain.descender_px(upem))
        );
        let tight = VerticalMetrics {
            ascent: 1000,
            descent: -100,
            line_gap: 0,
            cap_height: 700,
        };
        let font = Font::from_slice(FONT_BYTES)
            .unwrap()
            .with_vertical_metrics(tight);
        assert_eq!(
            font.ink_extent_px(upem),
            (
                face.ascender().max(1000) as f64,
                face.descender().min(-100).unsigned_abs() as f64
            )
        );
        let wide = VerticalMetrics {
            ascent: 4000,
            descent: -3000,
            ..tight
        };
        let font = Font::from_slice(FONT_BYTES)
            .unwrap()
            .with_vertical_metrics(wide);
        assert_eq!(font.ink_extent_px(upem), (4000.0, 3000.0));
    }

    #[test]
    fn an_override_leaves_advances_alone() {
        let plain = Font::from_slice(FONT_BYTES).unwrap();
        let overridden = Font::from_slice(FONT_BYTES)
            .unwrap()
            .with_vertical_metrics(OVERRIDE);
        assert_eq!(
            measure_text_metrics(&plain, "New Design", 16.0).width,
            measure_text_metrics(&overridden, "New Design", 16.0).width
        );
    }
}
