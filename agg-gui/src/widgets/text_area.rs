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
use crate::widget::{BackbufferCache, BackbufferMode, Widget};
use crate::widgets::scrollbar::ScrollbarAxis;
use crate::widgets::multi_click::{MultiClickTracker, SelectGranularity};
use crate::widgets::text_field_core::{
    next_char_boundary, next_word_boundary, paragraph_range_at, prev_char_boundary,
    prev_word_boundary, word_range_at, TextEditState,
};

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

/// Snapshot of every input that affects the cached backbuffer bitmap
/// (background + selection band + wrapped text). `layout` compares the current
/// sig against the last one and drops the LCD/RGBA cache on any difference, so
/// typing / selecting re-rasterise but an idle (blinking-only) frame just
/// re-blits the cached pixels.
///
/// Deliberately excludes cursor-blink phase, the floating scrollbar, and the
/// border — all paint in `paint_overlay` after the cache blit, so they never
/// force a re-raster. It also excludes the *raw* scroll offset: the widget
/// rasters an over-scan band anchored in content space, so plain scrolling
/// within the band only moves the blit offset. Only the band *anchor* and its
/// margins are tracked here, so re-anchoring (offset left the band) still
/// re-rasters exactly once. Typography- and theme-driven invalidation (font
/// swap, LCD/hinting toggle, dark/light flip) is handled by the framework via
/// the epoch checks in `paint_subtree_backbuffered`, so this sig only tracks the
/// widget's own state.
#[derive(Clone, PartialEq)]
struct TextAreaSig {
    epoch: u64,
    cursor: usize,
    anchor: usize,
    focused: bool,
    hovered: bool,
    /// The band anchor (content-space offset the backbuffer is rastered at) and
    /// its over-scan extents — NOT the live scroll offset. Scrolling within the
    /// band leaves these unchanged, so the cache stays valid and only the blit
    /// offset moves; re-anchoring (offset left the band) changes the anchor and
    /// forces one re-raster. `band_active` distinguishes the bounds-sized path.
    band_active: bool,
    band_anchor_bits: u64,
    band_over_top_bits: u64,
    band_over_bottom_bits: u64,
    w_bits: u64,
    h_bits: u64,
    h_align: TextHAlign,
    v_align: TextVAlign,
    font_size_bits: u64,
}

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

    /// Fired after any text mutation (typing, delete, paste, cut, and
    /// key-intercept edits that advance the content epoch). Mirrors
    /// TextField's `on_change`. Builder + dispatcher live in
    /// `text_area/callbacks.rs`.
    on_change: Option<Box<dyn FnMut(&str)>>,

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

    /// Internal vertical scroll state. Reuses `ScrollView`'s [`ScrollbarAxis`]
    /// so overflow math, thumb geometry, drag/hover and painting stay
    /// pixel-consistent with the app's other scroll bars. `offset` is the
    /// scroll distance from the top (0 = first line visible); it is folded into
    /// [`content_top_y`](Self::content_top_y). See `text_area/scroll.rs`.
    vbar: ScrollbarAxis,

    /// Optional publish channel mirroring `vbar.offset`. Sibling widgets that
    /// live outside this widget's subtree — e.g. the Code Editor demo's
    /// line-number gutter — have no access to the internal scroll state, yet
    /// must follow the viewport pixel-for-pixel. Whenever the offset moves the
    /// widget writes it here so the sibling can read the same value it paints
    /// with. See [`with_scroll_watch`](Self::with_scroll_watch).
    scroll_watch: Option<Rc<Cell<f64>>>,

    /// Cursor byte offset observed at the previous `layout`. `None` until the
    /// first layout. A change between layouts that the widget's own edit funnel
    /// didn't already scroll for (i.e. an *external* mutation of the shared
    /// [`TextEditState`], as the demo's "start"/"end" buttons do) triggers
    /// [`ensure_cursor_visible`](Self::ensure_cursor_visible) so programmatic
    /// caret moves scroll into view just like typed navigation.
    last_layout_cursor: Option<usize>,

    /// Ephemeral input state.
    focused: bool,
    hovered: bool,
    selecting_drag: bool,
    focus_time: Option<Instant>,
    blink_last_phase: Cell<u64>,

    /// Multi-click (single / double / triple) detection and the granularity of
    /// the active selection drag. A double-click selects the word, a
    /// triple-click the logical line (paragraph), and dragging afterwards
    /// extends by whole words / paragraphs. `select_pivot` is the byte range
    /// the initiating click selected.
    multi_click: MultiClickTracker,
    select_granularity: SelectGranularity,
    select_pivot: (usize, usize),

    /// Per-widget CPU bitmap cache. Routes the whole editor (bg + selection +
    /// text) through the same LCD-subpixel / grayscale pipeline as `Label` and
    /// `TextField`: `paint_subtree_backbuffered` renders this subtree into an
    /// `LcdBuffer` (LCD on) or RGBA `Framebuffer` (LCD off) and blits it, so the
    /// editor gets the app's best text rendering by default and follows the
    /// System settings live.
    cache: BackbufferCache,
    /// Last painted signature; a change invalidates [`Self::cache`] in `layout`.
    last_sig: Option<TextAreaSig>,

    /// Over-scan band state (anchor + margins) so scrolling within the band is a
    /// pure blit offset instead of a re-raster. Recomputed each `layout`; see
    /// `text_area/band.rs`.
    band: band::BandState,
    /// `Some(anchor)` only while painting into the band buffer, so content
    /// geometry uses the band's fixed anchor there while hit-test / caret /
    /// scroll math outside paint keep using the live offset. See
    /// [`Self::content_top_y`].
    render_band_offset: Cell<Option<f64>>,
    /// Count of real backbuffer re-rasters (bumped in `paint`). Introspection
    /// hook for the band tests — scrolling within a band must not bump it.
    raster_count: Cell<u64>,
    /// Count of *partial* (dirty-line-strip) re-rasters (bumped in `paint` when
    /// it repaints only the edited strip). Introspection hook for the edit
    /// tests — a localized edit must bump this, a re-anchor must not.
    strip_raster_count: Cell<u64>,
    /// Visual-line index range `(first, last)` that changed in the last
    /// `refresh_wrap` re-wrap, plus whether the total line count changed.
    /// `None` when nothing re-wrapped or the width changed (full re-flow).
    /// Feeds [`Self::plan_dirty_strip`]. Transient (recomputed each layout).
    wrap_change: Option<(usize, usize, bool)>,
    /// Inclusive visual-line range to repaint into the retained band buffer for
    /// a strip-only edit; `None` means repaint the whole band. Set in `layout`
    /// (see [`Self::plan_dirty_strip`]), consumed + cleared in `paint`.
    render_dirty_lines: Cell<Option<(usize, usize)>>,

    /// Default-on right-click Cut/Copy/Paste/Select-All menu. Opt out with
    /// [`with_context_menu(false)`](Self::with_context_menu).
    context_menu: crate::widgets::text_context_menu::TextContextMenu,
    context_menu_enabled: bool,
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
            on_change: None,
            edit: Rc::new(RefCell::new(TextEditState::default())),
            cached_wrap_width: -1.0,
            cached_lines: Vec::new(),
            cached_line_h: 0.0,
            cached_epoch: 0,
            vbar: ScrollbarAxis {
                enabled: true,
                ..ScrollbarAxis::default()
            },
            scroll_watch: None,
            last_layout_cursor: None,
            focused: false,
            hovered: false,
            selecting_drag: false,
            focus_time: None,
            blink_last_phase: Cell::new(0),
            multi_click: MultiClickTracker::default(),
            select_granularity: SelectGranularity::default(),
            select_pivot: (0, 0),
            cache: BackbufferCache::default(),
            last_sig: None,
            band: band::BandState::default(),
            render_band_offset: Cell::new(None),
            raster_count: Cell::new(0),
            strip_raster_count: Cell::new(0),
            wrap_change: None,
            render_dirty_lines: Cell::new(None),
            context_menu: crate::widgets::text_context_menu::TextContextMenu::new(),
            context_menu_enabled: true,
        }
    }

    /// Build the backbuffer-cache invalidation signature from current state.
    fn cache_sig(&self) -> TextAreaSig {
        let st = self.edit.borrow();
        TextAreaSig {
            epoch: st.epoch,
            cursor: st.cursor,
            anchor: st.anchor,
            focused: self.focused,
            hovered: self.hovered,
            band_active: self.band.active,
            band_anchor_bits: self.band.anchor.to_bits(),
            band_over_top_bits: self.band.over_top.to_bits(),
            band_over_bottom_bits: self.band.over_bottom.to_bits(),
            w_bits: self.bounds.width.to_bits(),
            h_bits: self.bounds.height.to_bits(),
            h_align: self.resolved_h_align(),
            v_align: self.resolved_v_align(),
            font_size_bits: self.font_size.to_bits(),
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
            // No re-wrap: leave `wrap_change` as `layout` reset it, so a second
            // `refresh_wrap` in the same pass (via `sync_scroll`) can't clobber
            // the change range the first, re-wrapping call recorded.
            return;
        }
        let lines = wrap_text_indexed(&self.font, &st.text, self.font_size, inner_w.max(1.0));
        // Diff old vs new visual lines to find the changed range, so an in-place
        // edit can re-raster just that strip. Runs whenever we re-wrap over an
        // existing wrap (a fresh build has nothing to diff). The re-wrap flag
        // `mark_dirty` sets clears `cached_wrap_width`, so we can't tell an edit
        // from a genuine width change here — but `plan_dirty_strip` rejects the
        // strip when the width actually changed (its `w_bits` guard), so a
        // width re-flow safely falls back to a full band repaint.
        self.wrap_change = if self.cached_lines.is_empty() {
            None
        } else {
            band::diff_line_range(&self.cached_lines, &lines)
        };
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
    ///
    /// Folds in the internal vertical scroll offset: as the user scrolls down
    /// (`vbar.offset` grows), line 0 rises above the visible top edge and lower
    /// lines come into view. Every geometry query (paint, hit-test, cursor
    /// overlay, scroll-to-cursor) routes through here, so they all agree.
    fn content_top_y(&self) -> f64 {
        // While rastering the over-scan band, positioning uses the band's fixed
        // anchor (set in `paint`), so the cached bitmap is stable as the user
        // scrolls and only the blit offset moves. Everywhere else (hit-test,
        // caret overlay, scroll math) uses the live offset.
        let offset = self.render_band_offset.get().unwrap_or(self.vbar.offset);
        self.bounds.height - self.padding - self.v_align_shift() + offset
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

}

mod band;
mod callbacks;
mod context_menu;
mod edit_ops;
mod geometry;
mod scroll;
mod widget_impl;

#[cfg(test)]
mod band_tests;
#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod redraw_tests;
#[cfg(test)]
mod scroll_watch_tests;
