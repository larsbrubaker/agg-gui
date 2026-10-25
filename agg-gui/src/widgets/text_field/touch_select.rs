//! [`TextField`]'s side of touch text selection
//! ([`crate::widgets::touch_selection`]): the single text line's geometry
//! (caret positions, where the text ends) and its edit verbs, which the
//! shared controller drives for long presses, handle drags, taps and the
//! floating toolbar. `widget_impl.rs` routes events, paint and overlay hit
//! tests through the controller.

use super::*;
use crate::geometry::Point;
use crate::widgets::text_context_menu::TextMenuAction;
use crate::widgets::touch_selection::handles::CaretLine;
use crate::widgets::touch_selection::{TouchSelectable, TouchSelection};

impl TextField {
    /// X (widget-local) of the caret at byte `offset` of the text, with the
    /// horizontal scroll and password masking folded in.
    fn x_at_offset(&self, offset: usize) -> f64 {
        let font = self.active_font();
        let st = self.edit.borrow();
        let prefix = st.text.get(..offset.min(st.text.len())).unwrap_or("");
        let advance = if self.masking_active() {
            let masked = "•".repeat(prefix.chars().count());
            measure_advance(&font, &masked, self.font_size)
        } else {
            measure_advance(&font, prefix, self.font_size)
        };
        self.text_insets().left - self.scroll_x + advance
    }

    /// The touch selection state, for tests.
    #[cfg(test)]
    pub(crate) fn touch_selection_state(&self) -> &TouchSelection {
        &self.touch_selection
    }

    /// Whether the open right-click menu's `action` is enabled, for tests.
    #[cfg(test)]
    pub(crate) fn menu_action_enabled(&self, action: TextMenuAction) -> Option<bool> {
        self.context_menu.action_enabled(action)
    }
}

impl TouchSelectable for TextField {
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
        self.flush_pending();
        {
            let mut st = self.edit.borrow_mut();
            let len = st.text.len();
            st.anchor = anchor.min(len);
            st.cursor = cursor.min(len);
        }
        self.ensure_cursor_visible();
        self.focus_time = Some(crate::clock::now());
        crate::animation::request_draw();
    }

    fn ts_text(&self) -> String {
        self.text()
    }

    fn ts_offset_at(&self, local: Point) -> usize {
        let tx = local.x - self.text_insets().left + self.scroll_x;
        let text = self.text();
        self.click_to_cursor(&text, tx)
    }

    fn ts_on_text(&self, local: Point) -> bool {
        let len = self.edit.borrow().text.len();
        len > 0 && local.x <= self.x_at_offset(len)
    }

    fn ts_caret_line(&self, offset: usize) -> CaretLine {
        let font = self.active_font();
        let m = crate::text::measure_text_metrics(&font, "Ag", self.font_size);
        let baseline = self.text_baseline(self.bounds.height, &m);
        CaretLine {
            x: self.x_at_offset(offset),
            bottom: baseline - m.descent,
            top: baseline + m.ascent,
        }
    }

    fn ts_text_rect(&self) -> Rect {
        let ins = self.text_insets();
        Rect::new(
            ins.left,
            0.0,
            (self.bounds.width - ins.left - ins.right).max(0.0),
            self.bounds.height,
        )
    }

    fn ts_editable(&self) -> bool {
        !self.read_only
    }

    fn ts_secret(&self) -> bool {
        self.password_mode
    }

    fn ts_toolbar_allowed(&self) -> bool {
        self.context_menu_enabled
    }

    fn ts_apply(&mut self, action: TextMenuAction) {
        self.apply_text_menu_action(action);
    }

    fn ts_font(&self) -> Arc<Font> {
        self.active_font()
    }
}
