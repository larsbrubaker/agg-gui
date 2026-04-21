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

use std::rc::Rc;
use std::cell::RefCell;
use std::sync::Arc;

// web-time provides a WASM-compatible Instant (uses performance.now() in the
// browser; falls back to Instant on native).
use web_time::Instant;

use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{Font, measure_advance};
use crate::undo::UndoBuffer;
use crate::widget::{BackbufferCache, BackbufferMode, Widget};
use super::text_field_core::{
    TextEditCommand, TextEditState,
    byte_at_x, next_char_boundary, next_word_boundary,
    prev_char_boundary, prev_word_boundary, word_range_at,
};

// ---------------------------------------------------------------------------
// Clipboard stubs
// ---------------------------------------------------------------------------

#[cfg(feature = "clipboard")]
fn clipboard_get() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}
/// Native non-clipboard build: silently no-ops (clipboard disabled at compile time).
#[cfg(all(not(feature = "clipboard"), not(target_arch = "wasm32")))]
fn clipboard_get() -> Option<String> { None }
/// WASM build: read from the in-process buffer bridged by the JS harness.
#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn clipboard_get() -> Option<String> { crate::wasm_clipboard::get() }

#[cfg(feature = "clipboard")]
fn clipboard_set(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(text.to_string()); }
}
/// Native non-clipboard build: silently no-ops.
#[cfg(all(not(feature = "clipboard"), not(target_arch = "wasm32")))]
fn clipboard_set(_: &str) {}
/// WASM build: write to the in-process buffer so the JS `copy`/`cut` handler
/// can forward it to the browser's system clipboard.
#[cfg(all(not(feature = "clipboard"), target_arch = "wasm32"))]
fn clipboard_set(text: &str) { crate::wasm_clipboard::set(text); }

// ---------------------------------------------------------------------------
// TextField
// ---------------------------------------------------------------------------

/// Single-line editable text field.
pub struct TextField {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    base:     WidgetBase,

    // All mutable editing state lives here — shared with undo commands.
    edit: Rc<RefCell<TextEditState>>,

    // Undo/redo history.
    undo: UndoBuffer,

    // Pending coalesced insert command (committed when action type changes).
    pending_insert: Option<TextEditCommand>,

    // Snapshot of text when focus was gained — used to decide if on_edit_complete fires.
    text_on_focus: String,

    // Font
    font:      Arc<Font>,
    font_size: f64,

    // Editing options
    pub read_only:           bool,
    pub select_all_on_focus: bool,
    /// When `true`, every character is displayed as '•' (U+2022).
    /// The actual text is stored and edited normally; only the render is masked.
    pub password_mode:       bool,

    // Interaction state
    focused:    bool,
    hovered:    bool,
    mouse_down: bool,
    scroll_x:   f64,

    // Cursor blink: set to Some(Instant::now()) on FocusGained.
    focus_time: Option<Instant>,
    // Blink phase (floor(elapsed_ms / 500)) last drawn by `paint_overlay`.
    // `needs_paint` compares the current phase against this and reports
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
    on_change:        Option<Box<dyn FnMut(&str)>>,
    on_enter:         Option<Box<dyn FnMut(&str)>>,
    on_edit_complete: Option<Box<dyn FnMut(&str)>>,

    // ── Backbuffer cache ─────────────────────────────────────────────
    //
    // Cache holds bg + text + selection + border.  Cursor draws in
    // `paint_overlay` directly on the outer ctx AFTER the cache blit
    // so cursor-blink state flips (twice per second) don't invalidate
    // the cache.  Sig deliberately excludes `blink_visible`.
    cache:    BackbufferCache,
    last_sig: Option<TextFieldSig>,
}

#[derive(Clone, PartialEq)]
struct TextFieldSig {
    text:          String,
    cursor:        usize,
    anchor:        usize,
    focused:       bool,
    hovered:       bool,
    scroll_x_bits: u64,
    w_bits:        u64,
    h_bits:        u64,
    // Font identity + size: the cached bitmap was rasterised with a specific
    // typeface at a specific point size, so any live swap in the System
    // window (which runs through `font_settings::set_system_font` /
    // `set_font_size_scale`) must invalidate — otherwise the stale bitmap
    // keeps blitting until some other field in the sig happens to change
    // (e.g. the user hovers the control, which flips `hovered`).
    font_ptr:      usize,
    font_size_bits: u64,
}

