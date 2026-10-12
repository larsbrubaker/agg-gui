//! Unwrapped mode for [`TextArea`](super::TextArea): agg-sharp
//! `TextEditWidget.WordWrap = false` (its default), where every line stays
//! whole and the field scrolls sideways to keep the caret in view instead of
//! continuing a long line on the next one.
//!
//! The wrap (`wrap.rs`) is handed an unbounded width, so only the text's own
//! newlines break lines, and [`TextArea::line_x_start`] subtracts the sideways
//! scroll, so paint, hit-testing, the caret and vertical line moves
//! (`geometry.rs`, `edit_ops.rs`) all follow it. The scroll moves only to
//! bring the caret back into view, from
//! [`ensure_cursor_visible`](TextArea::ensure_cursor_visible) (`scroll.rs`),
//! as agg-sharp's `InternalTextEditWidget` does after each edit or caret move.

use super::*;

/// Room kept right of the caret when scrolling to it, so the bar is drawn
/// inside the field (agg-sharp's `Width - 2`).
const CARET_ROOM: f64 = 2.0;

impl TextArea {
    /// Wrap lines at the area's width (`true`, the default) or keep each line
    /// whole and scroll sideways to the caret (`false`). agg-sharp
    /// `TextEditWidget.WordWrap`.
    pub fn with_word_wrap(mut self, word_wrap: bool) -> Self {
        self.set_word_wrap(word_wrap);
        self
    }

    /// Switch wrapping on or off after construction; see
    /// [`with_word_wrap`](Self::with_word_wrap). The lines re-flow at the next
    /// layout, starting from the left edge.
    pub fn set_word_wrap(&mut self, word_wrap: bool) {
        if self.word_wrap != word_wrap {
            self.word_wrap = word_wrap;
            self.h_offset = 0.0;
            self.mark_dirty();
            crate::animation::request_draw();
        }
    }

    /// Whether lines wrap at the area's width.
    pub fn word_wrap(&self) -> bool {
        self.word_wrap
    }

    /// agg-sharp `TopLeftOffset.X`: how far the text has scrolled sideways,
    /// `0` while the start of every line is in view and negative once the text
    /// has scrolled left. Always `0` while the area wraps.
    pub fn top_left_offset_x(&self) -> f64 {
        -self.h_offset
    }

    /// The width `wrap_text_indexed` breaks lines at: the inner width while
    /// wrapping, unbounded otherwise (only newlines break a line).
    pub(super) fn wrap_width(&self, inner_w: f64) -> f64 {
        if self.word_wrap {
            inner_w.max(1.0)
        } else {
            f64::INFINITY
        }
    }

    /// Scroll sideways the least that brings the caret inside the padded inner
    /// width, keeping [`CARET_ROOM`] clear on the right for the caret bar
    /// (agg-sharp `TextEditWidget.InternalTextEditWidget_InsertBarPositionChanged`'s
    /// horizontal half). A wrapping area never scrolls sideways.
    pub(super) fn scroll_caret_into_view_sideways(&mut self) {
        if self.word_wrap || self.cached_lines.is_empty() {
            self.h_offset = 0.0;
            return;
        }
        let inner_w = self.inner_width();
        let cursor = self.edit.borrow().cursor;
        let line = &self.cached_lines[self.line_for_cursor(cursor)];
        let seg_end = cursor.clamp(line.start, line.end);
        // The caret's advance from its line's start, the same measure the
        // caret is drawn at (`pos_for_cursor`) before the scroll is applied.
        let caret_x = self
            .edit
            .borrow()
            .text
            .get(line.start..seg_end)
            .map_or(0.0, |seg| measure_advance(&self.font, seg, self.font_size));
        if caret_x < self.h_offset {
            self.h_offset = caret_x;
        } else if caret_x > self.h_offset + inner_w - CARET_ROOM {
            self.h_offset = caret_x - inner_w + CARET_ROOM;
        }
    }
}
