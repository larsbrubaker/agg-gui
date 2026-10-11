//! Undo / redo for [`TextArea`], ported from agg-sharp's
//! `TextEditUndoHistory` (Gui/TextWidgets/TextEditUndoHistory.cs) and the
//! undo half of `InternalTextEditWidget` (`Undo`, `Redo`, `ClearUndoHistory`,
//! `SetTextAsUndoBaseline`, the `Text` setter) plus its line-end
//! normalisation (`TextLineEndings`).
//!
//! As in C#, every edit is one step that pairs the state the previous step
//! (or the last undo, redo or reset) left with the state now, so each step
//! undoes to exactly where the one before it left off. A caret moved without
//! editing is not restored by undo. C#'s `MergeTypingDuringUndo` never merged
//! anything (both of its branches record a step), so each typed character is
//! its own step here too.
//!
//! The edit funnels in `edit_ops.rs` call [`TextArea::begin_undoable_edit`]
//! before and [`TextArea::record_edit`] after they change the text. A change
//! made through the shared [`TextEditState`] handle by someone else is taken
//! as the start of the next step (C#'s `AcceptUnrecordedChange` for the
//! `Text` setter), detected by its content epoch.
//!
//! A text area holds only `'\n'` line ends: loading, setting and pasting text
//! turn `"\r\n"` and a lone `'\r'` into `'\n'`, so a click at a visual
//! position maps to the same index the user sees.

use super::caret_api::byte_of_char;
use super::*;
use crate::undo::{UndoBuffer, UndoRedoCommand};

/// The text, caret and selection one undo step restores.
#[derive(Clone, PartialEq)]
pub(super) struct TextAreaUndoState {
    text: String,
    cursor: usize,
    anchor: usize,
}

impl TextAreaUndoState {
    fn capture(st: &TextEditState) -> Self {
        Self {
            text: st.text.clone(),
            cursor: st.cursor,
            anchor: st.anchor,
        }
    }
}

/// One undo step: restores `before` / `after` into the shared state and
/// advances its content epoch (never rewinds it, so the wrap cache can't
/// mistake an older text for the cached one).
struct TextAreaUndoCommand {
    target: Rc<RefCell<TextEditState>>,
    before: TextAreaUndoState,
    after: TextAreaUndoState,
}

impl TextAreaUndoCommand {
    fn apply(&self, state: &TextAreaUndoState) {
        let mut st = self.target.borrow_mut();
        st.text = state.text.clone();
        st.cursor = state.cursor.min(st.text.len());
        st.anchor = state.anchor.min(st.text.len());
        st.note_text_change();
    }
}

impl UndoRedoCommand for TextAreaUndoCommand {
    fn name(&self) -> &str {
        "edit text"
    }
    fn do_it(&mut self) {
        self.apply(&self.after);
    }
    fn undo_it(&mut self) {
        self.apply(&self.before);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The undo history of one [`TextArea`] (C#'s `TextEditUndoHistory`).
pub(super) struct TextAreaUndoHistory {
    buffer: UndoBuffer,
    /// The state the last step (or undo, redo, reset) left.
    last_recorded: TextAreaUndoState,
    /// The content epoch at `last_recorded`; a different live epoch means
    /// someone changed the text through the shared handle since.
    last_epoch: u64,
}

impl TextAreaUndoHistory {
    pub(super) fn new(st: &TextEditState) -> Self {
        Self {
            buffer: UndoBuffer::new(),
            last_recorded: TextAreaUndoState::capture(st),
            last_epoch: st.epoch,
        }
    }

    fn accept(&mut self, st: &TextEditState) {
        self.last_recorded = TextAreaUndoState::capture(st);
        self.last_epoch = st.epoch;
    }
}

/// C#'s `TextLineEndings.Normalize(text, charIndex, out normalizedCharIndex)`:
/// `"\r\n"` and a lone `'\r'` become `'\n'`, and `char_index` (a character
/// index into `text`) moves to the same character in the result.
pub fn normalize_line_endings(text: &str, char_index: usize) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut normalized_index = 0usize;
    let mut chars = text.chars().enumerate().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\r' {
            out.push('\n');
            if chars.peek().map(|&(_, n)| n) == Some('\n') {
                chars.next();
            }
        } else {
            out.push(c);
        }
        if i < char_index {
            normalized_index += 1;
        }
    }
    (out, normalized_index)
}

