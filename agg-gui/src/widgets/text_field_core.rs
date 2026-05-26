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
#[derive(Clone, Default)]
pub struct TextEditState {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
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
