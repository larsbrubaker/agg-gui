//! Input-method (IME) plumbing between winit and agg-gui: winit's
//! `WindowEvent::Ime` becomes `ForwarderEvent::ImePreedit` / `TextInput`, and
//! [`ImeSync`] keeps the OS side in step with the app, enabling IME exactly
//! while a text widget has focus and placing the candidate window at the
//! caret (`Window::set_ime_allowed`, `Window::set_ime_cursor_area`).
//!
//! The decisions are pure ([`ImeSync::update`], [`forwarder_event`]) so they
//! are unit tested without a window.

use agg_gui::shell_input::ForwarderEvent;
use agg_gui::{App, Rect};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::Ime;
use winit::window::Window;

/// Map a winit IME event to the forwarder event the app understands.
/// `Enabled` carries nothing; `Disabled` ends any composition on screen;
/// winit sends `Preedit("", None)` before a `Commit`, so a composition is
/// already cleared when the committed text arrives.
pub(crate) fn forwarder_event(ime: Ime) -> Option<ForwarderEvent> {
    match ime {
        Ime::Enabled => None,
        Ime::Preedit(text, cursor) => Some(ForwarderEvent::ImePreedit { text, cursor }),
        Ime::Commit(text) => Some(ForwarderEvent::TextInput { text }),
        Ime::Disabled => Some(ForwarderEvent::ImePreedit {
            text: String::new(),
            cursor: None,
        }),
    }
}

/// What to tell the OS after a change in what the app wants.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImeUpdate {
    /// New allowed state, when it changed.
    pub(crate) allowed: Option<bool>,
    /// New candidate-window rectangle (physical px, Y-down), when it changed
    /// and IME is on.
    pub(crate) area: Option<Rect>,
}

/// The IME state last sent to the OS, so it is only called when it changes.
#[derive(Default)]
pub(crate) struct ImeSync {
    allowed: bool,
    area: Option<Rect>,
}

impl ImeSync {
    /// Compare what the app wants (`wants`, and the caret rectangle `area`)
    /// with what the OS was last told; the returned update lists the calls to
    /// make.
    pub(crate) fn update(&mut self, wants: bool, area: Option<Rect>) -> ImeUpdate {
        let allowed = (wants != self.allowed).then_some(wants);
        self.allowed = wants;
        if !wants {
            self.area = None;
            return ImeUpdate {
                allowed,
                area: None,
            };
        }
        // A newly allowed IME needs its area even when it equals the old one.
        let area = match area {
            Some(a) if allowed.is_some() || self.area != Some(a) => Some(a),
            _ => None,
        };
        if area.is_some() {
            self.area = area;
        }
        ImeUpdate { allowed, area }
    }

    /// Bring the window's IME in step with the app's focused widget.
    pub(crate) fn sync(&mut self, window: &Window, app: &App) {
        let update = self.update(app.wants_ime(), app.ime_cursor_area());
        if let Some(allowed) = update.allowed {
            window.set_ime_allowed(allowed);
        }
        if let Some(r) = update.area {
            window.set_ime_cursor_area(
                PhysicalPosition::new(r.x, r.y),
                PhysicalSize::new(r.width.max(1.0), r.height.max(1.0)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winit_ime_events_become_forwarder_events() {
        assert!(forwarder_event(Ime::Enabled).is_none());
        match forwarder_event(Ime::Preedit("にほ".into(), Some((0, 3)))) {
            Some(ForwarderEvent::ImePreedit { text, cursor }) => {
                assert_eq!(text, "にほ");
                assert_eq!(cursor, Some((0, 3)));
            }
            other => panic!("expected a preedit, got {other:?}"),
        }
        match forwarder_event(Ime::Commit("日本".into())) {
            Some(ForwarderEvent::TextInput { text }) => assert_eq!(text, "日本"),
            other => panic!("expected committed text, got {other:?}"),
        }
        match forwarder_event(Ime::Disabled) {
            Some(ForwarderEvent::ImePreedit { text, cursor }) => {
                assert!(text.is_empty() && cursor.is_none());
            }
            other => panic!("expected an empty preedit, got {other:?}"),
        }
    }

    #[test]
    fn ime_follows_focus_and_the_caret() {
        let mut sync = ImeSync::default();
        let a = Rect::new(10.0, 20.0, 1.0, 16.0);
        let b = Rect::new(30.0, 20.0, 1.0, 16.0);
        // No text widget: nothing to tell the OS.
        assert_eq!(
            sync.update(false, None),
            ImeUpdate {
                allowed: None,
                area: None
            }
        );
        // Focus enters a text field: enable and place.
        assert_eq!(
            sync.update(true, Some(a)),
            ImeUpdate {
                allowed: Some(true),
                area: Some(a)
            }
        );
        // Same caret: no calls.
        assert_eq!(
            sync.update(true, Some(a)),
            ImeUpdate {
                allowed: None,
                area: None
            }
        );
        // The caret moves: place only.
        assert_eq!(
            sync.update(true, Some(b)),
            ImeUpdate {
                allowed: None,
                area: Some(b)
            }
        );
        // Focus leaves: disable, and re-enabling places again.
        assert_eq!(
            sync.update(false, None),
            ImeUpdate {
                allowed: Some(false),
                area: None
            }
        );
        assert_eq!(
            sync.update(true, Some(b)),
            ImeUpdate {
                allowed: Some(true),
                area: Some(b)
            }
        );
    }
}
