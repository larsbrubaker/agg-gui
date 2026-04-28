//! Event handling for `MarkdownView`.
//!
//! Kept separate from the main widget type so selection, copy, link activation,
//! and image context-menu interactions do not push `markdown.rs` over the
//! project file-length limit.

use crate::cursor::{set_cursor_icon, CursorIcon};
use crate::event::{Event, EventResult, Key, MouseButton};

use super::MarkdownView;

impl MarkdownView {
    pub(super) fn handle_markdown_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                if let Some(block_idx) = self.dragging_block() {
                    if self.drag_block_scrollbar(block_idx, *pos) {
                        crate::animation::request_draw();
                    }
                    return EventResult::Consumed;
                }
                if self.selecting_drag {
                    if let Some(start) = self.selection_drag_start {
                        let dx = pos.x - start.x;
                        let dy = pos.y - start.y;
                        if dx * dx + dy * dy > 9.0 {
                            self.selection_dragged = true;
                        }
                    }
                    if let Some(text_pos) = self.text_pos_at(*pos) {
                        self.selection_cursor = Some(text_pos);
                        crate::animation::request_draw();
                    }
                    set_cursor_icon(CursorIcon::Text);
                    return EventResult::Consumed;
                }
                if self.hit_scrollbar(*pos).is_some() {
                    set_cursor_icon(CursorIcon::ResizeHorizontal);
                }
                if self.context_menu.is_some() || self.hit_image(*pos).is_some() {
                    set_cursor_icon(CursorIcon::ContextMenu);
                } else if self.link_at(*pos).is_some() {
                    set_cursor_icon(CursorIcon::PointingHand);
                } else if self.text_pos_at(*pos).is_some() {
                    set_cursor_icon(CursorIcon::Text);
                }
                EventResult::Ignored
            }
            Event::MouseWheel {
                pos,
                delta_x,
                delta_y,
                modifiers,
            } => {
                if let Some((block_idx, viewport, content)) = self.point_over_scrollable_block(*pos)
                {
                    let delta = if delta_x.abs() > 1e-6 {
                        delta_x * 40.0
                    } else if modifiers.shift {
                        delta_y * 40.0
                    } else {
                        0.0
                    };
                    if delta.abs() > 1e-6 {
                        let old = self.block_scroll_offset(block_idx);
                        if self.scroll_block_to(block_idx, old + delta, viewport, content) {
                            crate::animation::request_draw();
                        }
                        return EventResult::Consumed;
                    }
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                pos,
                button: MouseButton::Left,
                ..
            } => self.handle_left_mouse_down(*pos),
            Event::MouseDown {
                pos,
                button: MouseButton::Right,
                ..
            } => {
                if self.open_image_context_menu(*pos) {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => self.handle_left_mouse_up(*pos),
            Event::FocusGained => {
                self.focused = true;
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::FocusLost => {
                self.focused = false;
                self.selecting_drag = false;
                self.selection_drag_start = None;
                self.context_menu = None;
                crate::animation::request_draw();
                EventResult::Ignored
            }
            Event::KeyDown { key, modifiers } => {
                let cmd = modifiers.ctrl || modifiers.meta;
                match key {
                    Key::Char('a') | Key::Char('A') if cmd => {
                        self.select_all_text();
                        crate::animation::request_draw();
                        EventResult::Consumed
                    }
                    Key::Char('c') | Key::Char('C') if cmd => {
                        self.copy_selection();
                        EventResult::Consumed
                    }
                    Key::Insert if modifiers.ctrl => {
                        self.copy_selection();
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_left_mouse_down(&mut self, pos: crate::geometry::Point) -> EventResult {
        if self.context_menu.is_some() && self.handle_context_menu_mouse_down(pos) {
            return EventResult::Consumed;
        }
        if let Some(hit) = self.hit_scrollbar(pos) {
            let drag_thumb_offset = pos.x - self.padding - hit.thumb.x;
            let scroll = self.block_scroll_mut(hit.block_idx);
            if hit.on_thumb {
                scroll.dragging = true;
                scroll.drag_thumb_offset = drag_thumb_offset;
            } else {
                let center = pos.x - self.padding - hit.thumb.width * 0.5;
                let travel = (hit.bar.width - hit.thumb.width).max(1.0);
                let frac = ((center - hit.bar.x) / travel).clamp(0.0, 1.0);
                self.scroll_block_to(
                    hit.block_idx,
                    frac * (hit.content_width - hit.viewport_width).max(0.0),
                    hit.viewport_width,
                    hit.content_width,
                );
            }
            crate::animation::request_draw();
            return EventResult::Consumed;
        }

        self.context_menu = None;
        if let Some(text_pos) = self.text_pos_at(pos) {
            self.selection_anchor = Some(text_pos);
            self.selection_cursor = Some(text_pos);
            self.selection_drag_start = Some(pos);
            self.selection_dragged = false;
            self.selecting_drag = true;
            crate::animation::request_draw();
            EventResult::Consumed
        } else if self.link_at(pos).is_some() {
            EventResult::Consumed
        } else {
            self.clear_selection();
            EventResult::Ignored
        }
    }

    fn handle_left_mouse_up(&mut self, pos: crate::geometry::Point) -> EventResult {
        let was_dragging = self.block_scrolls.iter().any(|scroll| scroll.dragging);
        if was_dragging {
            for scroll in &mut self.block_scrolls {
                scroll.dragging = false;
            }
            crate::animation::request_draw();
            return EventResult::Consumed;
        }
        if self.selecting_drag {
            self.selecting_drag = false;
            self.selection_drag_start = None;
            let was_drag = self.selection_dragged;
            self.selection_dragged = false;
            if !was_drag {
                self.clear_selection();
                self.activate_link_at(pos);
            }
            crate::animation::request_draw();
            return EventResult::Consumed;
        }

        if self.activate_link_at(pos) {
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    }

    fn activate_link_at(&mut self, pos: crate::geometry::Point) -> bool {
        let url = self.link_at(pos).map(str::to_string);
        if let Some(url) = url {
            if let Some(cb) = self.on_link_click.as_mut() {
                cb(&url);
            }
            true
        } else {
            false
        }
    }
}
