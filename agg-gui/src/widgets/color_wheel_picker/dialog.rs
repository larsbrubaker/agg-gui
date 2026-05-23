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
    let allow_none = picker.allow_none;
    let show_alpha = picker.show_alpha;
    let font = picker.font.clone();

    let content_w = picker_width();
    let content_h = picker_height(allow_none, show_alpha);
    // ~28px title bar + a small breathing room.
    let win_h = content_h + 28.0 + 4.0;

    let win = Window::new(title, font, Box::new(picker))
        .with_bounds(Rect::new(60.0, 60.0, content_w, win_h))
        .with_min_size(Size::new(content_w, win_h))
        .with_auto_size(true)
        .with_resizable(false)
        .with_constrain(true);

    Box::new(win)
}
