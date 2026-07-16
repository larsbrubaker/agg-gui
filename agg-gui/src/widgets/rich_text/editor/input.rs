//! Keyboard and mouse handling for [`RichTextEdit`], split out of `editor.rs`
//! to keep it under the 800-line cap.
//!
//! Mirrors `TextArea`'s semantics — click positions the caret, drag selects,
//! Shift extends, the usual navigation and clipboard chords — but drives the
//! shared [`RichEditCore`](super::core::RichEditCore) and the rich document's
//! structural edits instead of a flat string.  Every handled event is
//! `Consumed` so the auto-invalidation convention schedules the redraw.

use web_time::Instant;

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::widgets::multi_click::SelectGranularity;
use crate::widgets::text_field_core::{next_char_boundary, prev_char_boundary};

use super::super::model::DocPos;
use super::scroll::ScrollMove;
use super::RichTextEdit;

impl RichTextEdit {
    /// Central event dispatch (called from `Widget::on_event`).
    pub(super) fn handle_event(&mut self, event: &Event) -> EventResult {
        // While the right-click menu is open it captures events (see
        // `has_active_modal`); route them through it first.
        if let Some(result) = self.route_context_menu(event) {
            return result;
        }
        match event {
            Event::MouseMove { pos } => {
                let bar_hover_changed = match self.scrollbar_on_mouse_move(*pos) {
                    ScrollMove::Dragging(moved) => {
                        if moved {
                            crate::animation::request_draw();
                        }
                        return EventResult::Consumed;
                    }
                    ScrollMove::Hover(changed) => changed,
                };
                let was = self.hovered;
                self.hovered = self.hit_test_local(*pos);
                if self.hovered {
                    set_cursor_icon(CursorIcon::Text);
                }
                if self.selecting_drag {
                    let target = self.hit_test_pos(*pos);
                    self.extend_selection_drag(target);
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                if was != self.hovered || bar_hover_changed {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                pos,
                modifiers,
            } => {
                if self.scrollbar_begin_drag(*pos) {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                let target = self.hit_test_pos(*pos);
                let clicks = self.multi_click.register(*pos);
                self.begin_pointer_selection(target, clicks, modifiers.shift);
                self.selecting_drag = true;
                self.focus_time = Some(Instant::now());
                crate::animation::request_draw();
                EventResult::Consumed
            }
            Event::MouseDown {
                button: MouseButton::Right,
                pos,
                ..
            } => {
                if self.context_menu_enabled {
                    self.open_context_menu(*pos);
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                button: MouseButton::Left,
                ..
            } => {
                self.scrollbar_end_drag();
                self.selecting_drag = false;
                EventResult::Consumed
            }
            Event::MouseWheel { delta_y, .. } => {
                if self.scroll_by_wheel(*delta_y) {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusGained => {
                self.focused = true;
                self.focus_time = Some(Instant::now());
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::FocusLost => {
                self.focused = false;
                self.selecting_drag = false;
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::KeyDown { key, modifiers } => self.handle_key(key, modifiers),
            _ => EventResult::Ignored,
        }
    }

    fn hit_test_local(&self, p: crate::geometry::Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.bounds.height
    }

    fn handle_key(&mut self, key: &Key, modifiers: &crate::event::Modifiers) -> EventResult {
        let shift = modifiers.shift;
        let cmd = modifiers.ctrl || modifiers.meta;
        let caret = self.core.borrow().caret();
        match key {
            Key::ArrowLeft => {
                let target = if cmd {
                    self.word_target(caret, -1)
                } else {
                    self.char_target(caret, -1)
                };
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::ArrowRight => {
                let target = if cmd {
                    self.word_target(caret, 1)
                } else {
                    self.char_target(caret, 1)
                };
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::ArrowUp => {
                let target = self.pos_by_visual_line(caret, -1);
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::ArrowDown => {
                let target = self.pos_by_visual_line(caret, 1);
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::Home => {
                let target = if cmd {
                    DocPos::new(0, 0)
                } else {
                    self.caret_line_bounds(caret).0
                };
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::End => {
                let target = if cmd {
                    self.core.borrow().doc().end_pos()
                } else {
                    self.caret_line_bounds(caret).1
                };
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::PageUp => {
                let n = self.page_lines(caret) as isize;
                let target = self.pos_by_visual_line(caret, -n);
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::PageDown => {
                let n = self.page_lines(caret) as isize;
                let target = self.pos_by_visual_line(caret, n);
                self.core.borrow_mut().set_caret(target, shift);
            }
            Key::Backspace => self.core.borrow_mut().backspace(),
            Key::Delete => self.core.borrow_mut().delete_forward(),
            Key::Enter => self.core.borrow_mut().split(),
            Key::Char('a') | Key::Char('A') if cmd => self.core.borrow_mut().select_all(),
            Key::Char('c') | Key::Char('C') if cmd => {
                let text = self.core.borrow().selected_plain_text();
                if !text.is_empty() {
                    crate::clipboard::set_text(&text);
                }
            }
            Key::Char('x') | Key::Char('X') if cmd => {
                let text = self.core.borrow().selected_plain_text();
                if !text.is_empty() {
                    crate::clipboard::set_text(&text);
                    self.core.borrow_mut().backspace();
                }
            }
            Key::Char('v') | Key::Char('V') if cmd => {
                if let Some(text) = crate::clipboard::get_text() {
                    self.core.borrow_mut().insert(&text);
                }
            }
            Key::Char('z') | Key::Char('Z') if cmd && shift => {
                self.core.borrow_mut().redo();
            }
            Key::Char('z') | Key::Char('Z') if cmd => {
                self.core.borrow_mut().undo();
            }
            Key::Char('y') | Key::Char('Y') if cmd => {
                self.core.borrow_mut().redo();
            }
            Key::Char(c) if !cmd => {
                let mut buf = [0u8; 4];
                self.core.borrow_mut().insert(c.encode_utf8(&mut buf));
            }
            _ => return EventResult::Ignored,
        }
        let caret = self.core.borrow().caret();
        self.ensure_pos_visible(caret);
        self.focus_time = Some(Instant::now());
        crate::animation::request_draw();
        EventResult::Consumed
    }

    /// One char left (`-1`) or right (`+1`) from `caret`, crossing block
    /// boundaries at the paragraph edges.
    fn char_target(&self, caret: DocPos, dir: i32) -> DocPos {
        let core = self.core.borrow();
        let doc = core.doc();
        let text = doc
            .blocks
            .get(caret.block)
            .map(|b| b.text())
            .unwrap_or_default();
        if dir < 0 {
            if caret.byte > 0 {
                DocPos::new(caret.block, prev_char_boundary(&text, caret.byte))
            } else if caret.block > 0 {
                let prev_len = doc.blocks[caret.block - 1].text_len();
                DocPos::new(caret.block - 1, prev_len)
            } else {
                caret
            }
        } else if caret.byte < text.len() {
            DocPos::new(caret.block, next_char_boundary(&text, caret.byte))
        } else if caret.block + 1 < doc.blocks.len() {
            DocPos::new(caret.block + 1, 0)
        } else {
            caret
        }
    }

    /// One word left/right from `caret` within the flattened block text,
    /// crossing paragraph boundaries when already at an edge.
    fn word_target(&self, caret: DocPos, dir: i32) -> DocPos {
        let core = self.core.borrow();
        let doc = core.doc();
        let Some(block) = doc.blocks.get(caret.block) else {
            return caret;
        };
        let text = block.text();
        let bytes = text.as_bytes();
        let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
        if dir < 0 {
            if caret.byte == 0 {
                return if caret.block > 0 {
                    DocPos::new(caret.block - 1, doc.blocks[caret.block - 1].text_len())
                } else {
                    caret
                };
            }
            let mut i = caret.byte;
            while i > 0 && !is_word(bytes[i - 1]) {
                i -= 1;
            }
            while i > 0 && is_word(bytes[i - 1]) {
                i -= 1;
            }
            DocPos::new(caret.block, i)
        } else {
            let len = text.len();
            if caret.byte >= len {
                return if caret.block + 1 < doc.blocks.len() {
                    DocPos::new(caret.block + 1, 0)
                } else {
                    caret
                };
            }
            let mut i = caret.byte;
            while i < len && is_word(bytes[i]) {
                i += 1;
            }
            while i < len && !is_word(bytes[i]) {
                i += 1;
            }
            DocPos::new(caret.block, i)
        }
    }

    /// Caret/selection update for a fresh pointer press. `clicks` is the
    /// multi-click count (1 = single, 2 = double, 3 = triple). Double selects
    /// the word under `target`, triple selects the whole block; `shift` extends
    /// the existing selection instead.
    pub(super) fn begin_pointer_selection(&mut self, target: DocPos, clicks: u32, shift: bool) {
        if shift {
            self.select_granularity = SelectGranularity::Char;
            self.select_pivot = (target, target);
            self.core.borrow_mut().set_caret(target, true);
            return;
        }
        match clicks {
            n if n >= 3 => {
                self.select_granularity = SelectGranularity::Line;
                let (a, b) = self.block_range_at_pos(target);
                self.select_pivot = (a, b);
                self.core.borrow_mut().set_selection(a, b);
            }
            2 => {
                self.select_granularity = SelectGranularity::Word;
                let (a, b) = self.word_range_at_pos(target);
                self.select_pivot = (a, b);
                self.core.borrow_mut().set_selection(a, b);
            }
            _ => {
                self.select_granularity = SelectGranularity::Char;
                self.select_pivot = (target, target);
                self.core.borrow_mut().set_caret(target, false);
            }
        }
    }

    /// Extend the selection during a drag, honouring the granularity the
    /// initiating click established. `target` is the caret position under the
    /// pointer.
    pub(super) fn extend_selection_drag(&mut self, target: DocPos) {
        match self.select_granularity {
            SelectGranularity::Char => self.core.borrow_mut().set_caret(target, true),
            SelectGranularity::Word => {
                let (pivot_start, pivot_end) = self.select_pivot;
                let (cs, ce) = self.word_range_at_pos(target);
                if target >= pivot_end {
                    self.core.borrow_mut().set_selection(pivot_start, ce);
                } else {
                    self.core.borrow_mut().set_selection(pivot_end, cs);
                }
            }
            SelectGranularity::Line => {
                let (pivot_start, pivot_end) = self.select_pivot;
                let (cs, ce) = self.block_range_at_pos(target);
                if target >= pivot_end {
                    self.core.borrow_mut().set_selection(pivot_start, ce);
                } else {
                    self.core.borrow_mut().set_selection(pivot_end, cs);
                }
            }
        }
    }

    /// `[start, end)` document range of the word under `pos`, within its block.
    /// Uses the same word classification as [`word_target`](Self::word_target)
    /// (ASCII alphanumerics, `_`, and any non-ASCII byte are "word" bytes) so
    /// double-click selection and Ctrl+arrow navigation agree.
    fn word_range_at_pos(&self, pos: DocPos) -> (DocPos, DocPos) {
        let core = self.core.borrow();
        let doc = core.doc();
        let Some(block) = doc.blocks.get(pos.block) else {
            return (pos, pos);
        };
        let text = block.text();
        let bytes = text.as_bytes();
        let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
        let clamp = pos.byte.min(text.len());
        // Class of the char at the click; past the block end there is no char,
        // so treat it as a non-word boundary (select the trailing run, if any).
        let anchor_class = clamp < text.len() && is_word(bytes[clamp]);
        let mut start = clamp;
        while start > 0 && is_word(bytes[start - 1]) == anchor_class {
            start -= 1;
        }
        let mut end = clamp;
        while end < text.len() && is_word(bytes[end]) == anchor_class {
            end += 1;
        }
        (DocPos::new(pos.block, start), DocPos::new(pos.block, end))
    }

    /// The whole block (triple-click line selection) containing `pos`, from its
    /// start to its end byte.
    fn block_range_at_pos(&self, pos: DocPos) -> (DocPos, DocPos) {
        let core = self.core.borrow();
        let len = core
            .doc()
            .blocks
            .get(pos.block)
            .map(|b| b.text_len())
            .unwrap_or(0);
        (DocPos::new(pos.block, 0), DocPos::new(pos.block, len))
    }
}