impl TextField {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds:   Rect::default(),
            children: Vec::new(),
            base:     WidgetBase::new(),
            edit:     Rc::new(RefCell::new(TextEditState::default())),
            undo:     UndoBuffer::new(),
            pending_insert: None,
            text_on_focus:  String::new(),
            font,
            font_size:           14.0,
            read_only:           false,
            select_all_on_focus: false,
            password_mode:       false,
            focused:    false,
            hovered:    false,
            mouse_down: false,
            scroll_x:   0.0,
            focus_time:       None,
            blink_last_phase: std::cell::Cell::new(u64::MAX),
            last_click_time:  None,
            placeholder: String::new(),
            padding: 8.0,
            on_change:        None,
            on_enter:         None,
            on_edit_complete: None,
            cache:            BackbufferCache::default(),
            last_sig:         None,
        }
    }

    /// Currently-active font — honours the thread-local system-font override
    /// (`font_settings::current_system_font`) so changes in the System window
    /// propagate live without a widget-tree rebuild.  Falls back to the font
    /// passed at construction when no override is set.
    fn active_font(&self) -> Arc<Font> {
        crate::font_settings::current_system_font()
            .unwrap_or_else(|| Arc::clone(&self.font))
    }

    // ── Builder / setter methods ─────────────────────────────────────────────

    pub fn with_font_size(mut self, s: f64) -> Self { self.font_size = s; self }
    pub fn with_padding(mut self, p: f64)   -> Self { self.padding   = p; self }
    pub fn with_read_only(mut self, v: bool) -> Self { self.read_only = v; self }
    pub fn with_select_all_on_focus(mut self, v: bool) -> Self { self.select_all_on_focus = v; self }
    pub fn with_password_mode(mut self, v: bool) -> Self { self.password_mode = v; self }

    pub fn with_placeholder(mut self, s: impl Into<String>) -> Self { self.placeholder = s.into(); self }
    pub fn with_text(self, s: impl Into<String>) -> Self {
        let t = s.into();
        let len = t.len();
        let mut st = self.edit.borrow_mut();
        st.text   = t;
        st.cursor = len;
        st.anchor = len;
        drop(st);
        self
    }

    pub fn on_change(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_change = Some(Box::new(cb)); self
    }
    pub fn on_enter(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_enter = Some(Box::new(cb)); self
    }
    pub fn on_edit_complete(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_edit_complete = Some(Box::new(cb)); self
    }

    pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
    pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
    pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }

    // ── Getters ──────────────────────────────────────────────────────────────

    pub fn text(&self) -> String { self.edit.borrow().text.clone() }
    pub fn cursor_pos(&self) -> usize { self.edit.borrow().cursor }
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
        st.text   = t;
        st.cursor = len;
        st.anchor = len;
        drop(st);
        self.undo.clear_history();
        self.pending_insert = None;
    }

    // ── Private state helpers ────────────────────────────────────────────────

    fn snap(&self) -> TextEditState { self.edit.borrow().clone() }
    #[allow(dead_code)]
    fn apply(&self, s: TextEditState) { *self.edit.borrow_mut() = s; }

    #[allow(dead_code)]
    fn sel_min(&self) -> usize { let s = self.edit.borrow(); s.cursor.min(s.anchor) }
    #[allow(dead_code)]
    fn sel_max(&self) -> usize { let s = self.edit.borrow(); s.cursor.max(s.anchor) }
    fn has_selection(&self) -> bool { let s = self.edit.borrow(); s.cursor != s.anchor }

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
            let n      = real_text.chars().count();
            let masked = BULLET.to_string().repeat(n);
            let disp   = byte_at_x(&font, &masked, self.font_size, tx);
            // Map masked byte offset → char index → real byte offset.
            let char_idx = disp / BULLET_LEN;
            real_text.char_indices()
                .nth(char_idx)
                .map(|(i, _)| i)
                .unwrap_or(real_text.len())
        } else {
            byte_at_x(&font, real_text, self.font_size, tx)
        }
    }

    /// Scroll `scroll_x` so that the cursor stays visible.
    fn ensure_cursor_visible(&mut self) {
        if self.bounds.width < 1.0 { return; }
        let inner_w = (self.bounds.width - self.padding * 2.0).max(0.0);
        let font = self.active_font();
        let cx = {
            let st = self.edit.borrow();
            if self.password_mode {
                const BULLET: char = '•';
                #[allow(dead_code)]
                const BULLET_LEN: usize = 3;
                let n      = st.text[..st.cursor].chars().count();
                let masked = BULLET.to_string().repeat(n);
                measure_advance(&font, &masked, self.font_size)
            } else {
                measure_advance(&font, &st.text[..st.cursor], self.font_size)
            }
        };
        if cx < self.scroll_x { self.scroll_x = cx; }
        else if cx > self.scroll_x + inner_w { self.scroll_x = cx - inner_w; }
    }

    // ── Edit operations ──────────────────────────────────────────────────────

    /// Insert `s` at cursor, replacing any selection.
    /// Consecutive single-char inserts are coalesced into one undo command.
    fn do_insert(&mut self, s: &str, is_single_char: bool) {
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
                let end = if word { next_word_boundary(&st.text, cursor) }
                          else    { next_char_boundary(&st.text, cursor) };
                if end > cursor { st.text.drain(cursor..end); }
                st.anchor = st.cursor;
            } else {
                let cursor = st.cursor;
                let start = if word { prev_word_boundary(&st.text, cursor) }
                            else    { prev_char_boundary(&st.text, cursor) };
                if start < cursor {
                    st.text.drain(start..cursor);
                    st.cursor = start;
                    st.anchor = start;
                }
            }
        }
        let after = self.snap();
        self.undo.add(Box::new(TextEditCommand {
            name: "delete text", before, after, target: Rc::clone(&self.edit),
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

    // ── Callback dispatchers ─────────────────────────────────────────────────

    fn notify_change(&mut self) {
        if let Some(mut cb) = self.on_change.take() {
            let t = self.text(); cb(&t); self.on_change = Some(cb);
        }
    }
    fn notify_enter(&mut self) {
        if let Some(mut cb) = self.on_enter.take() {
            let t = self.text(); cb(&t); self.on_enter = Some(cb);
        }
    }
    fn notify_edit_complete(&mut self) {
        if let Some(mut cb) = self.on_edit_complete.take() {
            let t = self.text(); cb(&t); self.on_edit_complete = Some(cb);
        }
    }

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
                            st.anchor = 0; st.cursor = len;
                            EventResult::Consumed
                        }
                        'z' | 'Z' if !mods.shift => { self.do_undo(); EventResult::Consumed }
                        'z' | 'Z' | 'y' | 'Y'   => { self.do_redo(); EventResult::Consumed }
                        'x' | 'X' => {
                            if self.has_selection() {
                                clipboard_set(&self.selection());
                                self.do_delete(false, false); // delete selection via do_delete
                            }
                            EventResult::Consumed
                        }
                        'c' | 'C' => {
                            if self.has_selection() { clipboard_set(&self.selection()); }
                            EventResult::Consumed
                        }
                        'v' | 'V' => {
                            if let Some(clip) = clipboard_get() { self.do_insert(&clip, false); }
                            EventResult::Consumed
                        }
                        _ => EventResult::Ignored,
                    };
                }
                if self.read_only { return EventResult::Ignored; }
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
                    if let Some(clip) = clipboard_get() { self.do_insert(&clip, false); }
                    return EventResult::Consumed;
                }
                if cmd {
                    if self.has_selection() { clipboard_set(&self.selection()); }
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
                    if self.has_selection() { clipboard_set(&self.selection()); self.do_delete(false, false); }
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
                    0                                        // Mac: Cmd+Left = line start
                } else if !mods.shift && cur != anchor {
                    cur.min(anchor)                          // collapse to left
                } else if word {
                    prev_word_boundary(&self.edit.borrow().text, cur)
                } else {
                    prev_char_boundary(&self.edit.borrow().text, cur)
                };
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur; st.anchor = new_anchor;
                drop(st);
                if new_cur == 0 { self.scroll_x = 0.0; }
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
                    text_len                                 // Mac: Cmd+Right = line end
                } else if !mods.shift && cur != anchor {
                    cur.max(anchor)                          // collapse to right
                } else if word {
                    next_word_boundary(&self.edit.borrow().text, cur)
                } else if cur < text_len {
                    next_char_boundary(&self.edit.borrow().text, cur)
                } else {
                    cur
                };
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur; st.anchor = new_anchor;
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
                let (_, anchor) = { let st = self.edit.borrow(); (st.cursor, st.anchor) };
                let new_cur = 0;
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur; st.anchor = new_anchor;
                drop(st);
                self.scroll_x = 0.0;
                EventResult::Consumed
            }
            Key::ArrowDown if mods.meta => {
                self.flush_pending();
                let len = self.edit.borrow().text.len();
                let (_, anchor) = { let st = self.edit.borrow(); (st.cursor, st.anchor) };
                let new_cur = len;
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur; st.anchor = new_anchor;
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
                if !mods.shift { st.anchor = 0; }
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
                if !mods.shift { st.anchor = len; }
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

            // ── Escape: clear selection ───────────────────────────────────
            Key::Escape => {
                self.flush_pending();
                let cur = self.edit.borrow().cursor;
                self.edit.borrow_mut().anchor = cur;
                EventResult::Consumed
            }

            _ => { let _ = anchor_before; EventResult::Ignored }
        }
    }
}

