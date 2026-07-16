//! Internal helpers for `TextField`: UTF-8 navigation, word boundaries,
//! hit-test geometry, shared edit state, and the undo command type.

use std::cell::RefCell;
use std::rc::Rc;

use crate::text::measure_advance;
use crate::text::Font;
use crate::undo::UndoRedoCommand;

// ---------------------------------------------------------------------------
// UTF-8 boundary helpers
// ---------------------------------------------------------------------------

pub fn prev_char_boundary(s: &str, byte_pos: usize) -> usize {
    let mut pos = byte_pos;
    loop {
        if pos == 0 {
            return 0;
        }
        pos -= 1;
        if s.is_char_boundary(pos) {
            return pos;
        }
    }
}

pub fn next_char_boundary(s: &str, byte_pos: usize) -> usize {
    let mut pos = byte_pos + 1;
    while pos <= s.len() {
        if s.is_char_boundary(pos) {
            return pos;
        }
        pos += 1;
    }
    s.len()
}

// ---------------------------------------------------------------------------
// Word-boundary helpers
// ---------------------------------------------------------------------------

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Ctrl+Right: advance to end of next token (skip whitespace then non-whitespace).
pub fn next_word_boundary(s: &str, pos: usize) -> usize {
    let mut chars = s[pos..].char_indices().peekable();
    let mut advanced = 0usize;
    // skip leading whitespace
    while let Some(&(i, c)) = chars.peek() {
        if !c.is_whitespace() {
            break;
        }
        advanced = i + c.len_utf8();
        chars.next();
    }
    // skip non-whitespace
    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() {
            break;
        }
        advanced = i + c.len_utf8();
        chars.next();
    }
    pos + advanced
}

/// Ctrl+Left: retreat to start of previous token.
pub fn prev_word_boundary(s: &str, pos: usize) -> usize {
    if pos == 0 {
        return 0;
    }
    let chars: Vec<(usize, char)> = s[..pos].char_indices().collect();
    let mut i = chars.len();
    while i > 0 && chars[i - 1].1.is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].1.is_whitespace() {
        i -= 1;
    }
    if i < chars.len() {
        chars[i].0
    } else {
        0
    }
}

/// Returns `[start, end)` byte range of the word under `byte_pos`
/// (used for double-click word selection).
pub fn word_range_at(s: &str, byte_pos: usize) -> (usize, usize) {
    let anchor_class = is_word_char(s[byte_pos..].chars().next().unwrap_or(' '));
    // walk back
    let start = {
        let mut p = byte_pos;
        while p > 0 {
            let prev = prev_char_boundary(s, p);
            let c = s[prev..p].chars().next().unwrap_or(' ');
            if is_word_char(c) != anchor_class {
                break;
            }
            p = prev;
        }
        p
    };
    // walk forward
    let end = {
        let mut p = byte_pos;
        for (_, c) in s[byte_pos..].char_indices() {
            if is_word_char(c) != anchor_class {
                break;
            }
            p += c.len_utf8();
        }
        p
    };
    (start, end)
}

