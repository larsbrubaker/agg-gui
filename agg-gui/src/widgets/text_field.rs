//! `TextField` — single-line editable text input.
//!
//! See [`text_field_core`](super::text_field_core) for the internal helpers,
//! shared edit state, and undo command type.
//!
//! Feature set mirrors C# agg-sharp `InternalTextEditWidget`:
//! - Character / word navigation (arrows, Ctrl+arrow, Home, End)
//! - Keyboard selection (Shift+movement), Ctrl+A select-all
//! - Mouse click to position cursor, drag to extend selection
//! - Double-click to select the word under the cursor
//! - Cut / Copy / Paste (Ctrl+X/C/V, Shift+Del, Ctrl/Shift+Ins) — requires
//!   the `clipboard` crate feature; silently no-ops without it
//! - Undo / Redo via the shared [`UndoBuffer`](crate::undo::UndoBuffer)
//! - Blinking cursor (500 ms half-period from the moment focus is gained)
//! - Horizontal scroll to keep cursor visible
//! - Placeholder text, read-only mode, SelectAllOnFocus
//! - Callbacks: on_change, on_enter, on_edit_complete

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

// web-time provides a WASM-compatible Instant (uses performance.now() in the
// browser; falls back to Instant on native).
use web_time::Instant;

use super::text_field_core::{
    byte_at_x, next_char_boundary, next_word_boundary, prev_char_boundary, prev_word_boundary,
    word_range_at, TextEditCommand, TextEditState,
};
use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{measure_advance, Font};
use crate::undo::UndoBuffer;
use crate::widget::{BackbufferCache, BackbufferMode, Widget};

// ---------------------------------------------------------------------------
// Clipboard stubs
// ---------------------------------------------------------------------------

mod binding;
mod clipboard;
mod filter;
mod keyboard_mode;
mod layout_builders;
mod theme;
mod widget_impl;

use clipboard::{clipboard_get, clipboard_set};
pub use theme::TextFieldTheme;

// ---------------------------------------------------------------------------
// TextField
// ---------------------------------------------------------------------------

/// Single-line editable text field.
pub struct TextField {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,

    // All mutable editing state lives here — shared with undo commands.
    edit: Rc<RefCell<TextEditState>>,

    // Undo/redo history.
    undo: UndoBuffer,

    // Pending coalesced insert command (committed when action type changes).
    pending_insert: Option<TextEditCommand>,

    // Snapshot of text when focus was gained — used to decide if on_edit_complete fires.
    text_on_focus: String,

    // Font
    font: Arc<Font>,
    font_size: f64,

    // Editing options
    pub read_only: bool,
    pub select_all_on_focus: bool,
    /// When `true`, every character is displayed as '•' (U+2022).
    /// The actual text is stored and edited normally; only the render is masked.
    pub password_mode: bool,

    // Interaction state
    focused: bool,
    hovered: bool,
    mouse_down: bool,
    scroll_x: f64,

    // Cursor blink: set to Some(Instant::now()) on FocusGained.
    focus_time: Option<Instant>,
    // Blink phase (floor(elapsed_ms / 500)) last drawn by `paint_overlay`.
    // `needs_draw` compares the current phase against this and reports
    // dirty when they diverge — i.e. the host-observed time has crossed a
    // flip boundary since the last paint.  `Cell` so the check can happen
    // from a `&self` method.  Initialised far out of range so the first
    // paint after focus always writes the real phase.
    blink_last_phase: std::cell::Cell<u64>,

    // Double-click detection.
    last_click_time: Option<Instant>,

    // Content
    pub placeholder: String,

    // Layout
    pub padding: f64,

    // Callbacks
    on_change: Option<Box<dyn FnMut(&str)>>,
    on_enter: Option<Box<dyn FnMut(&str)>>,
    on_edit_complete: Option<Box<dyn FnMut(&str)>>,
    text_cell: Option<Rc<RefCell<String>>>,

    /// Per-character allow-list. See [`with_char_filter`].
    char_filter: Option<Rc<dyn Fn(char) -> bool>>,

    /// Preferred on-screen-keyboard layer when this field is focused.
    /// `Rc<Cell<_>>` so external code (e.g. a settings radio in the
    /// demo) can swap the mode without rebuilding the widget tree —
    /// the next focus event picks up the new value.  See
    /// `text_field/keyboard_mode.rs` for the builders.
    keyboard_mode: Rc<Cell<crate::widgets::on_screen_keyboard::KeyboardInputMode>>,

