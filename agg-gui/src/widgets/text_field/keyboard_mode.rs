//! Keyboard-mode hooks for `TextField`.
//!
//! Split out of `text_field.rs` to keep that file under the 800-line
//! cap.  The storage (`keyboard_mode: Rc<Cell<KeyboardInputMode>>`)
//! lives on `TextField`; everything else — builders, the shared-cell
//! handle, and the runtime setter — lives here.
//!
//! Why a `Cell` instead of a plain field?
//! Demo / app code that wants a radio button to swap a focused field
//! between "letters" and "numeric" needs to mutate the mode after the
//! widget tree has been built and handed off as `Box<dyn Widget>`.
//! Threading a `&mut TextField` back out is awkward; an `Rc<Cell<…>>`
//! gives the demo a stable handle to flip whenever the radio fires.

use std::cell::Cell;
use std::rc::Rc;

use super::TextField;
use crate::widgets::on_screen_keyboard::KeyboardInputMode;

impl TextField {
    /// Assign a stable id for the programmatic focus channel. App code can
    /// then call [`crate::focus::request_focus(id)`](crate::focus::request_focus)
    /// to move keyboard focus here (and raise the on-screen keyboard) the
    /// next frame — e.g. to auto-focus a search field when its overlay opens.
    pub fn with_focus_id(mut self, id: crate::focus::FocusId) -> Self {
        self.focus_request_id = Some(id);
        self
    }

    /// Set the preferred keyboard input mode for this field — picks
    /// which layer the on-screen software keyboard slides up into the
    /// next time this field gains focus.  Defaults to
    /// [`KeyboardInputMode::Text`].
    ///
    /// This does **not** install a character filter: a numeric field
    /// still accepts whatever the user actually types (including via
    /// a paste or the on-screen `ABC` mode switch).  Combine with
    /// [`TextField::with_char_filter`] when you want hard validation.
    pub fn with_keyboard_mode(self, mode: KeyboardInputMode) -> Self {
        self.keyboard_mode.set(mode);
        self
    }

    /// Bind the field's keyboard mode to an externally-owned cell.
    /// The caller keeps a clone and can flip the mode at any time —
    /// the next focus event re-queries the cell, so the keyboard
    /// re-targets without a widget rebuild.  Mirrors the
    /// [`TextField::with_text_cell`](super::TextField) pattern.
    pub fn with_keyboard_mode_cell(mut self, cell: Rc<Cell<KeyboardInputMode>>) -> Self {
        self.keyboard_mode = cell;
        self
    }

    /// Read the current mode.  Used by the `Widget::text_input_mode`
    /// override and exposed publicly so app code can inspect it
    /// without going through a `dyn Widget` trait dispatch.
    pub fn keyboard_mode(&self) -> KeyboardInputMode {
        self.keyboard_mode.get()
    }

    /// Update the mode at runtime — typically called from a callback
    /// (e.g. a radio button's `on_change`).  Equivalent to mutating
    /// the cell directly via [`TextField::keyboard_mode_handle`] but
    /// reads better at the call site.
    pub fn set_keyboard_mode(&self, mode: KeyboardInputMode) {
        self.keyboard_mode.set(mode);
    }

    /// Clone the underlying cell so a parent can drive this field's
    /// mode from somewhere else in the tree (a radio button, a menu
    /// item, a feature-flag observer).  Multiple fields can share the
    /// same handle to ride a single switch.
    pub fn keyboard_mode_handle(&self) -> Rc<Cell<KeyboardInputMode>> {
        Rc::clone(&self.keyboard_mode)
    }
}
