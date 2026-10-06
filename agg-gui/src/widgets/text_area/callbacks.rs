//! Change- and edit-complete-notification wiring for [`TextArea`], split out
//! of `text_area.rs` to keep that file under the project's 800-line cap.
//!
//! `on_change` mirrors `TextField`'s `on_change` semantics (see
//! `text_field/filter.rs::notify_change`): a `FnMut(&str)` callback that fires
//! after every text mutation, letting a caller capture edits back into a shared
//! cell. The dispatcher is invoked from the two internal mutation funnels
//! ([`TextArea::insert_str`] and [`TextArea::delete`]) and, when the content
//! epoch advances, from the pre-default key-chord interceptor in `widget_impl`.
//!
//! `on_edit_complete` mirrors `TextField`'s `on_edit_complete`: it fires once
//! when focus leaves after the text changed since focus was gained. Unlike
//! `TextField`, no key commits — Enter (with or without modifiers) inserts a
//! newline in a multi-line editor, so focus loss is the only commit point.

use super::*;

impl TextArea {
    /// Install a callback fired after every text change.
    ///
    /// Fires for: typing, Enter, Tab-insert, backspace/delete, paste, cut, and
    /// key-intercept edits that advance the content epoch. It does NOT fire for
    /// purely programmatic mutations pushed straight into the shared
    /// [`TextEditState`] from outside the widget (those bypass the mutation
    /// funnel; the epoch mechanism still re-wraps them on the next layout).
    pub fn on_change(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_change = Some(Box::new(cb));
        self
    }

    /// Invoke the change callback with the current text. Callers must have
    /// already applied the mutation (and any `note_text_change`) before calling
    /// this so the callback observes the final content.
    pub(crate) fn notify_change(&mut self) {
        if let Some(mut cb) = self.on_change.take() {
            let t = self.edit.borrow().text.clone();
            cb(&t);
            self.on_change = Some(cb);
        }
    }

    /// Install a callback fired when the user finishes an edit: focus leaves
    /// the area and the text differs from what it was when focus arrived.
    /// Focusing and leaving without a change does not fire it.
    ///
    /// Mirrors `TextField::on_edit_complete`, except that no key commits:
    /// Enter (including Ctrl/Cmd+Enter) inserts a newline in a `TextArea`, so
    /// focus loss is the only trigger.
    pub fn on_edit_complete(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_edit_complete = Some(Box::new(cb));
        self
    }

    /// Called on `FocusLost`: fire `on_edit_complete` when the text differs
    /// from the `FocusGained` snapshot, then re-snapshot so a repeated
    /// `FocusLost` without a new edit does not fire again.
    pub(crate) fn notify_edit_complete_if_changed(&mut self) {
        let t = self.text();
        if t == self.text_on_focus {
            return;
        }
        self.text_on_focus = t.clone();
        if let Some(mut cb) = self.on_edit_complete.take() {
            cb(&t);
            self.on_edit_complete = Some(cb);
        }
    }
}
