//! Window activation on [`App`]: the window gaining or losing activation
//! (agg-sharp's `SystemWindow.Deactivated`), and the pointer capture a
//! deactivation ends. Split out of `app.rs` (800-line guardrail); the event
//! shapes are documented on [`Event::WindowActivated`],
//! [`Event::WindowDeactivated`] and [`Event::MouseCaptureLost`]. Platform
//! shells call these through
//! [`InputForwarder`](crate::shell_input::InputForwarder)
//! (`ForwarderEvent::WindowActivated` / `WindowDeactivated`): winit's
//! `WindowEvent::Focused` in `agg-gui-shell`, the page's `focus` / `blur` in
//! `agg-gui-web-shell`. Tests and automation send the same events as
//! simulated input.

use crate::event::Event;
use crate::geometry::Point;
use crate::widget::tree::{deliver_to_all, dispatch_event};
use crate::widget::App;

impl App {
    /// The window became active again. Sends [`Event::WindowActivated`] to
    /// every widget, once per change: a call while already active does
    /// nothing.
    pub fn on_window_activated(&mut self) {
        if !crate::event::set_window_active(true) {
            return;
        }
        self.resolve_tracked_paths();
        deliver_to_all(self.root.as_mut(), &Event::WindowActivated);
        crate::animation::request_draw();
    }

    /// The window lost activation. Once per change (a call while already
    /// inactive does nothing):
    ///
    /// 1. A widget holding pointer capture receives
    ///    [`Event::MouseCaptureLost`] and the capture is cleared: the
    ///    release of its press goes to whatever the user switched to, so
    ///    without this the drag would follow the pointer, button up, until
    ///    the next click. No `MouseUp` is synthesized — a button would
    ///    click, a drag would commit.
    /// 2. Every widget receives [`Event::WindowDeactivated`].
    ///
    /// Keyboard focus is left alone (no `FocusLost`), as agg-sharp leaves a
    /// text field focused across an app switch.
    pub fn on_window_deactivated(&mut self) {
        if !crate::event::set_window_active(false) {
            return;
        }
        self.resolve_tracked_paths();
        self.cancel_pointer_capture();
        deliver_to_all(self.root.as_mut(), &Event::WindowDeactivated);
        crate::animation::request_draw();
    }

    /// Whether the window is active; see [`crate::event::window_is_active`].
    pub fn window_is_active(&self) -> bool {
        crate::event::window_is_active()
    }

    /// End the pointer capture without a release: tell the holder with
    /// [`Event::MouseCaptureLost`], then forget the capture.
    fn cancel_pointer_capture(&mut self) {
        let Some(path) = self.captured.clone() else {
            return;
        };
        dispatch_event(
            &mut self.root,
            &path,
            &Event::MouseCaptureLost,
            Point::ORIGIN,
        );
        self.store_captured(None);
    }
}
