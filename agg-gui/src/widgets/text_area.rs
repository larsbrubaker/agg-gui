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
//! Beyond W5, this widget also backs the standalone **TextEdit demo**
//! (`demo-ui`'s `text_edit_demo.rs`, mirroring egui's `text_edit.rs`), so it
//! grew the following egui-parity capabilities:
//!   * configurable hint / placeholder text ([`with_hint_text`](TextArea::with_hint_text));
//!   * content alignment — horizontal [`TextHAlign`] and vertical [`TextVAlign`],
//!     settable statically or bound to a live `Rc<Cell<_>>`;
//!   * selection introspection ([`selection`](TextArea::selection) /
//!     [`selected_text`](TextArea::selected_text));
//!   * a shared, externally-mutable edit state ([`with_edit_state`](TextArea::with_edit_state)
//!     / [`edit_state`](TextArea::edit_state)) with content-epoch cache
//!     invalidation, so a caller can clear/replace the text or move the cursor
//!     from outside the widget tree;
//!   * a programmatic focus id + cursor-to-start/end helpers;
//!   * a pre-default key-chord interceptor ([`with_key_intercept`](TextArea::with_key_intercept)),
//!     used by the demo for the Ctrl/Cmd+Y "toggle case of selection" shortcut.
//!
//! Deferred (known gaps, filed for later polish):
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
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::focus::FocusId;
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{measure_advance, measure_text_metrics, Font};
use crate::widget::Widget;
use crate::widgets::text_field_core::{next_char_boundary, prev_char_boundary, TextEditState};

/// Horizontal alignment of the wrapped text content inside a [`TextArea`]'s
/// padded inner rect. Mirrors egui's `TextEdit::horizontal_align`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextHAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical alignment of the wrapped text block inside a [`TextArea`]'s padded
/// inner rect. Mirrors egui's `TextEdit::vertical_align`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TextVAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

/// Signature of a pre-default key-chord interceptor. Returns `true` to consume
/// the event and suppress the widget's built-in handling for that key.
pub type KeyIntercept = dyn FnMut(&Key, &Modifiers) -> bool;

