//! Event-dispatch logic for [`super::Button`], split out of `button.rs`
//! so the parent file stays under the project's 800-line cap.
//!
//! Pulled in via `#[path]` as a *child* module of `button` so this
//! file has direct access to `Button`'s private fields and helper
//! methods — the trait impl in `button.rs` keeps only the
//! `fn on_event` shell delegating to [`Button::handle_event`].
//!
//! The button is hovered between the move that reaches it and the move or
//! `MouseLeave` that says the pointer left; its focus ring shows for focus
//! the keyboard gave, never for focus a press gave or found.

use crate::event::{Event, EventResult, MouseButton};
use crate::widget::Widget;

use super::Button;

impl Button {
    /// Process one event.  Called from `<Button as Widget>::on_event`.
    pub(super) fn handle_event(&mut self, event: &Event) -> EventResult {
        if !self.is_enabled() {
            // Clear any lingering hover / pressed state so the button
            // looks idle the instant it's disabled mid-interaction.
            self.hovered = false;
            self.pressed = false;
            return EventResult::Ignored;
        }
        match event {
            Event::MouseMove { pos } => {
                let was_hovered = self.hovered;
                let was_pressed = self.pressed;
                self.hovered = self.hit_test(*pos);
                if !self.hovered {
                    self.pressed = false;
                }
                if was_hovered != self.hovered || was_pressed != self.pressed {
                    crate::animation::request_draw();
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            // The pointer is no longer over the button or anything in it.
            // Not `MouseOut`: that also arrives when the pointer moves onto
            // the button's own label, and the button is still under it then.
            Event::MouseLeave => {
                if self.hovered || self.pressed {
                    self.hovered = false;
                    self.pressed = false;
                    crate::animation::request_draw();
                }
                EventResult::Ignored
            }
            Event::MouseDown {
                button: MouseButton::Left,
                ..
            } => {
                if !self.pressed || self.focus_visible {
                    crate::animation::request_draw();
                }
                self.pressed = true;
                // Focus a press gave (the `App` focuses the button just
                // before this press reaches it) or found draws no ring: the
                // ring shows where the keyboard is, for keyboard navigation.
                self.focus_visible = false;
                // Note: we deliberately do NOT set `hovered` here. Touch has
                // no hover phase, and nothing clears `hovered` on a
                // touchscreen (no MouseMove/MouseLeave after a tap), so
                // setting it would leave the button stuck in its hovered
                // style. The press visual is covered by `pressed`, and the
                // click test in MouseUp checks `hit_test(*pos)` directly so
                // hover-less taps still fire.
                EventResult::Consumed
            }
            Event::MouseUp {
                pos,
                button: MouseButton::Left,
                ..
            } => {
                let was_pressed = self.pressed;
                self.pressed = false;
                if was_pressed {
                    crate::animation::request_draw();
                }
                // Fire when the release lands within the button, as
                // agg-sharp's `GuiWidget.OnMouseUp` decides a click. The
                // release position decides, never the hover flag cached
                // from the last move: taps on touch never set `hovered`,
                // and a release can be reported somewhere the pointer was
                // never seen moving to, which must not click.
                if was_pressed && self.hit_test(*pos) {
                    self.fire_click();
                    // Clear the focus ring after a mouse click — the ring is a
                    // keyboard-navigation aid and should not persist after a
                    // pointer interaction.
                    self.focused = false;
                    // Click handler almost always mutates app state that
                    // affects the next paint; request one so the handler's
                    // side-effects are visible.
                    crate::animation::request_draw();
                }
                EventResult::Consumed
            }
            Event::KeyDown { key, .. } => {
                use crate::event::Key;
                match key {
                    Key::Enter | Key::Char(' ') => {
                        self.fire_click();
                        crate::animation::request_draw();
                        EventResult::Consumed
                    }
                    _ => EventResult::Ignored,
                }
            }
            Event::FocusGained => {
                let was = self.focused;
                self.focused = true;
                // Shown until a press reaches the button (`MouseDown` above),
                // which a press that gave this focus does at once.
                self.focus_visible = true;
                if !was {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::FocusLost => {
                let was_focused = self.focused;
                let was_pressed = self.pressed;
                self.focused = false;
                self.pressed = false;
                if was_focused || was_pressed {
                    crate::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}
