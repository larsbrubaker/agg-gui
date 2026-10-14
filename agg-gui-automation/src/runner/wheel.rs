//! The runner's mouse wheel: turn the wheel where the pointer is, or over a
//! named widget after a stepped move onto it.
//!
//! Rust-only gestures (C#'s `AutomationRunner` has no wheel method; its
//! `ScrollIntoView`, ported in `scroll.rs`, sets the scroll position
//! directly). A turn reaches the `App` as the shells deliver one: a
//! [`ForwarderEvent::Wheel`] through the window's driver
//! ([`UiDriver::send`]) at the pointer, with the held modifiers, so Shift
//! held by [`press_modifier_keys`](AutomationRunner::press_modifier_keys)
//! turns it sideways as it does for a real wheel.
//!
//! Notches are agg-gui's wheel unit, what `agg-gui-shell` forwards for a
//! wheel's `LineDelta` (one detent is one notch, fractional for a
//! high-resolution wheel) and the web shell for a DOM `wheel` event: positive
//! is the wheel turned forward, toward the content above (a scroll view's
//! offset goes down by `agg_gui::wheel::PIXELS_PER_NOTCH` per notch);
//! negative scrolls down. Like a click, a turn draws before and after.

use agg_gui::shell_input::ForwarderEvent;

use super::{AutomationRunner, DragOpts};
use crate::driver::UiDriver;

impl<D: UiDriver> AutomationRunner<D> {
    /// Turn the wheel `notches` at the pointer: a draw, the wheel event, a
    /// draw. Panics when the pointer is outside the window, where the
    /// window would hear no wheel.
    pub fn scroll_wheel(&mut self, notches: f64) -> &mut Self {
        self.check_not_timed_out();
        let at = self.input.current_mouse_position();
        let (width, height) = UiDriver::size_px(&self.driver);
        let inside = at.x >= 0
            && i64::from(at.x) <= i64::from(width)
            && at.y >= 0
            && i64::from(at.y) <= i64::from(height);
        if !inside {
            panic!(
                "ScrollWheel Failed: the pointer ({}, {}) is outside the {width} x {height} window",
                at.x, at.y
            );
        }
        self.wait_for_draw();
        UiDriver::send(
            &mut self.driver,
            ForwarderEvent::Wheel {
                at: Some((f64::from(at.x), f64::from(at.y))),
                delta_x: 0.0,
                delta_y: notches,
                modifiers: None,
            },
        );
        self.wait_for_draw()
    }

    /// Move onto the center of the widget named `widget_name` in eased,
    /// paced steps (as [`drop_by_name`](Self::drop_by_name) moves), then
    /// [`scroll_wheel`](Self::scroll_wheel) `notches` there. Panics with
    /// C#'s not-found message when no such widget turns up.
    pub fn scroll_wheel_by_name(&mut self, widget_name: &str, notches: f64) -> &mut Self {
        let opts = DragOpts::default();
        let (widget, offset_hint) = self.drag_target("ScrollWheelByName", widget_name, &opts);
        let screen_position = self.drag_point(
            "ScrollWheelByName",
            &widget,
            opts.offset,
            offset_hint,
            opts.origin,
        );
        self.set_mouse_cursor_position(screen_position.x, screen_position.y);
        self.scroll_wheel(notches)
    }
}
