//! Cursor / hit-test geometry for [`TextArea`], split out of `text_area.rs`
//! to keep that file under the 800-line cap.
//!
//! These helpers map between byte offsets in the text and widget-local (Y-up)
//! coordinates. Every one of them routes vertical placement through
//! [`TextArea::content_top_y`] / [`TextArea::line_top_y`], so scrolling (and the
//! over-scan band's paint-time anchor override) is applied consistently across
//! hit-testing, the caret overlay, and cursor-position queries.

use super::*;

impl TextArea {
    /// Locate the (line_index, byte_pos_in_text) that the given cursor
    /// byte offset lives on.  Returns `(0, 0)` on empty content.
    pub(super) fn line_for_cursor(&self, byte_pos: usize) -> usize {
        for (i, l) in self.cached_lines.iter().enumerate() {
            if byte_pos >= l.start && byte_pos <= l.end {
                return i;
            }
        }
        self.cached_lines.len().saturating_sub(1)
    }

    /// Hit-test a widget-local point to a text byte offset.  Clamps to
    /// `[0, text.len()]` at the edges.  `local` is Y-UP.
    pub(super) fn byte_offset_at(&self, local: Point) -> usize {
        if self.cached_lines.is_empty() || self.cached_line_h <= 0.0 {
            return 0;
        }
        // Visual lines stack top-to-bottom; Y-up flips their y coords.
        // Line 0 sits at the top (high Y), line N at the bottom (low Y).
        // `content_top_y` folds in the vertical-alignment shift.
        let rel_from_top = self.content_top_y() - local.y;
        let mut line_idx = (rel_from_top / self.cached_line_h).floor() as isize;
        if line_idx < 0 {
            line_idx = 0;
        }
        if line_idx as usize >= self.cached_lines.len() {
            line_idx = self.cached_lines.len() as isize - 1;
        }
        let line = &self.cached_lines[line_idx as usize];
        // X hit test: walk chars in the line's rendered text and pick
        // the nearest grapheme boundary. The line's x start folds in the
        // horizontal-alignment shift.
        let pad_x = self.line_x_start(line);
        let rel_x = (local.x - pad_x).max(0.0);
        let txt = &line.text;
        let mut best_byte = 0usize;
        let mut best_delta = f64::INFINITY;
        let mut acc = 0.0_f64;
        let mut prev_byte = 0usize;
        for (i, _c) in txt.char_indices().chain(std::iter::once((txt.len(), ' '))) {
            let w_here = if i > prev_byte {
                measure_advance(&self.font, &txt[prev_byte..i], self.font_size)
            } else {
                0.0
            };
            acc += w_here;
            let d = (acc - rel_x).abs();
            if d < best_delta {
                best_delta = d;
                best_byte = i;
            }
            prev_byte = i;
        }
        line.start + best_byte
    }

    /// Screen position (widget-local, Y-UP) of the given cursor byte
    /// offset.  Returns the bottom-left corner of the cursor glyph
    /// cell.
    pub(super) fn pos_for_cursor(&self, byte_pos: usize) -> Point {
        if self.cached_lines.is_empty() {
            return Point::ORIGIN;
        }
        let line_idx = self.line_for_cursor(byte_pos);
        let line = &self.cached_lines[line_idx];
        let offset = byte_pos.saturating_sub(line.start).min(line.text.len());
        let x = self.line_x_start(line)
            + measure_advance(&self.font, &line.text[..offset], self.font_size);
        // Y-up: line i top-edge folds in the vertical-alignment shift.
        let line_top = self.line_top_y(line_idx);
        let line_bottom = line_top - self.cached_line_h;
        Point::new(x, line_bottom)
    }

    /// Glyph baseline Y (Y-up) for visual line `i`, vertically centring the
    /// font's full vertical extent within the 1.35× line cell.
    ///
    /// `descent` is a POSITIVE quantity here (see [`Font::descender_px`]), so
    /// the text block's height is `ascent + descent`. Getting this wrong pushes
    /// the block up and clips line 0's ascenders against the padded inner-rect
    /// clip, so paint and the clip-safety test share this single helper.
    pub(super) fn line_baseline_y(&self, i: usize) -> f64 {
        let ascent = self.font.ascender_px(self.font_size);
        let descent = self.font.descender_px(self.font_size);
        let line_top = self.line_top_y(i);
        let line_bottom = line_top - self.cached_line_h;
        let baseline = line_bottom + (self.cached_line_h - (ascent + descent)) * 0.5 + descent;
        // Snap to the pixel grid when hinting is on so multi-line text lands
        // stems on physical rows exactly like `Label` (no-op when hinting off).
        crate::font_settings::snap_baseline_y(baseline)
    }
}