// ---------------------------------------------------------------------------
// Widget impl
// ---------------------------------------------------------------------------

impl Widget for TextField {
    fn type_name(&self)  -> &'static str { "TextField" }
    fn bounds(&self)     -> Rect         { self.bounds }
    fn set_bounds(&mut self, b: Rect)    { self.bounds = b; }
    fn children(&self)   -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn is_focusable(&self) -> bool { true }

    /// While focused, the cursor blinks at 500 ms half-period.  The field
    /// itself drives its own repaint cadence: [`needs_paint`] reports dirty
    /// whenever wall-clock time has crossed a flip boundary since the last
    /// paint, and [`next_paint_deadline`] returns the exact wall-clock
    /// instant of the next boundary so the host can `WaitUntil` it.
    ///
    /// Losing focus makes both return `None` / `false`, and the tree walk's
    /// visibility check drops the field entirely when its enclosing window
    /// is closed / collapsed / tab not selected — so an invisible focused
    /// field does NOT keep the loop awake.
    fn needs_paint(&self) -> bool {
        if !self.focused { return false; }
        let Some(t) = self.focus_time else { return false; };
        let current_phase = (t.elapsed().as_millis() / 500) as u64;
        current_phase != self.blink_last_phase.get()
    }

    fn next_paint_deadline(&self) -> Option<web_time::Instant> {
        if !self.focused { return None; }
        let t = self.focus_time?;
        let ms = t.elapsed().as_millis() as u64;
        let next_phase = (ms / 500) + 1;
        Some(t + std::time::Duration::from_millis(next_phase * 500))
    }

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }

    fn backbuffer_mode(&self) -> BackbufferMode {
        if crate::font_settings::lcd_enabled() {
            BackbufferMode::LcdCoverage
        } else {
            BackbufferMode::Rgba
        }
    }

    fn layout(&mut self, available: Size) -> Size {
        // Sig excludes cursor-blink phase.  Cursor paints in
        // `paint_overlay` after cache blit — no blink-driven
        // invalidation.
        let st   = self.edit.borrow();
        let font = self.active_font();
        let sig = TextFieldSig {
            text:           st.text.clone(),
            cursor:         st.cursor,
            anchor:         st.anchor,
            focused:        self.focused,
            hovered:        self.hovered,
            scroll_x_bits:  self.scroll_x.to_bits(),
            w_bits:         self.bounds.width .to_bits(),
            h_bits:         self.bounds.height.to_bits(),
            font_ptr:       Arc::as_ptr(&font) as usize,
            font_size_bits: self.font_size.to_bits(),
        };
        drop(st);
        if self.last_sig.as_ref() != Some(&sig) {
            self.last_sig = Some(sig);
            self.cache.invalidate();
        }
        Size::new(available.width, (self.font_size * 2.4).max(28.0))
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w   = self.bounds.width;
        let h   = self.bounds.height;
        let r   = 6.0;
        let pad = self.padding;
        let (raw_text, raw_cursor, raw_anchor) = {
            let st = self.edit.borrow();
            (st.text.clone(), st.cursor, st.anchor)
        };
        // In password mode render '•' for every character, but keep byte positions
        // consistent by recomputing them against the masked string.
        let (text, cursor, anchor) = if self.password_mode {
            const BULLET: char = '•';
            const BULLET_LEN: usize = 3; // '•' is 3 bytes in UTF-8
            let n     = raw_text.chars().count();
            let masked = BULLET.to_string().repeat(n);
            let cur   = raw_text[..raw_cursor].chars().count() * BULLET_LEN;
            let anc   = raw_text[..raw_anchor].chars().count() * BULLET_LEN;
            (masked, cur, anc)
        } else {
            (raw_text, raw_cursor, raw_anchor)
        };

        let v = ctx.visuals();

        // ── Background ────────────────────────────────────────────────────
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        // ── Text area clip ────────────────────────────────────────────────
        ctx.clip_rect(pad, 0.0, (w - pad * 2.0).max(0.0), h);

        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(self.font_size);

        let m          = ctx.measure_text("Ag").unwrap_or_default();
        let baseline_y = h * 0.5 - (m.ascent - m.descent) * 0.5;
        let text_x     = pad - self.scroll_x;

        // ── Selection highlight ───────────────────────────────────────────
        if cursor != anchor {
            let lo = cursor.min(anchor);
            let hi = cursor.max(anchor);
            let lo_x = measure_advance(&font, &text[..lo], self.font_size);
            let hi_x = measure_advance(&font, &text[..hi], self.font_size);
            let sx   = (text_x + lo_x).max(pad);
            let sw   = (text_x + hi_x).min(w - pad) - sx;
            if sw > 0.0 {
                let hl_bot = baseline_y - m.descent;
                let hl_h   = (m.ascent + m.descent) * 1.2;
                ctx.set_fill_color(if self.focused {
                    v.selection_bg
                } else {
                    v.selection_bg_unfocused
                });
                ctx.begin_path();
                ctx.rect(sx, hl_bot - hl_h * 0.1, sw, hl_h);
                ctx.fill();
            }
        }

        // ── Text or placeholder ───────────────────────────────────────────
        if text.is_empty() && !self.focused {
            ctx.set_fill_color(v.text_dim);
            ctx.fill_text(&self.placeholder, text_x, baseline_y);
        } else {
            ctx.set_fill_color(v.text_color);
            ctx.fill_text(&text, text_x, baseline_y);
        }

        // Cursor draws in `paint_overlay` — skipped here so blink
        // state doesn't force the cache to re-raster twice per second.

        ctx.reset_clip();

        // ── Border ────────────────────────────────────────────────────────
        let border_color = if self.focused { v.accent }
            else if self.hovered { v.widget_stroke_active }
            else { v.widget_stroke };
        ctx.set_stroke_color(border_color);
        ctx.set_line_width(if self.focused { 2.0 } else { 1.0 });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.stroke();
    }

    /// Cursor overlay — runs AFTER the cache blit on every frame, so
    /// blink-phase flips don't invalidate the backbuffer.  Reads the
    /// same edit state `paint()` does so cursor lands on the glyph the
    /// cached text shows.
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Record the blink phase being drawn this frame.  The next tree
        // walk's `needs_paint` will compare against this and report dirty
        // once wall-clock time crosses the next 500 ms boundary — no
        // host-side deadline bookkeeping, the widget drives itself.
        if self.focused {
            if let Some(t) = self.focus_time {
                let phase = (t.elapsed().as_millis() / 500) as u64;
                self.blink_last_phase.set(phase);
            }
        }

        let cursor_visible = self.focused && {
            let st = self.edit.borrow();
            st.cursor == st.anchor
        } && match self.focus_time {
            Some(t) => (t.elapsed().as_millis() / 500) % 2 == 0,
            None    => false,
        };
        if !cursor_visible { return; }

        let (text, cursor) = {
            let st = self.edit.borrow();
            let text = if self.password_mode {
                const BULLET: char = '•';
                let n = st.text.chars().count();
                BULLET.to_string().repeat(n)
            } else {
                st.text.clone()
            };
            let cursor = if self.password_mode {
                const BULLET_LEN: usize = 3;
                st.text[..st.cursor].chars().count() * BULLET_LEN
            } else {
                st.cursor
            };
            (text, cursor)
        };

        let h   = self.bounds.height;
        let pad = self.padding;
        let v   = ctx.visuals();

        let font = self.active_font();
        ctx.set_font(Arc::clone(&font));
        ctx.set_font_size(self.font_size);
        let m = ctx.measure_text("Ag").unwrap_or_default();
        let baseline_y = h * 0.5 - (m.ascent - m.descent) * 0.5;
        let text_x     = pad - self.scroll_x;
        let cx  = text_x + measure_advance(&font, &text[..cursor], self.font_size);
        let top = baseline_y + m.ascent;
        let bot = baseline_y - m.descent;

        // Clip to the text area so the cursor can't spill past the
        // padding or the border.
        ctx.save();
        ctx.clip_rect(pad, 0.0, (self.bounds.width - pad * 2.0).max(0.0), h);
        ctx.set_stroke_color(v.accent);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(cx, bot);
        ctx.line_to(cx, top);
        ctx.stroke();
        ctx.restore();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if self.mouse_down && self.focused {
                    let tx = pos.x - self.padding + self.scroll_x;
                    let text = self.edit.borrow().text.clone();
                    let new_cur = self.click_to_cursor(&text, tx);
                    self.edit.borrow_mut().cursor = new_cur;
                    crate::animation::request_tick();
                }
                if was != self.hovered { crate::animation::request_tick(); }
                EventResult::Ignored
            }

            Event::MouseDown { pos, button: MouseButton::Left, modifiers: mods } => {
                self.mouse_down = true;
                let tx = pos.x - self.padding + self.scroll_x;
                let text = self.edit.borrow().text.clone();
                let new_cur = self.click_to_cursor(&text, tx);

                // Double-click: select word
                let is_double = self.last_click_time
                    .map(|t| t.elapsed().as_millis() < 350)
                    .unwrap_or(false);
                self.last_click_time = Some(Instant::now());

                if is_double && !mods.shift {
                    let (ws, we) = word_range_at(&text, new_cur);
                    self.edit.borrow_mut().anchor = ws;
                    self.edit.borrow_mut().cursor = we;
                } else if mods.shift {
                    self.edit.borrow_mut().cursor = new_cur;
                } else {
                    self.edit.borrow_mut().cursor = new_cur;
                    self.edit.borrow_mut().anchor = new_cur;
                }
                // Reset blink phase on click so cursor is immediately visible.
                self.focus_time = Some(Instant::now());
                crate::animation::request_tick();
                EventResult::Consumed
            }

            Event::MouseUp { button: MouseButton::Left, .. } => {
                self.mouse_down = false;
                EventResult::Ignored
            }

            Event::FocusGained => {
                self.focused        = true;
                self.focus_time     = Some(Instant::now());
                self.text_on_focus  = self.text();
                if self.select_all_on_focus {
                    let len = self.edit.borrow().text.len();
                    self.edit.borrow_mut().anchor = 0;
                    self.edit.borrow_mut().cursor = len;
                }
                crate::animation::request_tick();
                EventResult::Ignored
            }

            Event::FocusLost => {
                let was_focused = self.focused;
                self.focused    = false;
                self.focus_time = None;
                self.mouse_down = false;
                self.flush_pending();
                if self.text() != self.text_on_focus { self.notify_edit_complete(); }
                if was_focused { crate::animation::request_tick(); }
                EventResult::Ignored
            }

            Event::KeyDown { key, modifiers } if self.focused => {
                // Reset blink on any keypress so cursor is visible immediately.
                self.focus_time = Some(Instant::now());
                let result = self.handle_key(key, *modifiers);
                // Any text-editing keystroke that reached the focused field
                // visibly mutates the text / cursor / selection; repaint.
                if result == EventResult::Consumed {
                    crate::animation::request_tick();
                }
                result
            }

            _ => EventResult::Ignored,
        }
    }
}
