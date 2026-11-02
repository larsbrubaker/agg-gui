//! Which mouse buttons are held down right now, for a widget that must not
//! act on a press the user has already let go of.
//!
//! `Event::MouseMove` carries only `pos`, and a release does not always
//! reach the widget that saw the press: when the pressed widget's subtree
//! leaves the tree mid-press, the capture moves to a surviving ancestor
//! (`widget/app/path_anchor.rs`) and the release goes there; a release
//! outside the window may never arrive at all. A widget that keeps its own
//! "press under way" state (a drag source waiting for the pointer to travel
//! far enough) asks [`mouse_button_held`] on each move and ends the press
//! when its button is no longer down.
//!
//! [`crate::widget::App`] keeps this current whatever widget (if any)
//! handles the events: its press entry point marks the button held before
//! dispatching, its release entry point clears it before dispatching, and
//! the window losing activation clears every button (the release of a
//! press the user carried outside the window goes to whatever they switched
//! to). Touch emulation's finger presses are replayed through the same
//! entry points, so a finger counts as the button it is dispatched as (a
//! tap or a widget's own drag as Left, a pan as Middle, a held long press as
//! Right); a finger press that ends without a release (`EmuCmd::CancelPress`,
//! a held lift whose capture was dropped) clears it too. Per UI thread, like
//! [`super::current_modifiers`].

use std::cell::RefCell;

use super::MouseButton;

thread_local! {
    /// The buttons held, at most one entry per button. A handful at most,
    /// so a list beats a 259-bit set for `Other(u8)`.
    static HELD: RefCell<Vec<MouseButton>> = const { RefCell::new(Vec::new()) };
}

/// Whether `button` is held down as of the most recent pointer event the
/// [`crate::widget::App`] processed. `false` before any press, after its
/// release (wherever the release was delivered, even to no widget), and
/// after the window lost activation.
pub fn mouse_button_held(button: MouseButton) -> bool {
    HELD.with(|held| held.borrow().contains(&button))
}

/// Mark `button` held. Called by `App`'s press entry point.
pub(crate) fn note_button_down(button: MouseButton) {
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if !held.contains(&button) {
            held.push(button);
        }
    });
}

/// Mark `button` released. Called by `App`'s release entry point and by a
/// touch press that ends without a release.
pub(crate) fn note_button_up(button: MouseButton) {
    HELD.with(|held| held.borrow_mut().retain(|b| *b != button));
}

/// Mark every button released: the window lost activation, or the finger
/// emulating a press let go of it without a release.
pub(crate) fn release_all_buttons() {
    HELD.with(|held| held.borrow_mut().clear());
}