/// Returns the `[start, end)` byte range of the *logical* line (paragraph)
/// containing `byte_pos` — everything between the surrounding `\n`s, excluding
/// the newlines themselves. Used for triple-click line selection in the
/// multiline editors. "Logical" means the source paragraph, not a soft-wrapped
/// visual line, so a triple-click grabs the whole paragraph even when it wraps.
pub fn paragraph_range_at(s: &str, byte_pos: usize) -> (usize, usize) {
    let pos = byte_pos.min(s.len());
    let start = s[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = s[pos..].find('\n').map(|i| pos + i).unwrap_or(s.len());
    (start, end)
}

// ---------------------------------------------------------------------------
// X-coordinate ↔ byte-offset
// ---------------------------------------------------------------------------

/// Byte offset of the character boundary closest to `target_x` in rendered text.
pub fn byte_at_x(font: &Font, text: &str, font_size: f64, target_x: f64) -> usize {
    if target_x <= 0.0 {
        return 0;
    }
    let mut prev_x = 0.0f64;
    let mut prev_pos = 0usize;
    for (i, c) in text.char_indices() {
        let x = measure_advance(font, &text[..i], font_size);
        let mid = (prev_x + x) * 0.5;
        if target_x < mid {
            return prev_pos;
        }
        prev_x = x;
        prev_pos = i;
        let _ = c;
    }
    let total = measure_advance(font, text, font_size);
    let mid = (prev_x + total) * 0.5;
    if target_x < mid {
        prev_pos
    } else {
        text.len()
    }
}

// ---------------------------------------------------------------------------
// Shared edit state
// ---------------------------------------------------------------------------

/// The mutable editing state shared between `TextField` and its undo commands.
///
/// `epoch` is a monotonically-increasing text-content revision. It lets a
/// widget that caches something derived from `text` (e.g. `TextArea`'s wrapped
/// visual lines) detect when the text was mutated *through a different owner of
/// the same shared handle* — the case where the widget's own internal
/// invalidation never ran. Bump it via [`note_text_change`](Self::note_text_change)
/// whenever `text` is modified through a shared reference; cursor/anchor-only
/// moves must NOT bump it (they don't affect wrapping).
#[derive(Clone, Default)]
pub struct TextEditState {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
    pub epoch: u64,
}

impl TextEditState {
    /// Record that `text` changed: advances the content [`epoch`](Self::epoch).
    #[inline]
    pub fn note_text_change(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// Sorted `[start, end)` byte range of the current selection, or `None`
    /// when the selection is empty (cursor == anchor).
    #[inline]
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let lo = self.cursor.min(self.anchor);
        let hi = self.cursor.max(self.anchor);
        if hi > lo {
            Some((lo, hi))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Undo command for text edits
// ---------------------------------------------------------------------------

/// Stores before/after snapshots of `TextEditState` and a shared reference
/// to the live state so that undo/redo can restore it.
pub struct TextEditCommand {
    pub name: &'static str,
    pub before: TextEditState,
    pub after: TextEditState,
    pub target: Rc<RefCell<TextEditState>>,
}

impl UndoRedoCommand for TextEditCommand {
    fn name(&self) -> &str {
        self.name
    }
    fn do_it(&mut self) {
        *self.target.borrow_mut() = self.after.clone();
    }
    fn undo_it(&mut self) {
        *self.target.borrow_mut() = self.before.clone();
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod word_line_tests {
    use super::{paragraph_range_at, word_range_at};

    #[test]
    fn word_range_selects_alnum_run() {
        let s = "hello world";
        // Click inside "hello".
        assert_eq!(word_range_at(s, 2), (0, 5));
        // Click inside "world".
        assert_eq!(word_range_at(s, 8), (6, 11));
    }

    #[test]
    fn word_range_stops_at_punctuation() {
        let s = "foo.bar";
        // "foo" and "bar" are separate words; the '.' is its own run.
        assert_eq!(word_range_at(s, 1), (0, 3));
        assert_eq!(word_range_at(s, 5), (4, 7));
        // Clicking on the punctuation selects just the punctuation run.
        assert_eq!(word_range_at(s, 3), (3, 4));
    }

    #[test]
    fn word_range_at_word_edge() {
        let s = "cat dog";
        // Right at the boundary between "cat" and the space: the char at the
        // click position is the space, so the whitespace run is selected.
        assert_eq!(word_range_at(s, 3), (3, 4));
        // One byte earlier is still inside "cat".
        assert_eq!(word_range_at(s, 2), (0, 3));
    }

    #[test]
    fn word_range_includes_underscore() {
        let s = "snake_case here";
        assert_eq!(word_range_at(s, 3), (0, 10));
    }

    #[test]
    fn paragraph_range_single_line() {
        let s = "just one line";
        assert_eq!(paragraph_range_at(s, 4), (0, s.len()));
    }

    #[test]
    fn paragraph_range_middle_line() {
        let s = "first\nsecond\nthird";
        // Offset 8 is inside "second".
        assert_eq!(paragraph_range_at(s, 8), (6, 12));
    }

    #[test]
    fn paragraph_range_last_line_no_trailing_newline() {
        let s = "a\nbb\nccc";
        assert_eq!(paragraph_range_at(s, 7), (5, 8));
    }

    #[test]
    fn paragraph_range_on_blank_line() {
        let s = "a\n\nb";
        // Offset 2 is the empty line between the two newlines.
        assert_eq!(paragraph_range_at(s, 2), (2, 2));
    }
}
