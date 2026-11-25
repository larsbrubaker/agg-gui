//! Text-input (IME) methods on [`App`]: committed text and composition
//! (preedit) text routed to the focused widget (or the active modal layer,
//! which may be taking text itself, an open menu's search field), whether the
//! input method is wanted, and the caret rectangle a
//! shell places the OS candidate window at. Sits beside `keyboard.rs`, whose
//! key routing it mirrors (modal subtree first, then the focus path) minus the
//! accelerator and default-action fallbacks, which only make sense for keys.

use std::cell::Cell;

use super::tree_paths::widget_at_path_ref;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widget::keyboard_scroll::{current_lift, widget_rect_in_root};
use crate::widget::tree::{active_modal_path, dispatch_event};
use crate::widget::App;

thread_local! {
    /// Whether the active modal layer's text entry had the focus at the last
    /// layout ([`App::sync_modal_text_entry`]), per thread as the on-screen
    /// keyboard's state is.
    static MODAL_TEXT_ENTRY: Cell<bool> = const { Cell::new(false) };
}

impl App {
    /// Text the platform committed for the focused widget (an IME finished a
    /// composition, dictation, an emoji picker). Delivered as
    /// [`Event::TextInput`]; returns whether a widget consumed it. Empty text
    /// is ignored.
    pub fn on_text_input(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        crate::animation::note_user_input();
        // Text from the platform ends a dictation first, its tail committed.
        self.end_voice_input_keeping_tail();
        self.dispatch_text_event(Event::TextInput {
            text: text.to_string(),
        })
    }

    /// The input method's composition text changed. Delivered as
    /// [`Event::ImePreedit`]; empty `text` ends the composition. `cursor` is
    /// a byte range inside `text`, or `None` for no caret. Returns whether a
    /// widget consumed it.
    pub fn on_ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        // An empty preedit is input only when it ends a composition that was
        // showing: shells also send one when the input method is switched off
        // (focus leaving a text field), which the user didn't type.
        let was_showing = std::mem::replace(&mut self.platform_preedit_showing, !text.is_empty());
        if !text.is_empty() || was_showing {
            crate::animation::note_user_input();
        }
        if !text.is_empty() {
            // The platform's input method composing ends a dictation first.
            self.end_voice_input_keeping_tail();
        }
        self.dispatch_text_event(Event::ImePreedit {
            text: text.to_string(),
            cursor,
        })
    }

    /// Whether the OS input method should be active: the focused widget
    /// accepts typed text, or the active modal layer is taking typed text
    /// ([`crate::Widget::modal_text_input_active`], an open menu's search
    /// field). Shells enable IME (winit `set_ime_allowed`, the web shell's
    /// hidden input) exactly while this is true.
    pub fn wants_ime(&self) -> bool {
        self.focused_is_text_input() || self.modal_text_entry_active()
    }

    /// Whether the active modal layer is taking typed text itself
    /// ([`crate::Widget::modal_text_input_active`], an open menu's search
    /// field).
    fn modal_text_entry_active(&self) -> bool {
        active_modal_path(self.root.as_ref())
            .and_then(|path| widget_at_path_ref(self.root.as_ref(), &path))
            .is_some_and(|w| w.modal_text_input_active())
    }

    /// Raises the on-screen keyboard while the active modal layer's text
    /// entry has the menu's focus, and lowers it when that ends, as a focus
    /// change into or out of a text field does
    /// (`keyboard_scroll::notify_focus_change`). A focused text field
    /// already holds the keyboard up, so then this leaves it alone. Runs at
    /// the end of each layout: a menu's focus moves inside the menu, not
    /// through the App's focus.
    pub(super) fn sync_modal_text_entry(&mut self) {
        let active = self.modal_text_entry_active();
        if MODAL_TEXT_ENTRY.with(|c| c.replace(active)) == active || self.focused_is_text_input() {
            return;
        }
        use crate::widgets::on_screen_keyboard::{
            set_text_input_focused_with_hints, EnterAction, KeyboardInputMode, TextInputHints,
        };
        // A menu's text entry is a search: lower case, Enter picks a result.
        let hints = TextInputHints {
            enter_action: EnterAction::Search,
            auto_capitalize: false,
            ..TextInputHints::default()
        };
        set_text_input_focused_with_hints(active, None, KeyboardInputMode::Text, hints);
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
    pub(super) fn dispatch_text_event(&mut self, event: Event) -> bool {
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
