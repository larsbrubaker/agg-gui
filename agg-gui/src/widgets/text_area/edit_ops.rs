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

    /// Move the caret one word toward `dir` (`-1` = previous word / Ctrl+Left,
    /// `+1` = next word / Ctrl+Right). `extend_selection` leaves the anchor put
    /// so the selection grows (Shift held); otherwise the selection collapses.
    /// Word boundaries come from the shared [`text_field_core`] helpers, so
    /// TextArea and TextField agree on what "a word" is.
    pub(super) fn move_word(&mut self, dir: i32, extend_selection: bool) {
        let st = self.edit.borrow();
        let p = if dir < 0 {
            prev_word_boundary(&st.text, st.cursor)
        } else {
            next_word_boundary(&st.text, st.cursor)
        };
        drop(st);
        self.move_cursor_to(p, extend_selection);
    }

    /// Delete a whole word toward `dir` (`-1` = Ctrl+Backspace, `+1` =
    /// Ctrl+Delete). An active selection takes precedence and is deleted whole,
    /// matching every mainstream editor; only when the selection is empty does
    /// the word-boundary deletion kick in.
    pub(super) fn delete_word(&mut self, dir: i32) {
        let mut st = self.edit.borrow_mut();
        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        if lo != hi {
            st.text.replace_range(lo..hi, "");
            st.cursor = lo;
            st.anchor = lo;
        } else if dir < 0 && st.cursor > 0 {
            let cur = st.cursor;
            let prev = prev_word_boundary(&st.text, cur);
            st.text.replace_range(prev..cur, "");
            st.cursor = prev;
            st.anchor = prev;
        } else if dir > 0 && st.cursor < st.text.len() {
            let cur = st.cursor;
            let next = next_word_boundary(&st.text, cur);
            st.text.replace_range(cur..next, "");
        }
        st.note_text_change();
        drop(st);
        self.mark_dirty();
        self.notify_change();
    }

    /// Insert one indent (`TAB_WIDTH` spaces) at the start of every logical
    /// line the current selection touches. Selection endpoints ride along so
    /// the same characters stay selected — an endpoint sitting exactly at a
    /// line start does not move, so the fresh indent falls *inside* the
    /// selection (matching common code editors). Drives the multi-line Tab.
    pub(super) fn indent_selection(&mut self) {
        let mut st = self.edit.borrow_mut();
        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        let starts = touched_line_starts(&st.text, lo, hi);
        // Insert descending so earlier line-start offsets stay valid.
        for &s in starts.iter().rev() {
            st.text.insert_str(s, INDENT);
        }
        // Shift an endpoint right by one indent for every inserted line start
        // strictly before it.
        let shift =
            |p: usize| -> usize { p + INDENT.len() * starts.iter().filter(|&&s| s < p).count() };
        st.cursor = shift(st.cursor);
        st.anchor = shift(st.anchor);
        st.note_text_change();
        drop(st);
        self.mark_dirty();
        self.notify_change();
    }

    /// Remove one indent from the start of every logical line the selection
    /// touches (or just the caret's line when there is no multi-line
    /// selection). Each line loses one leading tab or up to `TAB_WIDTH` leading
    /// spaces — whatever it actually has, so a shallowly-indented line is not
    /// over-trimmed. Endpoints/caret shift left to preserve the selected text;
    /// a position inside a removed run clamps to its line start. Drives
    /// Shift+Tab.
    pub(super) fn outdent_selection(&mut self) {
        let mut st = self.edit.borrow_mut();
        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        let starts = touched_line_starts(&st.text, lo, hi);
        // How many bytes to strip from each touched line, measured on the
        // original text.
        let removals: Vec<(usize, usize)> = starts
            .iter()
            .map(|&s| {
                let line_end = st.text[s..]
                    .find('\n')
                    .map(|i| s + i)
                    .unwrap_or(st.text.len());
                (s, leading_outdent_len(&st.text[s..line_end]))
            })
            .collect();
        // Remove descending so earlier offsets stay valid.
        for &(s, r) in removals.iter().rev() {
            if r > 0 {
                st.text.replace_range(s..s + r, "");
            }
        }
        // Map an original offset to its post-removal position.
        let adjust = |p: usize| -> usize {
            let mut removed_before = 0usize;
            for &(s, r) in &removals {
                if r == 0 {
                    continue;
                }
                if p >= s + r {
                    removed_before += r;
                } else if p > s {
                    // Inside the removed run — clamp to the line start.
                    removed_before += p - s;
                }
            }
            p - removed_before
        };
        st.cursor = adjust(st.cursor);
        st.anchor = adjust(st.anchor);
        st.note_text_change();
        drop(st);
        self.mark_dirty();
        self.notify_change();
    }

    /// Handle the caret/selection change for a fresh pointer press. `clicks` is
    /// the multi-click count (1 = single, 2 = double, 3 = triple). A double
    /// selects the word under `off`, a triple the logical line (paragraph);
    /// `shift` extends the existing selection to `off`.
    pub(super) fn begin_pointer_selection(&mut self, off: usize, clicks: u32, shift: bool) {
        if shift {
            self.select_granularity = SelectGranularity::Char;
            self.select_pivot = (off, off);
            self.move_cursor_to(off, /*with_selection=*/ true);
            return;
        }
        let text = self.edit.borrow().text.clone();
        match clicks {
            n if n >= 3 => {
                self.select_granularity = SelectGranularity::Line;
                let (start, end) = paragraph_range_at(&text, off);
                self.select_pivot = (start, end);
                self.set_selection(start, end);
            }
            2 => {
                self.select_granularity = SelectGranularity::Word;
                let (start, end) = word_range_at(&text, off);
                self.select_pivot = (start, end);
                self.set_selection(start, end);
            }
            _ => {
                self.select_granularity = SelectGranularity::Char;
                self.select_pivot = (off, off);
                self.move_cursor_to(off, /*with_selection=*/ false);
            }
        }
    }

    /// Extend the selection during a drag, honouring the granularity the
    /// initiating click established. `off` is the caret byte offset under the
    /// pointer.
    pub(super) fn extend_selection_drag(&mut self, off: usize) {
        match self.select_granularity {
            SelectGranularity::Char => self.move_cursor_to(off, /*with_selection=*/ true),
            SelectGranularity::Word => {
                let text = self.edit.borrow().text.clone();
                let (pivot_start, pivot_end) = self.select_pivot;
                let (cs, ce) = word_range_at(&text, off);
                if off >= pivot_end {
                    self.set_caret_anchor(ce, pivot_start);
                } else {
                    self.set_caret_anchor(cs, pivot_end);
                }
            }
            SelectGranularity::Line => {
                let text = self.edit.borrow().text.clone();
                let (pivot_start, pivot_end) = self.select_pivot;
                let (cs, ce) = paragraph_range_at(&text, off);
                if off >= pivot_end {
                    self.set_caret_anchor(ce, pivot_start);
                } else {
                    self.set_caret_anchor(cs, pivot_end);
                }
            }
        }
    }

    /// Set anchor and cursor to a sorted `[start, end)` selection, caret at the
    /// end.
    pub(super) fn set_selection(&mut self, start: usize, end: usize) {
        let mut st = self.edit.borrow_mut();
        let len = st.text.len();
        st.anchor = start.min(len);
        st.cursor = end.min(len);
    }

    /// Place the caret at `cursor` with the selection anchored at `anchor`
    /// (both clamped to the text length).
    fn set_caret_anchor(&mut self, cursor: usize, anchor: usize) {
        let mut st = self.edit.borrow_mut();
        let len = st.text.len();
        st.cursor = cursor.min(len);
        st.anchor = anchor.min(len);
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
        self.move_lines(dir as isize, with_selection);
    }

    /// Cursor `delta` visual lines up (negative) or down (positive), preserving
    /// the pixel column. Clamps to the first/last visual line. Shared by arrow
    /// navigation ([`move_line`](Self::move_line)) and PageUp/PageDown.
    pub(super) fn move_lines(&mut self, delta: isize, with_selection: bool) {
        if self.cached_lines.is_empty() {
            return;
        }
        let cursor = self.edit.borrow().cursor;
        let cur_line = self.line_for_cursor(cursor) as isize;
        let last = self.cached_lines.len() as isize - 1;
        let target_line = (cur_line + delta).clamp(0, last);
        if target_line == cur_line {
            return;
        }
        let cur_line = cur_line as usize;
        let target_line = target_line as usize;
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

/// One indentation level, code-editor style: spaces, not a hard tab. Matches
/// the plain-Tab insertion the widget already used.
const INDENT: &str = "    ";
/// Number of spaces removed by a single outdent when a line is space-indented.
const TAB_WIDTH: usize = 4;

/// Byte offsets of the start of every logical line (`\n`-delimited) that the
/// selection `[lo, hi)` touches. For an empty selection this is just the
/// caret's line. The exclusive end `hi` is stepped back one byte so a
/// selection that stops exactly at a line start does not drag that next line
/// into an indent/outdent — the caret is "before" it, not on it.
fn touched_line_starts(text: &str, lo: usize, hi: usize) -> Vec<usize> {
    let first = text[..lo].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end_pos = if hi > lo { hi - 1 } else { lo };
    let mut starts = Vec::new();
    let mut s = first;
    loop {
        starts.push(s);
        match text[s..].find('\n') {
            Some(off) => {
                let next = s + off + 1;
                if next > end_pos {
                    break;
                }
                s = next;
            }
            None => break,
        }
    }
    starts
}

/// Bytes of leading indentation a single outdent strips from `line`: one hard
/// tab, or up to [`TAB_WIDTH`] leading spaces (fewer if the line has fewer).
fn leading_outdent_len(line: &str) -> usize {
    if line.starts_with('\t') {
        1
    } else {
        // Every leading space is one byte, so the count is also the byte len.
        line.chars()
            .take_while(|&c| c == ' ')
            .take(TAB_WIDTH)
            .count()
    }
}