/// Signature of a per-visual-line syntax highlighter. Given one wrapped line's
/// rendered text, it returns a list of `(start_byte, end_byte, color)` runs
/// (byte offsets relative to that line). Bytes not covered by any run are
/// drawn in the ambient text colour. Runs are expected to be non-overlapping
/// and sorted; overlaps simply repaint. Keeping it line-oriented sidesteps
/// span-across-wrap bookkeeping and matches the simple fallback highlighter in
/// egui's Code Editor demo.
pub type LineHighlighter = dyn Fn(&str) -> Vec<(usize, usize, crate::color::Color)>;

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

    /// Placeholder shown (dimmed) while the buffer is empty. Mirrors egui's
    /// `TextEdit::hint_text`.
    hint: String,

    /// Static content alignment. Ignored on the axis where a `*_align_cell`
    /// binding is present (the cell wins so external toggles apply live).
    content_h_align: TextHAlign,
    content_v_align: TextVAlign,
    /// Optional live bindings for content alignment — lets a caller flip
    /// alignment from a segmented control without rebuilding the widget.
    h_align_cell: Option<Rc<Cell<TextHAlign>>>,
    v_align_cell: Option<Rc<Cell<TextVAlign>>>,

    /// Stable id for the programmatic focus channel
    /// ([`crate::focus::request_focus`]); `None` opts out.
    focus_request_id: Option<FocusId>,

    /// Pre-default key-chord interceptor. Invoked at the top of `KeyDown`
    /// handling; when it returns `true` the event is consumed and the
    /// built-in key handling is skipped.
    on_key_chord: Option<Rc<RefCell<KeyIntercept>>>,

    /// Optional per-line syntax highlighter (see [`LineHighlighter`]). When
    /// present, each wrapped line is painted as coloured runs instead of a
    /// single ambient-colour string.
    highlighter: Option<Rc<LineHighlighter>>,

    /// Live edit state.  Shared with future undo / clipboard wiring, and with
    /// external callers via [`with_edit_state`](Self::with_edit_state).
    edit: Rc<RefCell<TextEditState>>,

    /// Cached layout — invalidated when text / font / width changes.
    cached_wrap_width: f64,
    cached_lines: Vec<WrappedLine>,
    cached_line_h: f64,
    /// `edit.epoch` observed when `cached_lines` was last (re)built. A
    /// mismatch means the text was mutated through the shared handle by an
    /// external owner, so the wrap cache is stale even at the same width.
    cached_epoch: u64,

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
            hint: "Type here…".to_string(),
            content_h_align: TextHAlign::Left,
            content_v_align: TextVAlign::Top,
            h_align_cell: None,
            v_align_cell: None,
            focus_request_id: None,
            on_key_chord: None,
            highlighter: None,
            edit: Rc::new(RefCell::new(TextEditState::default())),
            cached_wrap_width: -1.0,
            cached_lines: Vec::new(),
            cached_line_h: 0.0,
            cached_epoch: 0,
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
        let epoch = self.edit.borrow().epoch.wrapping_add(1);
        *self.edit.borrow_mut() = TextEditState {
            text: t,
            cursor,
            anchor: cursor,
            epoch,
        };
        self
    }

    /// Placeholder text shown, dimmed, while the buffer is empty.
    pub fn with_hint_text(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// Static horizontal alignment of the wrapped content.
    pub fn with_content_h_align(mut self, a: TextHAlign) -> Self {
        self.content_h_align = a;
        self
    }
    /// Static vertical alignment of the wrapped content block.
    pub fn with_content_v_align(mut self, a: TextVAlign) -> Self {
        self.content_v_align = a;
        self
    }
    /// Bind horizontal alignment to a live cell (wins over the static value).
    pub fn with_h_align_cell(mut self, cell: Rc<Cell<TextHAlign>>) -> Self {
        self.h_align_cell = Some(cell);
        self
    }
    /// Bind vertical alignment to a live cell (wins over the static value).
    pub fn with_v_align_cell(mut self, cell: Rc<Cell<TextVAlign>>) -> Self {
        self.v_align_cell = Some(cell);
        self
    }

    /// Adopt an externally-owned edit state so a caller can read the text /
    /// selection and mutate it (clear, replace, move cursor) from outside the
    /// widget tree. External text mutations must call
    /// [`TextEditState::note_text_change`] so the wrap cache invalidates.
    pub fn with_edit_state(mut self, state: Rc<RefCell<TextEditState>>) -> Self {
        self.edit = state;
        self.cached_wrap_width = -1.0;
        self
    }

    /// Stable id for the programmatic focus channel
    /// ([`crate::focus::request_focus`]).
    pub fn with_focus_id(mut self, id: FocusId) -> Self {
        self.focus_request_id = Some(id);
        self
    }

    /// Install a pre-default key-chord interceptor. It runs before the widget's
    /// built-in key handling on every `KeyDown`; returning `true` consumes the
    /// event and suppresses the default action for that key.
    pub fn with_key_intercept(
        mut self,
        cb: impl FnMut(&Key, &Modifiers) -> bool + 'static,
    ) -> Self {
        self.on_key_chord = Some(Rc::new(RefCell::new(cb)));
        self
    }

    /// Install a per-visual-line syntax highlighter (see [`LineHighlighter`]).
    pub fn with_highlighter(
        mut self,
        cb: impl Fn(&str) -> Vec<(usize, usize, crate::color::Color)> + 'static,
    ) -> Self {
        self.highlighter = Some(Rc::new(cb));
        self
    }

    /// Clone of the shared edit-state handle, for selection/text readback and
    /// external mutation.
    pub fn edit_state(&self) -> Rc<RefCell<TextEditState>> {
        Rc::clone(&self.edit)
    }

    /// Current selection as a sorted `[start, end)` byte range, or `None` when
    /// nothing is selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.edit.borrow().selection_range()
    }

    /// The currently-selected substring (empty when there is no selection).
    pub fn selected_text(&self) -> String {
        let st = self.edit.borrow();
        match st.selection_range() {
            Some((lo, hi)) => st.text[lo..hi].to_string(),
            None => String::new(),
        }
    }

    /// Collapse the selection and place the cursor at the very start.
    pub fn set_cursor_to_start(&mut self) {
        let mut st = self.edit.borrow_mut();
        st.cursor = 0;
        st.anchor = 0;
    }

    /// Collapse the selection and place the cursor at the very end.
    pub fn set_cursor_to_end(&mut self) {
        let mut st = self.edit.borrow_mut();
        let end = st.text.len();
        st.cursor = end;
        st.anchor = end;
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
        // An external owner of the shared edit state may have replaced the
        // text without touching our width or calling `mark_dirty`; the epoch
        // catches that case so the cache never blits stale wrapping.
        let same_epoch = self.cached_epoch == st.epoch;
        if same_width && same_epoch && !self.cached_lines.is_empty() {
            return;
        }
        let lines = wrap_text_indexed(&self.font, &st.text, self.font_size, inner_w.max(1.0));
        self.cached_lines = lines;
        self.cached_wrap_width = inner_w;
        self.cached_epoch = st.epoch;
        // Line height — a little slacker than tight metrics so
        // descenders from line N don't kiss ascenders from N+1.
        self.cached_line_h = self.font_size * 1.35;
    }

    /// Force a re-wrap on the next layout.
    fn mark_dirty(&mut self) {
        self.cached_wrap_width = -1.0;
    }

    // ── Content-alignment geometry ────────────────────────────────────────
    //
    // Alignment shifts where wrapped lines sit inside the padded inner rect.
    // These helpers are the single source of truth for that offset math so
    // paint, cursor overlay, hit-testing and cursor-position queries all agree.

    /// Resolved horizontal alignment (cell binding wins over the static value).
    fn resolved_h_align(&self) -> TextHAlign {
        self.h_align_cell
            .as_ref()
            .map(|c| c.get())
            .unwrap_or(self.content_h_align)
    }

    /// Resolved vertical alignment (cell binding wins over the static value).
    fn resolved_v_align(&self) -> TextVAlign {
        self.v_align_cell
            .as_ref()
            .map(|c| c.get())
            .unwrap_or(self.content_v_align)
    }

    /// Inner content width (widget width minus horizontal padding).
    fn inner_width(&self) -> f64 {
        (self.bounds.width - self.padding * 2.0).max(0.0)
    }

    /// Vertical shift (downward, in the Y-up frame) applied to the whole line
    /// block to honour [`TextVAlign`]. `0` for `Top`.
    fn v_align_shift(&self) -> f64 {
        let content_h = self.cached_lines.len() as f64 * self.cached_line_h;
        let inner_h = (self.bounds.height - self.padding * 2.0).max(0.0);
        let slack = (inner_h - content_h).max(0.0);
        match self.resolved_v_align() {
            TextVAlign::Top => 0.0,
            TextVAlign::Center => slack * 0.5,
            TextVAlign::Bottom => slack,
        }
    }

    /// Y coordinate (Y-up) of the TOP edge of visual line 0.
    fn content_top_y(&self) -> f64 {
        self.bounds.height - self.padding - self.v_align_shift()
    }

    /// Y coordinate (Y-up) of the top edge of visual line `i`.
    fn line_top_y(&self, i: usize) -> f64 {
        self.content_top_y() - i as f64 * self.cached_line_h
    }

    /// Horizontal start (x) of a line's rendered text, honouring [`TextHAlign`].
    fn line_x_start(&self, line: &WrappedLine) -> f64 {
        let line_w = measure_advance(&self.font, &line.text, self.font_size);
        let slack = (self.inner_width() - line_w).max(0.0);
        let shift = match self.resolved_h_align() {
            TextHAlign::Left => 0.0,
            TextHAlign::Center => slack * 0.5,
            TextHAlign::Right => slack,
        };
        self.padding + shift
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
    fn pos_for_cursor(&self, byte_pos: usize) -> Point {
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
        st.note_text_change();
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
        st.note_text_change();
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

mod widget_impl;

#[cfg(test)]
mod tests;
