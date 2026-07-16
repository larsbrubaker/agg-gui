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
use crate::widgets::text_field_core::{next_char_boundary, prev_char_boundary};

use super::super::model::DocPos;
use super::scroll::ScrollMove;
use super::RichTextEdit;

impl RichTextEdit {
    /// Central event dispatch (called from `Widget::on_event`).
    pub(super) fn handle_event(&mut self, event: &Event) -> EventResult {
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
                    self.core.borrow_mut().set_caret(target, true);
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
                self.core.borrow_mut().set_caret(target, modifiers.shift);
                self.selecting_drag = true;
                self.focus_time = Some(Instant::now());
                crate::animation::request_draw();
                EventResult::Consumed
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
}
