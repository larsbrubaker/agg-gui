//! Undo / redo entry points for [`TextField`], split out of `text_field.rs`
//! (800-line cap). Each flushes the pending coalesced insert first, steps the
//! shared [`UndoBuffer`](crate::undo::UndoBuffer), and re-clamps the caret to
//! the restored text.

use super::*;

impl TextField {
    pub(super) fn do_undo(&mut self) {
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

    pub(super) fn do_redo(&mut self) {
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
}
