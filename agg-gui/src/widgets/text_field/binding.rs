//! Setting a [`TextField`]'s text and caret from outside: the external text
//! cell binding, C# agg-sharp's `SetCursorPosition`, and the raw selection
//! the on-screen keyboard's caret gestures read and put back
//! ([`Widget::text_selection`](crate::widget::Widget::text_selection)).

use super::*;

impl TextField {
    /// Bind the field to external text state.
    ///
    /// `layout` picks up external writes (e.g. a Clear button) and
    /// `on_change` writes user edits back into the cell.
    pub fn with_text_cell(mut self, cell: Rc<RefCell<String>>) -> Self {
        let text = cell.borrow().clone();
        self.set_text(text);
        self.text_cell = Some(cell);
        self
    }

    pub(crate) fn sync_from_text_cell(&mut self) {
        let Some(cell) = &self.text_cell else {
            return;
        };
        let external = cell.borrow().clone();
        if external != self.edit.borrow().text {
            self.set_text(external);
        }
    }

    /// C# `SetCursorPosition`: put the caret before character `char_index`
    /// (clamped to the text), collapse the selection and keep the caret in
    /// view. `set_text` followed by this is C#'s
    /// `SetTextAsUndoBaseline(text, charIndex)` seeding.
    pub fn set_cursor_position(&mut self, char_index: usize) {
        self.flush_pending();
        {
            let mut st = self.edit.borrow_mut();
            let byte = st
                .text
                .char_indices()
                .nth(char_index)
                .map_or(st.text.len(), |(i, _)| i);
            st.cursor = byte;
            st.anchor = byte;
        }
        self.ensure_cursor_visible();
        crate::animation::request_draw();
        self.suggest_after_edit();
    }

    /// The selection as `(anchor, caret)` byte offsets.
    pub(super) fn anchor_and_caret(&self) -> (usize, usize) {
        let st = self.edit.borrow();
        (st.anchor, st.cursor)
    }

    /// Put the selection at `(anchor, caret)` byte offsets (clamped to the
    /// text and its character boundaries) and keep the caret in view.
    pub(super) fn set_anchor_and_caret(&mut self, anchor: usize, caret: usize) {
        self.flush_pending();
        {
            let mut st = self.edit.borrow_mut();
            st.anchor = crate::widgets::text_field_core::floor_char_boundary(&st.text, anchor);
            st.cursor = crate::widgets::text_field_core::floor_char_boundary(&st.text, caret);
        }
        self.ensure_cursor_visible();
        crate::animation::request_draw();
    }
}
