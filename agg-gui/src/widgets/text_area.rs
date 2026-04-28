//! `TextArea` — a multiline text editor.
//!
//! Built for W5 of the Window Resize Test (egui's "↔ resizable with
//! TextEdit") — a widget that **fills its available area** and lets
//! the user edit a paragraph of text across many wrapped visual
//! lines.  Shares the underlying `TextEditState` with `TextField` so
//! the same keyboard shortcuts / undo semantics are in reach later.
//!
//! # Scope (Stage 4)
//!
//! Covers the behaviour W5 actually needs and what a mobile user
//! would expect from an editable paragraph:
//!   * word-wrap to the widget's inner width;
//!   * typing / backspace / delete / Enter produce visible edits;
//!   * arrow keys navigate by char or visual line;
//!   * click positions cursor; drag selects;
//!   * cursor blink with focus state;
//!   * copy / cut / paste via the standard clipboard shortcuts.
//!
//! Deferred (known gaps, filed for Stage 5 polish):
//!   * word-boundary jumps (Ctrl+arrows) across wrapped visual lines;
//!   * undo / redo;
//!   * input-method composition;
//!   * BiDi and RTL layout.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use web_time::Instant;

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{measure_advance, measure_text_metrics, Font};
use crate::widget::Widget;
use crate::widgets::text_field_core::{next_char_boundary, prev_char_boundary, TextEditState};

fn clipboard_get() -> Option<String> {
    crate::clipboard::get_text()
}

fn clipboard_set(text: &str) {
    crate::clipboard::set_text(text);
}

// ─── Wrapping helper ─────────────────────────────────────────────────────────

/// A single visual line produced by [`wrap_text_indexed`].
#[derive(Clone, Debug)]
struct WrappedLine {
    /// Inclusive byte offset into the source `text` where this visual
    /// line's content begins.
    start: usize,
    /// Exclusive byte offset where this visual line's content ends
    /// (not including a trailing newline).
    end: usize,
    /// Rendered text for this visual line (a substring of the source).
    text: String,
    /// Whether this visual line ended because of an explicit `\n` in
    /// the source (vs. a soft wrap at word boundary).  Used to choose
    /// whether moving the cursor past the end of the line lands on
    /// the next visual line or just past the newline character.
    hard_break: bool,
}

/// Wrap `text` at `max_width` and return the visual lines along with
/// byte-offset ranges back into the source.  Explicit `\n` always
/// produces a line break; between newlines, word-boundary soft wraps
/// keep each visual line ≤ `max_width`.  An empty source still returns
/// one empty line (so the cursor has somewhere to sit).
fn wrap_text_indexed(
    font: &Arc<Font>,
    text: &str,
    font_size: f64,
    max_width: f64,
) -> Vec<WrappedLine> {
    let mut out: Vec<WrappedLine> = Vec::new();
    let mut para_start = 0usize;
    for (rel_end, chunk) in split_keep_newlines(text).enumerate() {
        let _ = rel_end;
        let para = chunk;
        let para_abs_start = para_start;
        let para_abs_end = para_abs_start + para.len();
        // Each paragraph soft-wraps independently.  Walk its char
        // byte indices and fill lines up to `max_width`.
        let mut cursor = 0usize; // byte offset within `para`
        let last_boundary = 0usize;
        while cursor < para.len() {
            // Find the longest prefix of `para[line_start..]` that
            // fits in `max_width`.  Use word boundaries — fall back
            // to the full prefix when no boundary is available (long
            // unbroken token).
            let line_start = cursor;
            let mut fit_end = line_start;
            let mut last_word_end: Option<usize> = None;
            let mut idx = line_start;
            while idx < para.len() {
                let next = next_char_boundary(para, idx);
                let candidate = &para[line_start..next];
                let w = measure_text_metrics(font, candidate, font_size).width;
                if w > max_width && fit_end > line_start {
                    break;
                }
                fit_end = next;
                // Record word boundaries as we pass them.
                if next < para.len() {
                    let next_ch = para[next..].chars().next().unwrap_or(' ');
                    if next_ch.is_whitespace() {
                        last_word_end = Some(next);
                    }
                }
                idx = next;
            }
            // Decide where to break: the last word boundary if we have
            // one AND we're not at the end of the paragraph; else just
            // at `fit_end`.
            let break_at = if fit_end < para.len() && last_word_end.is_some() {
                last_word_end.unwrap()
            } else {
                fit_end.max(next_char_boundary(para, line_start))
            };
            let _ = last_boundary; // reserved for future hyphenation
            let line_text = para[line_start..break_at].trim_end().to_string();
            let abs_start = para_abs_start + line_start;
            let abs_end = para_abs_start + break_at;
            out.push(WrappedLine {
                start: abs_start,
                end: abs_end,
                text: line_text,
                hard_break: false,
            });
            // Skip over the whitespace we just consumed as a separator.
            let mut next_line_start = break_at;
            while next_line_start < para.len() {
                let ch = para[next_line_start..].chars().next().unwrap_or('x');
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                next_line_start = next_char_boundary(para, next_line_start);
            }
            cursor = next_line_start;
            if cursor >= para.len() {
                break;
            }
        }
        // Emit at least one line for an empty paragraph (blank line
        // between \n\n, or a fresh doc with no content).
        if out.is_empty() || out.last().map(|l| l.end).unwrap_or(0) != para_abs_end {
            if para.is_empty() {
                out.push(WrappedLine {
                    start: para_abs_start,
                    end: para_abs_end,
                    text: String::new(),
                    hard_break: false,
                });
            }
        }
        // Mark the paragraph's last visual line as ending with a hard
        // break if the source had a trailing newline (see
        // `split_keep_newlines` contract below).
        let source_end = para_abs_end + 1; // +1 for the consumed '\n', if any
        let had_newline =
            source_end <= text.len() && text.as_bytes().get(para_abs_end) == Some(&b'\n');
        if had_newline {
            if let Some(last) = out.last_mut() {
                last.hard_break = true;
            }
        }
        para_start = if had_newline {
            source_end
        } else {
            para_abs_end
        };
    }
    if out.is_empty() {
        out.push(WrappedLine {
            start: 0,
            end: 0,
            text: String::new(),
            hard_break: false,
        });
    }
    out
}

