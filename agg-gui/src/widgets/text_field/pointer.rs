//! Pointer-to-caret mapping, drag selection, and horizontal auto-scroll for
//! [`TextField`], split out of `text_field.rs` to keep it under the project's
//! 800-line cap.
//!
//! These `pub(super)` helpers are driven by the mouse handlers in `widget_impl`
//! and by the edit funnels that need to keep the caret on screen.

use super::*;

impl TextField {
    /// Convert a pixel x position (in text-local space) to a byte offset in
    /// `real_text`.  In password mode, measures the masked string and maps back.
    pub(super) fn click_to_cursor(&self, real_text: &str, tx: f64) -> usize {
        let font = self.active_font();
        if self.masking_active() {
            const BULLET: char = '•';
            const BULLET_LEN: usize = 3;
            let n = real_text.chars().count();
            let masked = BULLET.to_string().repeat(n);
            let disp = byte_at_x(&font, &masked, self.font_size, tx);
            // Map masked byte offset → char index → real byte offset.
            let char_idx = disp / BULLET_LEN;
            real_text
                .char_indices()
                .nth(char_idx)
                .map(|(i, _)| i)
                .unwrap_or(real_text.len())
        } else {
            byte_at_x(&font, real_text, self.font_size, tx)
        }
    }

    /// Extend the selection during a drag, honouring the granularity the
    /// initiating click established. A plain drag moves the caret one grapheme
    /// at a time; a word-drag (after a double-click) grows the selection by
    /// whole words; a line-drag (after a triple-click) keeps the whole field
    /// selected. `new_cur` is the caret byte offset under the pointer.
    pub(super) fn extend_selection_drag(&mut self, text: &str, new_cur: usize) {
        use crate::widgets::multi_click::SelectGranularity;
        match self.select_granularity {
            SelectGranularity::Char => {
                self.edit.borrow_mut().cursor = new_cur;
            }
            SelectGranularity::Word => {
                let (pivot_start, pivot_end) = self.select_pivot;
                let (cs, ce) = word_range_at(text, new_cur);
                let mut st = self.edit.borrow_mut();
                if new_cur >= pivot_end {
                    // Dragging to/past the pivot's right edge: anchor stays at
                    // the pivot's left, caret snaps to the far word's end.
                    st.anchor = pivot_start;
                    st.cursor = ce;
                } else {
                    // Dragging left of the pivot: anchor flips to the pivot's
                    // right, caret snaps to the near word's start.
                    st.anchor = pivot_end;
                    st.cursor = cs;
                }
            }
            SelectGranularity::Line => {
                // Single logical line — the whole field stays selected.
                let len = text.len();
                let mut st = self.edit.borrow_mut();
                st.anchor = 0;
                st.cursor = len;
            }
        }
    }

    /// Scroll `scroll_x` so that the cursor stays visible.
    pub(super) fn ensure_cursor_visible(&mut self) {
        if self.bounds.width < 1.0 {
            return;
        }
        let inner_w = (self.bounds.width - self.padding * 2.0).max(0.0);
        let font = self.active_font();
        let cx = {
            let st = self.edit.borrow();
            if self.masking_active() {
                const BULLET: char = '•';
                #[allow(dead_code)]
                const BULLET_LEN: usize = 3;
                let n = st.text[..st.cursor].chars().count();
                let masked = BULLET.to_string().repeat(n);
                measure_advance(&font, &masked, self.font_size)
            } else {
                measure_advance(&font, &st.text[..st.cursor], self.font_size)
            }
        };
        if cx < self.scroll_x {
            self.scroll_x = cx;
        } else if cx > self.scroll_x + inner_w {
            self.scroll_x = cx - inner_w;
        }
    }
}
