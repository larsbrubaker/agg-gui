//! `TextField` — single-line text input with cursor movement.
//!
//! Phase 4 scope: cursor movement (arrow keys, Home, End), character
//! insertion and deletion (Backspace, Delete). No clipboard, no selection.
//!
//! The cursor position is stored as a **byte offset** into `self.text`
//! (a UTF-8 `String`). Helper functions ensure the offset always sits on
//! a valid char boundary.

use std::sync::Arc;

use crate::color::Color;
use crate::event::{Event, EventResult, Key, MouseButton};
use crate::geometry::{Rect, Size};
use crate::draw_ctx::DrawCtx;
use crate::text::Font;
use crate::widget::Widget;

// ---------------------------------------------------------------------------
// UTF-8 cursor helpers
// ---------------------------------------------------------------------------

fn prev_char_boundary(s: &str, byte_pos: usize) -> usize {
    let mut pos = byte_pos;
    loop {
        if pos == 0 { return 0; }
        pos -= 1;
        if s.is_char_boundary(pos) { return pos; }
    }
}

fn next_char_boundary(s: &str, byte_pos: usize) -> usize {
    let mut pos = byte_pos + 1;
    while pos <= s.len() {
        if s.is_char_boundary(pos) { return pos; }
        pos += 1;
    }
    s.len()
}

// ---------------------------------------------------------------------------
// TextField
// ---------------------------------------------------------------------------

/// A single-line text input field.
pub struct TextField {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    font: Arc<Font>,
    font_size: f64,
    /// The text content.
    pub text: String,
    /// Cursor position as a byte offset into `text`.
    cursor: usize,
    /// Placeholder shown when `text` is empty and not focused.
    pub placeholder: String,

    hovered: bool,
    focused: bool,
    padding: f64,
}

impl TextField {
    pub fn new(font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            font_size: 14.0,
            text: String::new(),
            cursor: 0,
            placeholder: String::new(),
            hovered: false,
            focused: false,
            padding: 8.0,
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }

    pub fn with_placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self.cursor = self.text.len();
        self
    }

    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p;
        self
    }
}

impl Widget for TextField {
    fn type_name(&self) -> &'static str { "TextField" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, bounds: Rect) { self.bounds = bounds; }

    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn is_focusable(&self) -> bool { true }

    fn layout(&mut self, available: Size) -> Size {
        let height = (self.font_size * 2.4).max(28.0);
        Size::new(available.width, height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let r = 6.0;
        let pad = self.padding;

        // Background
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();

        // Border — blue when focused, light gray otherwise
        let border_color = if self.focused {
            Color::rgb(0.22, 0.45, 0.88)
        } else if self.hovered {
            Color::rgb(0.7, 0.7, 0.75)
        } else {
            Color::rgb(0.82, 0.82, 0.86)
        };
        ctx.set_stroke_color(border_color);
        ctx.set_line_width(if self.focused { 2.0 } else { 1.0 });
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.stroke();

        // Set up font
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(self.font_size);

        let metrics = ctx.measure_text("Ag").unwrap_or_default();
        let baseline_y = h * 0.5 - (metrics.ascent - metrics.descent) * 0.5 + metrics.descent;
        let text_x = pad;

        // Text or placeholder
        if self.text.is_empty() && !self.focused && !self.placeholder.is_empty() {
            ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
            ctx.fill_text(&self.placeholder, text_x, baseline_y);
        } else {
            ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.9));
            ctx.fill_text(&self.text, text_x, baseline_y);
        }

        // Cursor — only when focused
        if self.focused {
            let cursor_advance = if self.cursor == 0 {
                0.0
            } else {
                ctx.measure_text(&self.text[..self.cursor])
                    .map(|m| m.width)
                    .unwrap_or(0.0)
            };
            let cx = text_x + cursor_advance;
            let cursor_top    = baseline_y + metrics.ascent;
            let cursor_bottom = baseline_y - metrics.descent;

            ctx.set_stroke_color(Color::rgb(0.22, 0.45, 0.88));
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.move_to(cx, cursor_bottom);
            ctx.line_to(cx, cursor_top);
            ctx.stroke();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseMove { pos } => {
                self.hovered = self.hit_test(*pos);
                EventResult::Ignored
            }
            Event::MouseDown { button: MouseButton::Left, .. } => {
                // Consumed so App knows to route focus here.
                EventResult::Consumed
            }
            Event::FocusGained => {
                self.focused = true;
                EventResult::Ignored
            }
            Event::FocusLost => {
                self.focused = false;
                EventResult::Ignored
            }
            Event::KeyDown { key, .. } if self.focused => {
                match key {
                    Key::Char(c) => {
                        self.text.insert(self.cursor, *c);
                        self.cursor += c.len_utf8();
                        EventResult::Consumed
                    }
                    Key::Backspace => {
                        if self.cursor > 0 {
                            let new_cursor = prev_char_boundary(&self.text, self.cursor);
                            self.text.drain(new_cursor..self.cursor);
                            self.cursor = new_cursor;
                        }
                        EventResult::Consumed
                    }
                    Key::Delete => {
                        if self.cursor < self.text.len() {
                            let end = next_char_boundary(&self.text, self.cursor);
                            self.text.drain(self.cursor..end);
                        }
                        EventResult::Consumed
                    }
                    Key::ArrowLeft => {
                        self.cursor = prev_char_boundary(&self.text, self.cursor);
                        EventResult::Consumed
                    }
                    Key::ArrowRight => {
                        if self.cursor < self.text.len() {
                            self.cursor = next_char_boundary(&self.text, self.cursor);
                        }
                        EventResult::Consumed
                    }
                    Key::Home => {
                        self.cursor = 0;
                        EventResult::Consumed
                    }
                    Key::End => {
                        self.cursor = self.text.len();
                        EventResult::Consumed
                    }
                    Key::Tab => EventResult::Ignored,   // let App handle tab-focus
                    Key::Escape => {
                        self.focused = false;
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }
}
