//! `Label`'s line box: which of [`LineBox`](crate::font_settings::LineBox)'s
//! boxes one line of text gets, its height, and the baseline the text sits on
//! in a box of a given height. `Label::layout`, `paint`, `ink_overhang` and
//! `measure_min_height` (in `label.rs`) place text through these.

use super::Label;

impl Label {
    /// The line box in force: this label's own, else the app-wide one.
    pub(super) fn effective_line_box(&self) -> crate::font_settings::LineBox {
        self.line_box
            .unwrap_or_else(crate::font_settings::current_line_box)
    }

    /// Height of one line of `font` at `size`: 1.5 em for [`LineBox::Standard`],
    /// one em for [`LineBox::Em`] (agg-sharp's `TextWidget`, whose box is the
    /// font size tall with the text centred on its ascent/descent span), and
    /// [`LineBox::EmSnapped`] that box rounded out to whole device pixels.
    ///
    /// [`LineBox::Standard`]: crate::font_settings::LineBox::Standard
    /// [`LineBox::Em`]: crate::font_settings::LineBox::Em
    /// [`LineBox::EmSnapped`]: crate::font_settings::LineBox::EmSnapped
    pub(super) fn line_height(&self, font: &crate::text::Font, size: f64) -> f64 {
        use crate::font_settings::LineBox;
        match self.effective_line_box() {
            LineBox::Standard => size * 1.5,
            LineBox::Em => size,
            LineBox::EmSnapped => {
                crate::text::snapped_line_box(
                    font.ascender_px(size),
                    font.descender_px(size),
                    size,
                    crate::device_scale(),
                )
                .height
            }
        }
    }

    /// The baseline (Y up from the bottom of a box `box_h` tall) that places a
    /// line with metrics `m` the way this label's line box does: centred on
    /// its ascent/descent span, or for [`LineBox::EmSnapped`] with the snapped
    /// box's baseline, the box itself centred in `box_h`.
    ///
    /// [`LineBox::EmSnapped`]: crate::font_settings::LineBox::EmSnapped
    pub(super) fn text_baseline(&self, m: &crate::text::TextMetrics, box_h: f64) -> f64 {
        if self.effective_line_box() == crate::font_settings::LineBox::EmSnapped {
            let b = crate::text::snapped_line_box(
                m.ascent,
                m.descent,
                self.active_font_size(),
                crate::device_scale(),
            );
            (box_h - b.height) * 0.5 + b.baseline
        } else {
            m.centered_baseline_y(box_h)
        }
    }
}
