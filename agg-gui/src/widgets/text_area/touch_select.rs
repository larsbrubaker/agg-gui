//! [`TextArea`]'s side of touch text selection
//! ([`crate::widgets::touch_selection`]): the wrapped lines' geometry (caret
//! positions, where each line's text ends) and the area's edit verbs, which
//! the shared controller drives for long presses, handle drags, taps and the
//! floating toolbar. `event_impl.rs` routes events through the controller,
//! `widget_impl.rs` its paint and overlay hit test.
//!
//! A handle dragged past the top or bottom edge aims at a line out of view;
//! [`TextArea::ensure_cursor_visible`] then scrolls it in, so the area
//! auto-scrolls as the finger moves on.

use super::*;
use crate::widgets::text_context_menu::TextMenuAction;
use crate::widgets::touch_selection::handles::CaretLine;
use crate::widgets::touch_selection::{TouchSelectable, TouchSelection};

impl TextArea {
    /// The touch selection state, for tests.
    #[cfg(test)]
    pub(crate) fn touch_selection_state(&self) -> &TouchSelection {
        &self.touch_selection
    }

    /// The visual line under the widget-local `y`, `None` below the last.
    fn line_at_y(&self, y: f64) -> Option<usize> {
        if self.cached_lines.is_empty() || self.cached_line_h <= 0.0 {
            return None;
        }
        let from_top = ((self.content_top_y() - y) / self.cached_line_h).floor();
        let idx = from_top.max(0.0) as usize;
        (idx < self.cached_lines.len()).then_some(idx)
    }
}

impl TouchSelectable for TextArea {
    fn touch_selection(&self) -> &TouchSelection {
        &self.touch_selection
    }

    fn touch_selection_mut(&mut self) -> &mut TouchSelection {
        &mut self.touch_selection
    }

    fn ts_anchor_cursor(&self) -> (usize, usize) {
        let st = self.edit.borrow();
        (st.anchor, st.cursor)
    }

    fn ts_select(&mut self, anchor: usize, cursor: usize) {
        self.set_caret_anchor(cursor, anchor);
        self.ensure_cursor_visible();
        self.focus_time = Some(crate::clock::now());
        crate::animation::request_draw();
    }

    fn ts_text(&self) -> String {
        self.text()
    }

    fn ts_offset_at(&self, local: Point) -> usize {
        self.byte_offset_at(local)
    }

    fn ts_on_text(&self, local: Point) -> bool {
        let Some(idx) = self.line_at_y(local.y) else {
            return false;
        };
        let line = &self.cached_lines[idx];
        line.end > line.start && local.x <= self.pos_for_cursor(line.end).x
    }

    fn ts_caret_line(&self, offset: usize) -> CaretLine {
        let p = self.pos_for_cursor(offset);
        CaretLine {
            x: p.x,
            bottom: p.y,
            top: p.y + self.cached_line_h,
        }
    }

    fn ts_text_rect(&self) -> Rect {
        Rect::new(
            self.padding,
            self.padding,
            self.inner_width(),
            self.inner_height(),
        )
    }

    fn ts_editable(&self) -> bool {
        // A TextArea has no read-only mode.
        true
    }

    fn ts_toolbar_allowed(&self) -> bool {
        self.context_menu_enabled
    }

    fn ts_apply(&mut self, action: TextMenuAction) {
        self.apply_text_menu_action(action);
    }

    fn ts_autoscroll(&mut self, dy: f64) -> bool {
        let before = self.scroll_offset();
        self.set_scroll_offset(before + dy);
        self.scroll_offset() != before
    }

    fn ts_font(&self) -> Arc<Font> {
        crate::font_settings::current_system_font().unwrap_or_else(|| Arc::clone(&self.font))
    }
}
