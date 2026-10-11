//! `Label`'s line spacing for wrapped text: agg-sharp's
//! `TextWidget.LineSpacing` / `TypeFacePrinter.LineSpacing`, a multiple of
//! the line height that each line after the first advances by. The first
//! line keeps one line height, so `1.0` (the default) is the plain stacked
//! layout and `1.5` adds half a line between lines. `Label::layout`,
//! `paint`, `ink_overhang` and `measure_min_height` (in `label.rs`) place
//! wrapped lines through [`Label::wrapped_height`] and
//! [`Label::line_offset`].

use super::Label;

impl Label {
    /// agg-sharp `TextWidget.LineSpacing`: wrapped lines after the first
    /// advance by `spacing` line heights instead of one.
    pub fn with_line_spacing(mut self, spacing: f64) -> Self {
        self.set_line_spacing(spacing);
        self
    }

    /// Setter counterpart of [`with_line_spacing`](Self::with_line_spacing).
    /// Kept across [`set_text`](Self::set_text): text changes never reset it.
    pub fn set_line_spacing(&mut self, spacing: f64) {
        if self.line_spacing != spacing {
            self.line_spacing = spacing;
            self.cache.invalidate();
        }
    }

    /// The current line spacing (default `1.0`).
    pub fn line_spacing(&self) -> f64 {
        self.line_spacing
    }

    /// Height of `lines` wrapped lines of `line_h` each: the first line is
    /// one line tall, every later one adds `line_spacing` lines.
    pub(super) fn wrapped_height(&self, lines: usize, line_h: f64) -> f64 {
        if lines == 0 {
            return 0.0;
        }
        line_h + (lines - 1) as f64 * self.line_spacing * line_h
    }

    /// Distance from the top of the text to the centre of wrapped line `i`.
    pub(super) fn line_offset(&self, i: usize, line_h: f64) -> f64 {
        (i as f64 * self.line_spacing + 0.5) * line_h
    }
}
