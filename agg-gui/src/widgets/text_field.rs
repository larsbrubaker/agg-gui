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

use crate::color::Color;
use crate::event::{Event, EventResult, Key, Modifiers, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::{Font, measure_advance};
use crate::undo::UndoBuffer;
use crate::widget::Widget;
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
#[cfg(not(feature = "clipboard"))]
fn clipboard_get() -> Option<String> { None }

#[cfg(feature = "clipboard")]
fn clipboard_set(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(text.to_string()); }
}
#[cfg(not(feature = "clipboard"))]
fn clipboard_set(_: &str) {}

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

    // Interaction state
    focused:    bool,
    hovered:    bool,
    mouse_down: bool,
    scroll_x:   f64,

    // Cursor blink: set to Some(Instant::now()) on FocusGained.
    focus_time: Option<Instant>,

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
            focused:    false,
            hovered:    false,
            mouse_down: false,
            scroll_x:   0.0,
            focus_time:      None,
            last_click_time: None,
            placeholder: String::new(),
            padding: 8.0,
            on_change:        None,
            on_enter:         None,
            on_edit_complete: None,
        }
    }

    // ── Builder / setter methods ─────────────────────────────────────────────

    pub fn with_font_size(mut self, s: f64) -> Self { self.font_size = s; self }
    pub fn with_padding(mut self, p: f64)   -> Self { self.padding   = p; self }
    pub fn with_read_only(mut self, v: bool) -> Self { self.read_only = v; self }
    pub fn with_select_all_on_focus(mut self, v: bool) -> Self { self.select_all_on_focus = v; self }

    pub fn with_placeholder(mut self, s: impl Into<String>) -> Self { self.placeholder = s.into(); self }
    pub fn with_text(mut self, s: impl Into<String>) -> Self {
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
    fn apply(&self, s: TextEditState) { *self.edit.borrow_mut() = s; }

    fn sel_min(&self) -> usize { let s = self.edit.borrow(); s.cursor.min(s.anchor) }
    fn sel_max(&self) -> usize { let s = self.edit.borrow(); s.cursor.max(s.anchor) }
    fn has_selection(&self) -> bool { let s = self.edit.borrow(); s.cursor != s.anchor }

    /// Commit any pending coalesced insert command to the undo buffer.
    fn flush_pending(&mut self) {
        if let Some(cmd) = self.pending_insert.take() {
            self.undo.add(Box::new(cmd));
        }
    }

    /// Scroll `scroll_x` so that the cursor stays visible.
    fn ensure_cursor_visible(&mut self) {
        if self.bounds.width < 1.0 { return; }
        let inner_w = (self.bounds.width - self.padding * 2.0).max(0.0);
        let cx = {
            let st = self.edit.borrow();
            measure_advance(&self.font, &st.text[..st.cursor], self.font_size)
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

        match key {
            // ── Printable characters (and Ctrl shortcuts on Char) ──────────
            Key::Char(c) if !self.read_only || mods.ctrl => {
                if mods.ctrl {
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

            // ── Backspace ─────────────────────────────────────────────────
            Key::Backspace if !self.read_only => {
                self.do_delete(false, mods.ctrl);
                EventResult::Consumed
            }

            // ── Delete ────────────────────────────────────────────────────
            Key::Delete if !self.read_only => {
                if mods.shift {
                    // Shift+Delete = Cut
                    if self.has_selection() { clipboard_set(&self.selection()); self.do_delete(false, false); }
                } else {
                    self.do_delete(true, mods.ctrl);
                }
                EventResult::Consumed
            }

            // ── Arrow Left ────────────────────────────────────────────────
            Key::ArrowLeft => {
                self.flush_pending();
                let (cur, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = if !mods.shift && cur != anchor {
                    cur.min(anchor)  // collapse to left
                } else if mods.ctrl {
                    prev_word_boundary(&self.edit.borrow().text, cur)
                } else {
                    prev_char_boundary(&self.edit.borrow().text, cur)
                };
                let new_anchor = if mods.shift { anchor } else { new_cur };
                let mut st = self.edit.borrow_mut();
                st.cursor = new_cur; st.anchor = new_anchor;
                drop(st);
                self.ensure_cursor_visible();
                EventResult::Consumed
            }

            // ── Arrow Right ───────────────────────────────────────────────
            Key::ArrowRight => {
                self.flush_pending();
                let text_len = self.edit.borrow().text.len();
                let (cur, anchor) = {
                    let st = self.edit.borrow();
                    (st.cursor, st.anchor)
                };
                let new_cur = if !mods.shift && cur != anchor {
                    cur.max(anchor)  // collapse to right
                } else if mods.ctrl {
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

            // ── Home ──────────────────────────────────────────────────────
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
            Key::Enter => { self.flush_pending(); self.notify_enter(); EventResult::Consumed }

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

    fn margin(&self)   -> Insets  { self.base.margin }
    fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
    fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
    fn min_size(&self) -> Size    { self.base.min_size }
    fn max_size(&self) -> Size    { self.base.max_size }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, (self.font_size * 2.4).max(28.0))
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w   = self.bounds.width;
        let h   = self.bounds.height;
        let r   = 6.0;
        let pad = self.padding;
        let (text, cursor, anchor) = {
            let st = self.edit.borrow();
            (st.text.clone(), st.cursor, st.anchor)
        };

        // ── Background ────────────────────────────────────────────────────
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        // ── Text area clip ────────────────────────────────────────────────
        ctx.clip_rect(pad, 0.0, (w - pad * 2.0).max(0.0), h);

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);

        let m          = ctx.measure_text("Ag").unwrap_or_default();
        let baseline_y = h * 0.5 - (m.ascent - m.descent) * 0.5 + m.descent;
        let text_x     = pad - self.scroll_x;

        // ── Selection highlight ───────────────────────────────────────────
        if cursor != anchor {
            let lo = cursor.min(anchor);
            let hi = cursor.max(anchor);
            let lo_x = measure_advance(&self.font, &text[..lo], self.font_size);
            let hi_x = measure_advance(&self.font, &text[..hi], self.font_size);
            let sx   = (text_x + lo_x).max(pad);
            let sw   = (text_x + hi_x).min(w - pad) - sx;
            if sw > 0.0 {
                let hl_bot = baseline_y - m.descent;
                let hl_h   = (m.ascent + m.descent) * 1.2;
                ctx.set_fill_color(Color::rgba(0.22, 0.45, 0.88, 0.25));
                ctx.begin_path();
                ctx.rect(sx, hl_bot - hl_h * 0.1, sw, hl_h);
                ctx.fill();
            }
        }

        // ── Text or placeholder ───────────────────────────────────────────
        if text.is_empty() && !self.focused {
            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
            ctx.fill_text(&self.placeholder, text_x, baseline_y);
        } else {
            ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.9));
            ctx.fill_text(&text, text_x, baseline_y);
        }

        // ── Blinking cursor (500 ms half-period) ──────────────────────────
        let cursor_visible = self.focused && cursor == anchor && {
            match self.focus_time {
                Some(t) => (t.elapsed().as_millis() / 500) % 2 == 0,
                None    => false,
            }
        };
        if cursor_visible {
            let cx  = text_x + measure_advance(&self.font, &text[..cursor], self.font_size);
            let top = baseline_y + m.ascent;
            let bot = baseline_y - m.descent;
            ctx.set_stroke_color(Color::rgb(0.22, 0.45, 0.88));
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.move_to(cx, bot);
            ctx.line_to(cx, top);
            ctx.stroke();
        }

        ctx.reset_clip();

        // ── Border ────────────────────────────────────────────────────────
        let border_color = if self.focused { Color::rgb(0.22, 0.45, 0.88) }
            else if self.hovered { Color::rgb(0.70, 0.70, 0.75) }
            else { Color::rgb(0.82, 0.82, 0.86) };
        ctx.set_stroke_color(border_color);
        ctx.set_line_width(if self.focused { 2.0 } else { 1.0 });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.stroke();
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                if self.mouse_down && self.focused {
                    let tx = pos.x - self.padding + self.scroll_x;
                    let text = self.edit.borrow().text.clone();
                    let new_cur = byte_at_x(&self.font, &text, self.font_size, tx);
                    self.edit.borrow_mut().cursor = new_cur;
                }
                EventResult::Ignored
            }

            Event::MouseDown { pos, button: MouseButton::Left, modifiers: mods } => {
                self.mouse_down = true;
                let tx = pos.x - self.padding + self.scroll_x;
                let text = self.edit.borrow().text.clone();
                let new_cur = byte_at_x(&self.font, &text, self.font_size, tx);

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
                EventResult::Ignored
            }

            Event::FocusLost => {
                self.focused    = false;
                self.focus_time = None;
                self.mouse_down = false;
                self.flush_pending();
                if self.text() != self.text_on_focus { self.notify_edit_complete(); }
                EventResult::Ignored
            }

            Event::KeyDown { key, modifiers } if self.focused => {
                // Reset blink on any keypress so cursor is visible immediately.
                self.focus_time = Some(Instant::now());
                self.handle_key(key, *modifiers)
            }

            _ => EventResult::Ignored,
        }
    }
}
