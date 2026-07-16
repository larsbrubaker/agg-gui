//! `color_wheel_picker_dialog` — convenience constructor that wraps a
//! [`ColorWheelPicker`] in a [`Window`] for popup use.
//!
//! The dialog is what AtomArtist (and any other caller that wants a
//! floating colour picker) consumes: drop the returned `Box<dyn Widget>`
//! into a top-level `Stack` and let the user move it around / close it
//! via the window chrome.  Picker callbacks (`on_change`, `on_select`,
//! `on_cancel`) keep firing on the inner widget exactly as if it had
//! been placed directly into the tree.

use crate::geometry::{Rect, Size};
use crate::widget::Widget;
use crate::widgets::window::Window;

use super::{picker_height, picker_width, ColorWheelPicker};

/// Wrap `picker` in a draggable, auto-sized `Window` titled `title`.
///
/// The window starts at `(60, 60)` and is sized exactly to the
/// picker's natural extent; the title bar + window padding push the
/// outer bounds slightly beyond `picker_width / picker_height`, but
/// the framework's [`Window::with_auto_size`] keeps the chrome
/// hugging the content as the picker reconfigures (e.g. when the
/// caller flips `with_allow_none`).
pub fn color_wheel_picker_dialog(
    picker: ColorWheelPicker,
    title: impl Into<String>,
) -> Box<dyn Widget> {
    Box::new(build_window(picker, title))
}

/// Like [`color_wheel_picker_dialog`] but forwards the window's title-bar ×
/// button (and Escape, which routes through the same [`Window`] close path) to
/// `on_close`.
///
/// The picker's own Cancel button fires `ColorWheelPicker::on_cancel`; the
/// window chrome's close affordance is a *separate* route that bypasses it.
/// Hosts that must unwind state when the dialog is dismissed by any means — the
/// RichTextEdit demo cancels its live colour preview — wire `on_close` to the
/// same teardown as `on_cancel`, so closing via × or Escape can't strand the
/// preview session.
pub fn color_wheel_picker_dialog_with_on_close(
    picker: ColorWheelPicker,
    title: impl Into<String>,
    on_close: impl FnMut() + 'static,
) -> Box<dyn Widget> {
    Box::new(build_window(picker, title).on_close(on_close))
}

/// Shared builder: wrap `picker` in a modal, auto-sized, non-resizable window.
fn build_window(picker: ColorWheelPicker, title: impl Into<String>) -> Window {
    let allow_none = picker.allow_none;
    let show_alpha = picker.show_alpha;
    let font = picker.font.clone();

    let content_w = picker_width();
    let content_h = picker_height(allow_none, show_alpha);
    // ~28px title bar + a small breathing room.
    let win_h = content_h + 28.0 + 4.0;

    Window::new(title, font, Box::new(picker))
        .with_bounds(Rect::new(60.0, 60.0, content_w, win_h))
        .with_min_size(Size::new(content_w, win_h))
        .with_auto_size(true)
        .with_resizable(false)
        .with_constrain(true)
        // Grab all pointer/keyboard input over the dialog while it is open so
        // clicks (including the close button) never leak to widgets painted
        // beneath the floating window.  See `Window::with_modal`.
        .with_modal(true)
}
