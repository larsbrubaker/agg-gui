//! Keyboard-input methods on [`App`] — key down / up routing through the
//! modal path, the focused widget, focus traversal, unconsumed-key
//! accelerators, the root-level default / cancel action and finally the
//! legacy global handler. Split out of `app.rs` (800-line guardrail);
//! pointer input lives in `pointer.rs`, wheel routing stays in `app.rs`.

use crate::event::{Event, EventResult, Key, Modifiers};
use crate::geometry::Point;
use crate::widget::tree::{
    activate_action_at, active_modal_path, cancel_action_path, default_action_path, dispatch_event,
    dispatch_unconsumed_key,
};
use crate::widget::App;

impl App {
    /// Key pressed. Delivered to the focused widget first, then to the visible
    /// widget tree as an unconsumed key if focus ignores it.
    pub fn on_key_down(&mut self, key: Key, mods: Modifiers) {
        // Ctrl/Meta+Tab is a *direct* focus-traversal escape hatch: it advances
        // focus without ever offering the event to the focused widget, so a
        // widget that consumes plain Tab (e.g. the rich-text editor indenting)
        // can never trap keyboard focus. Plain Tab / Shift+Tab are instead
        // dispatched to the focused widget first (below) and only fall through
        // to traversal when it Ignores them — that "consume Tab to opt out" is
        // the established way for an editor to keep Tab for itself.
        if key == Key::Tab && (mods.ctrl || mods.meta) {
            self.advance_focus(!mods.shift);
            return;
        }
        let event = Event::KeyDown {
            key: key.clone(),
            modifiers: mods,
        };
        let result = if let Some(path) = active_modal_path(self.root.as_ref()) {
            // A focused widget INSIDE the modal (a dialog's text field)
            // gets keys first; the modal subtree handles the rest (Esc).
            let target = match self.focus.clone() {
                Some(focus) if focus.starts_with(&path) => focus,
                _ => path,
            };
            dispatch_event(&mut self.root, &target, &event, Point::ORIGIN)
        } else if let Some(path) = self.focus.clone() {
            dispatch_event(&mut self.root, &path, &event, Point::ORIGIN)
        } else {
            EventResult::Ignored
        };
        if !result.is_consumed() {
            // A plain Tab / Shift+Tab the focused widget didn't consume falls
            // back to focus traversal — the default — instead of the
            // unconsumed-key / global-handler path other keys take.
            if key == Key::Tab {
                self.advance_focus(!mods.shift);
                return;
            }
            let result = dispatch_unconsumed_key(self.root.as_mut(), &key, mods);
            if !result.is_consumed() {
                // Root-level default / cancel action (`Button::with_default_action`
                // / `with_cancel_action`). A showing modal has already had its
                // turn (its subtree is skipped here) and swallows the key unless
                // it passes keys through, so only the topmost scope fires.
                let result = self.dispatch_root_action(&key);
                if !result.is_consumed() {
                    if let Some(ref mut handler) = self.global_key_handler {
                        handler(key, mods);
                    }
                }
            }
        }
    }

    /// Enter → the tree's first visible `default_action` widget;
    /// Escape → the first `cancel_action` widget. Subtrees under an
    /// active modal are skipped (see `on_key_down`).
    fn dispatch_root_action(&mut self, key: &Key) -> EventResult {
        let path = match key {
            Key::Enter => default_action_path(self.root.as_ref(), true),
            Key::Escape => cancel_action_path(self.root.as_ref(), true),
            _ => None,
        };
        match path {
            Some(path) => activate_action_at(self.root.as_mut(), &path),
            None => EventResult::Ignored,
        }
    }

    /// Key released. Delivered to the focused widget.
    pub fn on_key_up(&mut self, key: Key, mods: Modifiers) {
        let event = Event::KeyUp {
            key,
            modifiers: mods,
        };
        if let Some(path) = self.focus.clone() {
            dispatch_event(&mut self.root, &path, &event, Point::ORIGIN);
        }
    }
}
