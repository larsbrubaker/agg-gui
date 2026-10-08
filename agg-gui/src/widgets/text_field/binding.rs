//! Setting a [`TextField`]'s text and caret from outside: the external text
//! cell binding, and C# agg-sharp's `SetCursorPosition`.

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
    }
}
