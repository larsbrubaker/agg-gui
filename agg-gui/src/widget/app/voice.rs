//! Voice input on [`App`]: starts the on-screen keyboard's dictation with
//! the focused field's context, delivers its text to the focused widget,
//! ends a dictation before focus leaves its field or another key or text
//! arrives, and lets it follow a field the app rebuilds in place
//! (`widgets/on_screen_keyboard/voice_session.rs`).
//!
//! Dictated text takes the input-method path (`ime.rs`): committed words
//! arrive as [`Event::TextInput`](crate::Event::TextInput) and the
//! unconfirmed tail as [`Event::ImePreedit`](crate::Event::ImePreedit), so
//! every widget that takes IME input takes dictation, with no change of its
//! own. Polled once a frame from `App::paint` and after the keyboard's
//! synthetic keys are drained (`keyboard.rs`).

use super::tree_paths::{widget_at_path, widget_at_path_ref};
use crate::event::{Event, Key};
use crate::widget::{App, Widget};
use crate::widgets::on_screen_keyboard::voice_session::{self, FieldMark, VoiceDelivery};

impl App {
    /// Start a dictation a tap asked for, poll the voice provider, and
    /// deliver its text to the focused widget.
    pub(super) fn pump_voice_input(&mut self) {
        if voice_session::take_start_request() {
            let before = self.focused_text_before_caret();
            voice_session::begin(&before);
            voice_session::note_field(self.focused_field_mark());
        }
        voice_session::poll();
        self.deliver_voice_text();
    }

    /// End a dictation where it stands, before focus leaves its field or a
    /// typed key or platform text reaches it: deliver what the provider has,
    /// end the dictation (its shown tail committed to this field), and
    /// deliver that too. Drains queued text even when nothing is dictating.
    pub(super) fn end_voice_input_keeping_tail(&mut self) {
        if crate::widgets::on_screen_keyboard::is_dictating() {
            self.pump_voice_input();
            voice_session::end_keeping_tail();
        }
        self.deliver_voice_text();
    }

    /// A key pressed on a hardware keyboard (or synthesized by the on-screen
    /// one) ends a dictation first, its tail committed before the key, as
    /// on the phone. A modifier alone doesn't.
    pub(super) fn end_voice_input_for_key(&mut self, key: &Key) {
        if !is_modifier_only(key) {
            self.end_voice_input_keeping_tail();
        }
    }

    /// Deliver everything the dictation queued, in order, to the focused
    /// widget, and remember the field as it leaves it.
    pub(super) fn deliver_voice_text(&mut self) {
        if !voice_session::has_deliveries() {
            return;
        }
        // Settle a replaced field first, so a dictation that can't follow it
        // is abandoned (its queue with it) before its text is taken.
        self.resolve_tracked_paths();
        let deliveries = voice_session::take_deliveries();
        for delivery in deliveries {
            // Straight to the widget: the public `on_text_input` /
            // `on_ime_preedit` are the platform's, which end a dictation.
            let event = match delivery {
                VoiceDelivery::Commit(text) => Event::TextInput { text },
                VoiceDelivery::Preedit(text) => {
                    let end = text.len();
                    Event::ImePreedit {
                        text,
                        cursor: Some((end, end)),
                    }
                }
            };
            self.dispatch_text_event(event);
        }
        if crate::widgets::on_screen_keyboard::is_dictating() {
            voice_session::note_field(self.focused_field_mark());
        }
    }

    /// The app replaced the focused widget and focus followed the one now at
    /// `path` (`focusable` when there is one): the dictation goes on into
    /// the same field rebuilt, which takes over its caret and shown tail at
    /// once ([`Widget::resume_text_input`]), and is abandoned for anything
    /// else ([`voice_session::follow_plan`]).
    pub(super) fn follow_voice_into_replacement(&mut self, path: &[usize], focusable: bool) {
        if !crate::widgets::on_screen_keyboard::is_dictating() {
            return;
        }
        let plan = widget_at_path_ref(self.root.as_ref(), path)
            .filter(|w| focusable && w.accepts_text_input())
            .and_then(|w| {
                let text = w.text_input_value()?;
                voice_session::follow_plan(kind_of(w), w.focus_id(), &text)
            });
        let resumed = plan.is_some_and(|(caret, tail, in_text)| {
            widget_at_path(&mut self.root, path)
                .is_some_and(|w| w.resume_text_input(caret, &tail, in_text))
        });
        if resumed {
            voice_session::note_field(self.focused_field_mark());
        } else {
            voice_session::abandon();
        }
    }

    /// The focused text widget as a dictation leaves it: its kind, id,
    /// committed text and caret.
    fn focused_field_mark(&self) -> Option<FieldMark> {
        let w = widget_at_path_ref(self.root.as_ref(), self.focus.as_deref()?)?;
        let text = w.text_input_value()?;
        let before = w.text_before_caret();
        let caret = match before {
            Some(before) if text.starts_with(&before) => before.len(),
            _ => text.len(),
        };
        Some(FieldMark {
            kind: kind_of(w),
            focus_id: w.focus_id(),
            text,
            caret,
        })
    }

    /// The focused widget's text before its caret, or all its text when it
    /// can't say where its caret is; empty without a text widget.
    fn focused_text_before_caret(&self) -> String {
        self.focus
            .as_deref()
            .and_then(|path| widget_at_path_ref(self.root.as_ref(), path))
            .and_then(|w| w.text_before_caret().or_else(|| w.text_input_value()))
            .unwrap_or_default()
    }
}

/// A widget's concrete type, as far as it can say, and its type name.
fn kind_of(w: &dyn Widget) -> (Option<std::any::TypeId>, &'static str) {
    (w.as_any().map(|a| a.type_id()), w.type_name())
}

/// A key that only modifies others (Shift, Control, …), which the shells
/// report as `Key::Other` with the platform's name for it.
fn is_modifier_only(key: &Key) -> bool {
    const MODIFIERS: &[&str] = &[
        "Shift",
        "Control",
        "Alt",
        "AltGraph",
        "Meta",
        "Super",
        "Hyper",
        "Fn",
        "FnLock",
        "CapsLock",
        "NumLock",
        "ScrollLock",
        "Symbol",
        "SymbolLock",
    ];
    matches!(key, Key::Other(name) if MODIFIERS.contains(&name.as_str()))
}
