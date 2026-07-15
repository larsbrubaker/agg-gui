//! Change-notification wiring for [`TextArea`], split out of `text_area.rs`
//! to keep that file under the project's 800-line cap.
//!
//! Mirrors `TextField`'s `on_change` semantics (see
//! `text_field/filter.rs::notify_change`): a `FnMut(&str)` callback that fires
//! after every text mutation, letting a caller capture edits back into a shared
//! cell. The dispatcher is invoked from the two internal mutation funnels
//! ([`TextArea::insert_str`] and [`TextArea::delete`]) and, when the content
//! epoch advances, from the pre-default key-chord interceptor in `widget_impl`.

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
}