    /// Per-widget colour overrides — `None` colours fall back to
    /// the ambient `visuals()` palette. Set via [`with_theme`].
    pub theme: TextFieldTheme,

    // ── Backbuffer cache ─────────────────────────────────────────────
    //
    // Cache holds bg + text + selection + border.  Cursor draws in
    // `paint_overlay` directly on the outer ctx AFTER the cache blit
    // so cursor-blink state flips (twice per second) don't invalidate
    // the cache.  Sig deliberately excludes `blink_visible`.
    cache: BackbufferCache,
    last_sig: Option<TextFieldSig>,
}

#[derive(Clone, PartialEq)]
struct TextFieldSig {
    text: String,
    cursor: usize,
    anchor: usize,
    focused: bool,
    hovered: bool,
    scroll_x_bits: u64,
    w_bits: u64,
    h_bits: u64,
    // Font identity + size: the cached bitmap was rasterised with a specific
    // typeface at a specific point size, so any live swap in the System
    // window (which runs through `font_settings::set_system_font` /
    // `set_font_size_scale`) must invalidate — otherwise the stale bitmap
    // keeps blitting until some other field in the sig happens to change
    // (e.g. the user hovers the control, which flips `hovered`).
    font_ptr: usize,
    font_size_bits: u64,
}

impl TextField {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            edit: Rc::new(RefCell::new(TextEditState::default())),
            undo: UndoBuffer::new(),
            pending_insert: None,
            text_on_focus: String::new(),
            font,
            font_size: 14.0,
            read_only: false,
            select_all_on_focus: false,
            password_mode: false,
            focused: false,
            hovered: false,
            mouse_down: false,
            scroll_x: 0.0,
            focus_time: None,
            blink_last_phase: std::cell::Cell::new(u64::MAX),
            last_click_time: None,
            placeholder: String::new(),
            padding: 8.0,
            on_change: None,
            on_enter: None,
            on_edit_complete: None,
            text_cell: None,
            char_filter: None,
            keyboard_mode: Rc::new(Cell::new(
                crate::widgets::on_screen_keyboard::KeyboardInputMode::default(),
            )),
            theme: TextFieldTheme::default(),
            cache: BackbufferCache::default(),
            last_sig: None,
        }
    }

    /// Currently-active font — honours the thread-local system-font override
    /// (`font_settings::current_system_font`) so changes in the System window
    /// propagate live without a widget-tree rebuild.  Falls back to the font
    /// passed at construction when no override is set.
    fn active_font(&self) -> Arc<Font> {
        crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
    }

    // ── Builder / setter methods ─────────────────────────────────────────────

    pub fn with_font_size(mut self, s: f64) -> Self {
        self.font_size = s;
        self
    }
    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p;
        self
    }
    pub fn with_read_only(mut self, v: bool) -> Self {
        self.read_only = v;
        self
    }
    pub fn with_select_all_on_focus(mut self, v: bool) -> Self {
        self.select_all_on_focus = v;
        self
    }
    pub fn with_password_mode(mut self, v: bool) -> Self {
        self.password_mode = v;
        self
    }

    pub fn with_placeholder(mut self, s: impl Into<String>) -> Self {
        self.placeholder = s.into();
        self
    }
    pub fn with_text(self, s: impl Into<String>) -> Self {
        let t = s.into();
        let len = t.len();
        let mut st = self.edit.borrow_mut();
        st.text = t;
        st.cursor = len;
        st.anchor = len;
        drop(st);
        self
    }

    pub fn on_change(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }
    pub fn on_enter(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_enter = Some(Box::new(cb));
        self
    }
    pub fn on_edit_complete(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_edit_complete = Some(Box::new(cb));
        self
    }

    // Layout-trait builders (with_margin / with_h_anchor / with_v_anchor /
    // with_min_size / with_max_size) live in `text_field/layout_builders.rs`
    // so this file stays under the project's 800-line cap.

    // ── Getters ──────────────────────────────────────────────────────────────

    pub fn text(&self) -> String {
        self.edit.borrow().text.clone()
    }
    pub fn cursor_pos(&self) -> usize {
        self.edit.borrow().cursor
    }
    pub fn selection(&self) -> String {
        let st = self.edit.borrow();
        let lo = st.cursor.min(st.anchor);
        let hi = st.cursor.max(st.anchor);
        st.text[lo..hi].to_string()
    }

    pub fn set_text(&mut self, s: impl Into<String>) {
        let t = s.into();
        let len = t.len();
        let mut st = self.edit.borrow_mut();
        st.text = t.clone();
        st.cursor = len;
        st.anchor = len;
        drop(st);
        if let Some(cell) = &self.text_cell {
            *cell.borrow_mut() = t;
        }
        self.undo.clear_history();
        self.pending_insert = None;
    }

    // ── Private state helpers ────────────────────────────────────────────────

    fn snap(&self) -> TextEditState {
        self.edit.borrow().clone()
    }
    #[allow(dead_code)]
    fn apply(&self, s: TextEditState) {
        *self.edit.borrow_mut() = s;
    }

    #[allow(dead_code)]
    fn sel_min(&self) -> usize {
        let s = self.edit.borrow();
        s.cursor.min(s.anchor)
    }
    #[allow(dead_code)]
    fn sel_max(&self) -> usize {
        let s = self.edit.borrow();
        s.cursor.max(s.anchor)
    }
    fn has_selection(&self) -> bool {
        let s = self.edit.borrow();
        s.cursor != s.anchor
    }

    /// Commit any pending coalesced insert command to the undo buffer.
    fn flush_pending(&mut self) {
        if let Some(cmd) = self.pending_insert.take() {
            self.undo.add(Box::new(cmd));
        }
    }

    /// Convert a pixel x position (in text-local space) to a byte offset in
    /// `real_text`.  In password mode, measures the masked string and maps back.
    fn click_to_cursor(&self, real_text: &str, tx: f64) -> usize {
        let font = self.active_font();
        if self.password_mode {
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

    /// Scroll `scroll_x` so that the cursor stays visible.
    fn ensure_cursor_visible(&mut self) {
        if self.bounds.width < 1.0 {
            return;
        }
        let inner_w = (self.bounds.width - self.padding * 2.0).max(0.0);
        let font = self.active_font();
        let cx = {
            let st = self.edit.borrow();
            if self.password_mode {
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

    // ── Edit operations ──────────────────────────────────────────────────────

    /// Insert `s` at cursor, replacing any selection.
    /// Consecutive single-char inserts are coalesced into one undo command.
    fn do_insert(&mut self, s: &str, is_single_char: bool) {
        // Strip disallowed chars; bail when nothing survives.
        let filtered = self.apply_char_filter(s);
        if filtered.is_empty() {
            return;
        }
        let s = filtered.as_str();
        let before = self.snap();
        let had_selection = before.cursor != before.anchor;

        // Apply the change
        {
            let mut st = self.edit.borrow_mut();
            if st.cursor != st.anchor {
                let lo = st.cursor.min(st.anchor);
                let hi = st.cursor.max(st.anchor);
                st.text.drain(lo..hi);
                st.cursor = lo;
                st.anchor = lo;
            }
            let cursor = st.cursor;
            st.text.insert_str(cursor, s);
            st.cursor = cursor + s.len();
            st.anchor = st.cursor;
        }

        let after = self.snap();

        if is_single_char && !had_selection {
            // Extend the pending coalesced command if one exists, otherwise start one.
            if let Some(ref mut pending) = self.pending_insert {
                pending.after = after;
            } else {
                self.pending_insert = Some(TextEditCommand {
                    name: "insert text",
                    before,
                    after,
                    target: Rc::clone(&self.edit),
                });
            }
        } else {
            // Non-char insert (paste) or insert-over-selection: commit pending and push new.
            self.flush_pending();
            self.undo.add(Box::new(TextEditCommand {
                name: "insert text",
                before,
                after,
                target: Rc::clone(&self.edit),
            }));
        }

        self.ensure_cursor_visible();
        self.notify_change();
    }

    /// Delete the selection (if any) or a single char/word, then push undo.
    fn do_delete(&mut self, forward: bool, word: bool) {
        self.flush_pending();
        let before = self.snap();
        {
            let mut st = self.edit.borrow_mut();
            if st.cursor != st.anchor {
                let lo = st.cursor.min(st.anchor);
                let hi = st.cursor.max(st.anchor);
                st.text.drain(lo..hi);
                st.cursor = lo;
                st.anchor = lo;
            } else if forward {
                let cursor = st.cursor;
                let end = if word {
                    next_word_boundary(&st.text, cursor)
                } else {
                    next_char_boundary(&st.text, cursor)
                };
                if end > cursor {
                    st.text.drain(cursor..end);
                }
                st.anchor = st.cursor;
            } else {
                let cursor = st.cursor;
                let start = if word {
                    prev_word_boundary(&st.text, cursor)
                } else {
                    prev_char_boundary(&st.text, cursor)
                };
                if start < cursor {
                    st.text.drain(start..cursor);
                    st.cursor = start;
                    st.anchor = start;
                }
            }
        }
        let after = self.snap();
        self.undo.add(Box::new(TextEditCommand {
            name: "delete text",
            before,
            after,
            target: Rc::clone(&self.edit),
        }));
        self.ensure_cursor_visible();
        self.notify_change();
    }

    fn do_undo(&mut self) {
        self.flush_pending();
        self.undo.undo();
        // Clamp positions in case the text changed length.
        let len = self.edit.borrow().text.len();
        let mut st = self.edit.borrow_mut();
        st.cursor = st.cursor.min(len);
        st.anchor = st.anchor.min(len);
        drop(st);
        self.ensure_cursor_visible();
        self.notify_change();
    }

    fn do_redo(&mut self) {
        self.flush_pending();
        self.undo.redo();
        let len = self.edit.borrow().text.len();
        let mut st = self.edit.borrow_mut();
        st.cursor = st.cursor.min(len);
        st.anchor = st.anchor.min(len);
        drop(st);
        self.ensure_cursor_visible();
        self.notify_change();
    }

    // Callback dispatchers — see `text_field/filter.rs`.

    // ── Keyboard handler ─────────────────────────────────────────────────────

    fn handle_key(&mut self, key: &Key, mods: Modifiers) -> EventResult {
        // Snapshot cursor/anchor before movement so we can keep anchor on Shift.
        let anchor_before = self.edit.borrow().anchor;

        // Command modifier (clipboard / select-all / undo): `Ctrl` on Windows
        // and Linux, `Cmd` (meta) on macOS.  Treating the two as equivalent
        // means the same handler serves both OSes without branching.
        let cmd = mods.ctrl || mods.meta;
        // Word-navigation modifier: `Ctrl` on Windows/Linux, `Option`
        // (alt) on macOS.  Used for Ctrl/Alt+Arrow, Ctrl/Alt+Backspace,
        // Ctrl/Alt+Delete.
        let word = mods.ctrl || mods.alt;

        match key {
            // ── Printable characters (and Ctrl/Cmd shortcuts on Char) ──────
            Key::Char(c) if !self.read_only || cmd => {
                if cmd {
                    return match c {
                        'a' | 'A' => {
                            let len = self.edit.borrow().text.len();
                            let mut st = self.edit.borrow_mut();
                            st.anchor = 0;
                            st.cursor = len;
                            EventResult::Consumed
                        }
                        'z' | 'Z' if !mods.shift => {
                            if !self.read_only {
                                self.do_undo();
                            }
                            EventResult::Consumed
                        }
                        'z' | 'Z' | 'y' | 'Y' => {
                            if !self.read_only {
                                self.do_redo();
                            }
                            EventResult::Consumed
                        }
                        'x' | 'X' => {
                            if !self.read_only && self.has_selection() {
                                clipboard_set(&self.selection());
                                self.do_delete(false, false); // delete selection via do_delete
                            }
                            EventResult::Consumed
                        }
                        'c' | 'C' => {
                            if self.has_selection() {
                                clipboard_set(&self.selection());
                            }
                            EventResult::Consumed
                        }
                        'v' | 'V' => {
                            if !self.read_only {
                                if let Some(clip) = clipboard_get() {
                                    self.do_insert(&clip, false);
                                }
                            }
                            EventResult::Consumed
                        }
                        _ => EventResult::Ignored,
                    };
                }
                if self.read_only {
                    return EventResult::Ignored;
                }
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                self.do_insert(s, true);
                EventResult::Consumed
            }

            // ── Insert clipboard shortcuts ────────────────────────────────
            // Classic Windows bindings (still common on Linux):
            //   Shift+Insert = Paste
            //   Ctrl+Insert  = Copy
            // Plain `Insert` toggles overwrite mode in many editors — we
            // don't model overwrite, so plain Insert is a no-op here.
            Key::Insert => {
                if mods.shift && !self.read_only {
                    if let Some(clip) = clipboard_get() {
                        self.do_insert(&clip, false);
                    }
                    return EventResult::Consumed;
                }
                if cmd {
                    if self.has_selection() {
                        clipboard_set(&self.selection());
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }

            // ── Backspace ─────────────────────────────────────────────────
            Key::Backspace if !self.read_only => {
                self.do_delete(false, word);
                EventResult::Consumed
            }

            // ── Delete ────────────────────────────────────────────────────
            Key::Delete if !self.read_only => {
                if mods.shift {
                    // Shift+Delete = Cut
                    if self.has_selection() {
                        clipboard_set(&self.selection());
                        self.do_delete(false, false);
                    }
                } else {
                    self.do_delete(true, word);
                }
                EventResult::Consumed
            }

            // ── Arrow Left ────────────────────────────────────────────────
            // Mac: `Cmd+Left` = start of line (Home behaviour).
            // Win/Mac: `Ctrl+Left` / `Option+Left` = previous word.
            // Plain: one character back (or collapse selection to left).
            Key::ArrowLeft => {
                self.flush_pending();
                let (cur, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = if mods.meta {
                    0 // Mac: Cmd+Left = line start
                } else if !mods.shift && cur != anchor {
                    cur.min(anchor) // collapse to left
                } else if word {
                    prev_word_boundary(&self.edit.borrow().text, cur)
                } else {
                    prev_char_boundary(&self.edit.borrow().text, cur)
                };
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur;
                st.anchor = new_anchor;
                drop(st);
                if new_cur == 0 {
                    self.scroll_x = 0.0;
                }
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Arrow Right ───────────────────────────────────────────────
            // Symmetric with ArrowLeft.  Mac: `Cmd+Right` = end of line.
            Key::ArrowRight => {
                self.flush_pending();
                let text_len = self.edit.borrow().text.len();
                let (cur, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = if mods.meta {
                    text_len // Mac: Cmd+Right = line end
                } else if !mods.shift && cur != anchor {
                    cur.max(anchor) // collapse to right
                } else if word {
                    next_word_boundary(&self.edit.borrow().text, cur)
                } else if cur < text_len {
                    next_char_boundary(&self.edit.borrow().text, cur)
                } else {
                    cur
                };
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur;
                st.anchor = new_anchor;
                drop(st);
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Arrow Up / Down ──────────────────────────────────────────
            // Single-line field, so vertical arrows only matter for the Mac
            // `Cmd+Up` / `Cmd+Down` (start / end of document) convention —
            // treat as Home / End.  Plain arrows fall through so callers
            // can spin numeric-input-style steppers, etc.
            Key::ArrowUp if mods.meta => {
                self.flush_pending();
                let (_, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = 0;
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur;
                st.anchor = new_anchor;
                drop(st);
                self.scroll_x = 0.0;
                EventResult::Consumed
            }
            Key::ArrowDown if mods.meta => {
                self.flush_pending();
                let len = self.edit.borrow().text.len();
                let (_, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = len;
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur;
                st.anchor = new_anchor;
                drop(st);
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Home ──────────────────────────────────────────────────────
            // Ctrl+Home is "start of document" on Windows — for a single-
            // line field that's the same as plain Home; accept both.
            Key::Home => {
                self.flush_pending();
                let mut st = self.edit.borrow_mut();
                st.cursor = 0;
                if !mods.shift {
                    st.anchor = 0;
                }
                drop(st);
                self.scroll_x = 0.0;
                EventResult::Consumed
            }

            // ── End ───────────────────────────────────────────────────────
            // Ctrl+End analogous to Ctrl+Home — treated as plain End here.
            Key::End => {
                self.flush_pending();
                let len = self.edit.borrow().text.len();
                let mut st = self.edit.borrow_mut();
                st.cursor = len;
                if !mods.shift {
                    st.anchor = len;
                }
                drop(st);
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Enter ─────────────────────────────────────────────────────
            // Commit as edit-complete too so numeric/parsed fields apply the
            // value on Enter (not only on blur).  Snapshot text to prevent a
            // second edit-complete firing on later focus loss.
            Key::Enter => {
                self.flush_pending();
                self.notify_enter();
                if self.text() != self.text_on_focus {
                    self.notify_edit_complete();
                    self.text_on_focus = self.text();
                }
                EventResult::Consumed
            }

            // Escape: clear selection if any, else let the parent
            // (typically a modal dialog) handle it.
            Key::Escape => {
                self.flush_pending();
                let (cur, anc) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                if cur != anc {
                    self.edit.borrow_mut().anchor = cur;
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }

            _ => {
                let _ = anchor_before;
                EventResult::Ignored
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Widget impl
// ---------------------------------------------------------------------------
