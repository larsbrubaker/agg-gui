//! Cursor movement and text-mutation primitives for [`TextArea`], split out of
//! `text_area.rs` to keep that file under the project's 800-line cap.
//!
//! These are the low-level funnels the widget's key/mouse handlers call:
//! `insert_str`/`delete` mutate the buffer (and fire `on_change` — see
//! `callbacks.rs`), while `move_cursor_to`/`move_char`/`move_line` reposition
//! the caret without touching the text. They are `pub(super)` so the sibling
//! `widget_impl` and `scroll` modules can drive them.

use super::*;

impl TextArea {
    /// Insert a string at the cursor, replacing any active selection.
    pub(super) fn insert_str(&mut self, s: &str) {
        let mut st = self.edit.borrow_mut();
        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        // Make sure we slice at grapheme boundaries.
        let lo = lo.min(st.text.len());
        let hi = hi.min(st.text.len());
        st.text.replace_range(lo..hi, s);
        st.cursor = lo + s.len();
        st.anchor = st.cursor;
        st.note_text_change();
        drop(st);
        self.mark_dirty();
        self.notify_change();
    }

    /// Delete the current selection, or (if empty) `dir` chars toward
    /// the supplied side.  `-1` = backspace, `+1` = delete, `0` = just
    /// collapse the selection (cut path).
    pub(super) fn delete(&mut self, dir: i32) {
        let mut st = self.edit.borrow_mut();
        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        if lo != hi {
            st.text.replace_range(lo..hi, "");
            st.cursor = lo;
            st.anchor = lo;
        } else if dir < 0 && st.cursor > 0 {
            let cur = st.cursor;
            let prev = prev_char_boundary(&st.text, cur);
            st.text.replace_range(prev..cur, "");
            st.cursor = prev;
            st.anchor = prev;
        } else if dir > 0 && st.cursor < st.text.len() {
            let cur = st.cursor;
            let next = next_char_boundary(&st.text, cur);
            st.text.replace_range(cur..next, "");
        }
        st.note_text_change();
        drop(st);
        self.mark_dirty();
        self.notify_change();
    }

    /// Move cursor to an absolute byte offset.  `with_selection=false`
    /// collapses anchor with cursor; `true` leaves the anchor alone
    /// so a selection is extended.
    pub(super) fn move_cursor_to(&mut self, pos: usize, with_selection: bool) {
        let mut st = self.edit.borrow_mut();
        let p = pos.min(st.text.len());
        st.cursor = p;
        if !with_selection {
            st.anchor = p;
        }
    }

    /// Cursor one char left / right.
    pub(super) fn move_char(&mut self, dir: i32, with_selection: bool) {
        let st = self.edit.borrow();
        let p = if dir < 0 {
            prev_char_boundary(&st.text, st.cursor)
        } else {
            next_char_boundary(&st.text, st.cursor)
        };
        drop(st);
        self.move_cursor_to(p, with_selection);
    }

    /// Cursor one visual line up / down.  `dir` = −1 for up, +1 for down.
    pub(super) fn move_line(&mut self, dir: i32, with_selection: bool) {
        if self.cached_lines.is_empty() {
            return;
        }
        let cursor = self.edit.borrow().cursor;
        let cur_line = self.line_for_cursor(cursor);
        let target_line = if dir < 0 {
            cur_line.saturating_sub(1)
        } else {
            (cur_line + 1).min(self.cached_lines.len() - 1)
        };
        if target_line == cur_line {
            return;
        }
        // Preserve horizontal position (pixel column, not byte column),
        // measured relative to the current line's aligned start so left /
        // center / right alignment all keep the caret in the same column.
        let cur_line_x = self
            .cached_lines
            .get(cur_line)
            .map(|l| self.line_x_start(l))
            .unwrap_or(self.padding);
        let cur_x = self.pos_for_cursor(cursor).x - cur_line_x;
        // Find byte offset in target_line closest to `cur_x`.
        let line = &self.cached_lines[target_line];
        let txt = &line.text;
        let mut best_byte = 0usize;
        let mut best_delta = f64::INFINITY;
        let mut acc = 0.0_f64;
        let mut prev_byte = 0usize;
        for (i, _) in txt.char_indices().chain(std::iter::once((txt.len(), ' '))) {
            let w = if i > prev_byte {
                measure_advance(&self.font, &txt[prev_byte..i], self.font_size)
            } else {
                0.0
            };
            acc += w;
            let d = (acc - cur_x).abs();
            if d < best_delta {
                best_delta = d;
                best_byte = i;
            }
            prev_byte = i;
        }
        let target = line.start + best_byte;
        self.move_cursor_to(target, with_selection);
    }
}