/// Iterator over paragraph chunks — everything between `\n` boundaries
/// (newline is NOT included in the yielded chunk, but the caller can
/// detect its presence by comparing chunk byte-ranges to the source).
fn split_keep_newlines(text: &str) -> impl Iterator<Item = &str> + '_ {
    // `split('\n')` already gives the right semantics: consecutive \n's
    // yield empty strings so cursor can sit on blank lines, and a
    // trailing \n produces a final empty string (a blank final line).
    text.split('\n')
}

// ─── TextArea widget ─────────────────────────────────────────────────────────

/// A multiline text editor that fills its available area.
pub struct TextArea {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,

    font: Arc<Font>,
    font_size: f64,
    padding: f64,

    /// Live edit state.  Shared with future undo / clipboard wiring.
    edit: Rc<RefCell<TextEditState>>,

    /// Cached layout — invalidated when text / font / width changes.
    cached_wrap_width: f64,
    cached_lines: Vec<WrappedLine>,
    cached_line_h: f64,

    /// Ephemeral input state.
    focused: bool,
    hovered: bool,
    selecting_drag: bool,
    focus_time: Option<Instant>,
    blink_last_phase: Cell<u64>,
}

impl TextArea {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            font,
            font_size: 13.0,
            padding: 8.0,
            edit: Rc::new(RefCell::new(TextEditState::default())),
            cached_wrap_width: -1.0,
            cached_lines: Vec::new(),
            cached_line_h: 0.0,
            focused: false,
            hovered: false,
            selecting_drag: false,
            focus_time: None,
            blink_last_phase: Cell::new(0),
        }
    }

    pub fn with_text(self, text: impl Into<String>) -> Self {
        let t: String = text.into();
        let cursor = t.len();
        *self.edit.borrow_mut() = TextEditState {
            text: t,
            cursor,
            anchor: cursor,
        };
        self
    }
    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }
    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p;
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    /// Current text.  Cheap — clones the underlying `String`.
    pub fn text(&self) -> String {
        self.edit.borrow().text.clone()
    }

    /// Current byte-offset cursor position (for tests and inspectors).
    pub fn cursor(&self) -> usize {
        self.edit.borrow().cursor
    }

    /// Count of visual lines at the last layout pass (cache).
    pub fn visual_line_count(&self) -> usize {
        self.cached_lines.len()
    }

    /// Ensure the wrap cache matches the current text + width.
    fn refresh_wrap(&mut self, inner_w: f64) {
        let st = self.edit.borrow();
        let same_width = (self.cached_wrap_width - inner_w).abs() < 0.5;
        if same_width && !self.cached_lines.is_empty() {
            // Wrap is expensive; skip when nothing that affects it
            // changed.  `text` changes go through `mark_dirty` which
            // resets `cached_wrap_width` to −1.
            return;
        }
        let lines = wrap_text_indexed(&self.font, &st.text, self.font_size, inner_w.max(1.0));
        self.cached_lines = lines;
        self.cached_wrap_width = inner_w;
        // Line height — a little slacker than tight metrics so
        // descenders from line N don't kiss ascenders from N+1.
        self.cached_line_h = self.font_size * 1.35;
    }

    /// Force a re-wrap on the next layout.
    fn mark_dirty(&mut self) {
        self.cached_wrap_width = -1.0;
    }

    /// Locate the (line_index, byte_pos_in_text) that the given cursor
    /// byte offset lives on.  Returns `(0, 0)` on empty content.
    fn line_for_cursor(&self, byte_pos: usize) -> usize {
        for (i, l) in self.cached_lines.iter().enumerate() {
            if byte_pos >= l.start && byte_pos <= l.end {
                return i;
            }
        }
        self.cached_lines.len().saturating_sub(1)
    }

    /// Hit-test a widget-local point to a text byte offset.  Clamps to
    /// `[0, text.len()]` at the edges.  `local` is Y-UP.
    fn byte_offset_at(&self, local: Point) -> usize {
        if self.cached_lines.is_empty() || self.cached_line_h <= 0.0 {
            return 0;
        }
        // Visual lines stack top-to-bottom; Y-up flips their y coords.
        // Line 0 sits at the top (high Y), line N at the bottom (low Y).
        let inner_top_y = self.bounds.height - self.padding;
        let rel_from_top = inner_top_y - local.y;
        let mut line_idx = (rel_from_top / self.cached_line_h).floor() as isize;
        if line_idx < 0 {
            line_idx = 0;
        }
        if line_idx as usize >= self.cached_lines.len() {
            line_idx = self.cached_lines.len() as isize - 1;
        }
        let line = &self.cached_lines[line_idx as usize];
        // X hit test: walk chars in the line's rendered text and pick
        // the nearest grapheme boundary.
        let pad_x = self.padding;
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
    fn pos_for_cursor(&self, byte_pos: usize) -> Point {
        if self.cached_lines.is_empty() {
            return Point::ORIGIN;
        }
        let line_idx = self.line_for_cursor(byte_pos);
        let line = &self.cached_lines[line_idx];
        let offset = byte_pos.saturating_sub(line.start).min(line.text.len());
        let x = self.padding + measure_advance(&self.font, &line.text[..offset], self.font_size);
        // Y-up: line i top-edge = inner_top - i * line_h.
        let inner_top_y = self.bounds.height - self.padding;
        let line_top = inner_top_y - line_idx as f64 * self.cached_line_h;
        let line_bottom = line_top - self.cached_line_h;
        Point::new(x, line_bottom)
    }

    /// Insert a string at the cursor, replacing any active selection.
    fn insert_str(&mut self, s: &str) {
        let mut st = self.edit.borrow_mut();
        let (lo, hi) = (st.cursor.min(st.anchor), st.cursor.max(st.anchor));
        // Make sure we slice at grapheme boundaries.
        let lo = lo.min(st.text.len());
        let hi = hi.min(st.text.len());
        st.text.replace_range(lo..hi, s);
        st.cursor = lo + s.len();
        st.anchor = st.cursor;
        drop(st);
        self.mark_dirty();
    }

    /// Delete the current selection, or (if empty) `dir` chars toward
    /// the supplied side.  `-1` = backspace, `+1` = delete, `0` = just
    /// collapse the selection (cut path).
    fn delete(&mut self, dir: i32) {
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
        drop(st);
        self.mark_dirty();
    }

    /// Move cursor to an absolute byte offset.  `with_selection=false`
    /// collapses anchor with cursor; `true` leaves the anchor alone
    /// so a selection is extended.
    fn move_cursor_to(&mut self, pos: usize, with_selection: bool) {
        let mut st = self.edit.borrow_mut();
        let p = pos.min(st.text.len());
        st.cursor = p;
        if !with_selection {
            st.anchor = p;
        }
    }

    /// Cursor one char left / right.
    fn move_char(&mut self, dir: i32, with_selection: bool) {
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
    fn move_line(&mut self, dir: i32, with_selection: bool) {
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
        // Preserve horizontal position (pixel column, not byte column).
        let cur_x = self.pos_for_cursor(cursor).x - self.padding;
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

mod widget_impl;
