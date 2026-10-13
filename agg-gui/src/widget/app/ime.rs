//! Text-input (IME) methods on [`App`]: committed text and composition
//! (preedit) text routed to the focused widget, and the caret rectangle a
//! shell places the OS candidate window at. Sits beside `keyboard.rs`, whose
//! key routing it mirrors (modal subtree first, then the focus path) minus the
//! accelerator and default-action fallbacks, which only make sense for keys.

use super::tree_paths::widget_at_path_ref;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widget::keyboard_scroll::{current_lift, widget_rect_in_root};
use crate::widget::tree::{active_modal_path, dispatch_event};
use crate::widget::App;

impl App {
    /// Text the platform committed for the focused widget (an IME finished a
    /// composition, dictation, an emoji picker). Delivered as
    /// [`Event::TextInput`]; returns whether a widget consumed it. Empty text
    /// is ignored.
    pub fn on_text_input(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.dispatch_text_event(Event::TextInput {
            text: text.to_string(),
        })
    }

    /// The input method's composition text changed. Delivered as
    /// [`Event::ImePreedit`]; empty `text` ends the composition. `cursor` is
    /// a byte range inside `text`, or `None` for no caret. Returns whether a
    /// widget consumed it.
    pub fn on_ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        self.dispatch_text_event(Event::ImePreedit {
            text: text.to_string(),
            cursor,
        })
    }

    /// Whether the OS input method should be active: the focused widget
    /// accepts typed text. Shells enable IME (winit `set_ime_allowed`, the
    /// web shell's hidden input) exactly while this is true.
    pub fn wants_ime(&self) -> bool {
        self.focused_is_text_input()
    }

    /// Where the OS candidate window belongs: the caret rectangle of the
    /// focused text widget (its [`Widget::ime_cursor_area`], or the whole
    /// widget when it names none), in **physical pixels, Y-down**, the space
    /// the pointer entry points take. `None` when no text widget has focus.
    pub fn ime_cursor_area(&self) -> Option<Rect> {
        let path = self.focus.as_deref()?;
        let widget = widget_at_path_ref(self.root.as_ref(), path)?;
        if !widget.accepts_text_input() {
            return None;
        }
        let rect = widget_rect_in_root(self.root.as_ref(), path, |w| {
            let b = w.bounds();
            match w.ime_cursor_area() {
                Some(a) => Rect::new(b.x + a.x, b.y + a.y, a.width, a.height),
                None => b,
            }
        })?;
        // Root logical Y-up -> on-screen (add the keyboard lift back), then
        // flip to Y-down and scale to physical pixels like `flip_y` inverts.
        let scale = crate::ux_scale::effective_scale().max(1e-6);
        let top_up = rect.y + rect.height + current_lift();
        let y_down = (self.viewport_height - top_up) * scale;
        Some(Rect::new(
            rect.x * scale,
            y_down,
            rect.width * scale,
            rect.height * scale,
        ))
    }

    /// Route a text event like a key: to a focused widget inside the active
    /// modal, else the modal subtree, else the focused widget.
    fn dispatch_text_event(&mut self, event: Event) -> bool {
        self.resolve_tracked_paths();
        let target = match active_modal_path(self.root.as_ref()) {
            Some(path) => match self.focus.clone() {
                Some(focus) if focus.starts_with(&path) => Some(focus),
                _ => Some(path),
            },
            None => self.focus.clone(),
        };
        let Some(target) = target else {
            return false;
        };
        let result = dispatch_event(&mut self.root, &target, &event, Point::ORIGIN);
        matches!(result, EventResult::Consumed | EventResult::ConsumedQuiet)
    }
}