impl TextArea {
    /// Take an edit made through the shared state by someone else as where
    /// the next undo step starts. Called by every edit funnel before it
    /// changes the text.
    pub(super) fn begin_undoable_edit(&mut self) {
        let st = self.edit.borrow();
        if st.epoch != self.undo.last_epoch {
            self.undo.accept(&st);
        }
    }

    /// Make the edit just applied one undo step (C#'s `RecordEdit`). An edit
    /// that left the text as it was (a backspace at the start) adds no step.
    pub(super) fn record_edit(&mut self) {
        let st = self.edit.borrow();
        let now = TextAreaUndoState::capture(&st);
        let epoch = st.epoch;
        drop(st);
        if now.text != self.undo.last_recorded.text {
            self.undo.buffer.add(Box::new(TextAreaUndoCommand {
                target: Rc::clone(&self.edit),
                before: self.undo.last_recorded.clone(),
                after: now.clone(),
            }));
        }
        self.undo.last_recorded = now;
        self.undo.last_epoch = epoch;
    }

    /// Undo the last edit (Cmd/Ctrl+Z). Nothing happens when there is none.
    pub fn undo(&mut self) {
        self.begin_undoable_edit();
        if !self.undo.buffer.can_undo() {
            return;
        }
        self.undo.buffer.undo();
        self.after_history_move();
    }

    /// Redo the last undone edit (Cmd/Ctrl+Shift+Z, Ctrl+Y). Nothing happens
    /// when there is none; a new edit after an undo forgets what was undone.
    pub fn redo(&mut self) {
        self.begin_undoable_edit();
        if !self.undo.buffer.can_redo() {
            return;
        }
        self.undo.buffer.redo();
        self.after_history_move();
    }

    /// Whether [`undo`](Self::undo) has a step to take back.
    pub fn can_undo(&self) -> bool {
        self.undo.buffer.can_undo()
    }

    /// Whether [`redo`](Self::redo) has a step to reapply.
    pub fn can_redo(&self) -> bool {
        self.undo.buffer.can_redo()
    }

    /// Forget every step; the current text becomes where undo stops (C#'s
    /// `ClearUndoHistory`).
    pub fn clear_undo_history(&mut self) {
        self.undo.buffer.clear_history();
        let st = self.edit.borrow();
        self.undo.accept(&st);
    }

    /// Load `text` as the start of the undo history, with the caret (and an
    /// empty selection) at character `char_index` (C#'s
    /// `SetTextAsUndoBaseline`): loading a document is not an undo step.
    /// Line ends are normalised to `'\n'`, the caret following its
    /// character. Fires [`on_change`](Self::on_change) as C#'s
    /// `OnTextChanged` does.
    pub fn set_text_as_undo_baseline(&mut self, text: &str, char_index: usize) {
        let (text, index) = normalize_line_endings(text, char_index);
        {
            let mut st = self.edit.borrow_mut();
            let byte = byte_of_char(&text, index);
            st.text = text;
            st.cursor = byte;
            st.anchor = byte;
            st.note_text_change();
        }
        self.mark_dirty();
        self.notify_change();
        self.clear_undo_history();
        crate::animation::request_draw();
    }

    /// Replace the text programmatically (C#'s `Text` setter): line ends are
    /// normalised, and new text puts the caret at its start with nothing
    /// selected. It is not an undo step, but the next edit's undo returns to
    /// it. Setting the text the area already holds changes nothing.
    pub fn set_text(&mut self, text: &str) {
        let (text, _) = normalize_line_endings(text, 0);
        if self.edit.borrow().text == text {
            return;
        }
        {
            let mut st = self.edit.borrow_mut();
            st.text = text;
            st.cursor = 0;
            st.anchor = 0;
            st.note_text_change();
        }
        let st = self.edit.borrow().clone();
        self.undo.accept(&st);
        self.mark_dirty();
        self.notify_change();
        crate::animation::request_draw();
    }

    /// Re-wrap, notify and re-show the caret after an undo or redo moved the
    /// text, and start the next step from the restored state.
    fn after_history_move(&mut self) {
        let st = self.edit.borrow().clone();
        self.undo.accept(&st);
        self.mark_dirty();
        self.notify_change();
        self.ensure_cursor_visible();
        crate::animation::request_draw();
    }
}
