//! The caret and selection API agg-sharp's `InternalTextEditWidget` exposes,
//! on [`TextArea`]: line spacing as a multiple of the em (C#'s
//! `TypeFacePrinter.LineSpacing`), the caret's offset from the start of the
//! text (`InsertBarPosition`), the caret and selection-start character indices
//! with C#'s out-of-range-tolerant setters (`CharIndexToInsertBefore`,
//! `SelectionIndexToStartBefore`) and `CopySelection`. The laid-out content
//! height (C#'s auto-sized `Height`) is `content_height` in `scroll.rs`.
//!
//! Split out of `text_area.rs` (the widget) next to `geometry.rs` (the
//! byte-offset ↔ widget-local mapping these build on).

use super::*;

/// Byte offset of character `char_index` in `text`, clamped to its end.
fn byte_of_char(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(byte, _)| byte)
}

/// Character index of byte offset `byte` in `text`.
fn char_of_byte(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// C#'s index setters take any `int`: below zero is the start, past the end
/// is the end.
fn clamp_char_index(text: &str, index: i32) -> usize {
    let len = text.chars().count();
    usize::try_from(index).map_or(0, |i| i.min(len))
}

impl TextArea {
    /// Line advance as a multiple of the font size (C#'s
    /// `TypeFacePrinter.LineSpacing`). agg-gui's default is 1.35; C#'s
    /// `TextEditWidget` advances one em per line (1.0). With a line spacing
    /// set, the widget's minimum height is one line box (C#'s auto-sized edit
    /// widget) rather than the default's roomier 1.6 em.
    pub fn with_line_spacing(mut self, spacing: f64) -> Self {
        self.line_spacing = Some(spacing);
        self.mark_dirty();
        self
    }

    /// Distance from one line's top to the next's, in pixels.
    pub(super) fn line_advance(&self) -> f64 {
        self.font_size * self.line_spacing.unwrap_or(DEFAULT_LINE_SPACING)
    }

    /// The smallest text height `layout` gives the widget: room for one line.
    pub(super) fn min_text_height(&self) -> f64 {
        match self.line_spacing {
            Some(_) => self.line_advance(),
            None => self.font_size * 1.6,
        }
    }

    /// C# `InsertBarPosition`: the caret's offset from the start of the text
    /// at the last layout, Y-up — `x` is the advance from its line's start,
    /// `y` is `0` on the first line and minus one line advance per line
    /// below it. Independent of padding, alignment and scrolling.
    pub fn insert_bar_position(&self) -> Point {
        if self.cached_lines.is_empty() {
            return Point::ORIGIN;
        }
        let st = self.edit.borrow();
        let line_idx = self.line_for_cursor(st.cursor);
        let line = &self.cached_lines[line_idx];
        let seg_end = st.cursor.clamp(line.start, line.end);
        let x = st
            .text
            .get(line.start..seg_end)
            .map_or(0.0, |seg| measure_advance(&self.font, seg, self.font_size));
        Point::new(x, -(line_idx as f64) * self.cached_line_h)
    }

    /// C# `CharIndexToInsertBefore`: the caret as a character index.
    pub fn char_index_to_insert_before(&self) -> usize {
        let st = self.edit.borrow();
        char_of_byte(&st.text, st.cursor)
    }

    /// C# `SelectionIndexToStartBefore`: the selection's fixed end (the
    /// anchor) as a character index; equal to the caret when nothing is
    /// selected.
    pub fn selection_index_to_start_before(&self) -> usize {
        let st = self.edit.borrow();
        char_of_byte(&st.text, st.anchor)
    }

    /// C# `CharIndexToInsertBefore = index`: move the caret, keeping the
    /// selection's anchor. Out-of-range indices clamp to the text, as C#'s
    /// `Selection` and `CopySelection` read them.
    pub fn set_char_index_to_insert_before(&mut self, index: i32) {
        let mut st = self.edit.borrow_mut();
        let byte = byte_of_char(&st.text, clamp_char_index(&st.text, index));
        st.cursor = byte;
        drop(st);
        crate::animation::request_draw();
    }

    /// C# `SelectionIndexToStartBefore = index`: move the selection's anchor,
    /// clamped like [`set_char_index_to_insert_before`](Self::set_char_index_to_insert_before).
    /// A `TextArea` is selecting whenever anchor and caret differ, so C#'s
    /// `Selecting = true` has no separate switch.
    pub fn set_selection_index_to_start_before(&mut self, index: i32) {
        let mut st = self.edit.borrow_mut();
        let byte = byte_of_char(&st.text, clamp_char_index(&st.text, index));
        st.anchor = byte;
        drop(st);
        crate::animation::request_draw();
    }

    /// C# `CopySelection`: put the selected text on the clipboard. Nothing is
    /// copied when nothing is selected.
    pub fn copy_selection(&mut self) {
        self.clipboard_copy();
    }
}
