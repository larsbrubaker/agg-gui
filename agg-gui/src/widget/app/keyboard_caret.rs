//! Running the on-screen keyboard's caret operations on [`App`]: the steps,
//! snapshot, restore and delete a caret gesture (the space-bar slide, the
//! Backspace slide, the iPhone trackpad) queues in
//! `widgets/on_screen_keyboard/caret_ops.rs`, run against the focused widget
//! with the answers the gesture needs.
//!
//! A step is an arrow key through [`App::on_key_down`], like any key, so
//! every text widget follows; the App compares the focused widget's
//! [`Widget::text_selection`] before and after to answer whether it moved
//! the caret. A line step reaches only a multi-line widget
//! ([`Widget::is_multi_line_text`]). Split out of `keyboard.rs` beside the
//! typed-key drain it runs with.

use super::tree_paths::{widget_at_path, widget_at_path_ref};
use crate::event::{Key, Modifiers};
use crate::widget::{App, Widget};
use crate::widgets::on_screen_keyboard::caret_ops::{self, CaretOp, StepKind};

impl App {
    /// Run the keyboard's queued caret operations, then its typed keys, in
    /// the focused widget. A dictation's committed text goes first: a
    /// gesture that ended the dictation snapshots the field after its words.
    pub(super) fn dispatch_keyboard_queue(&mut self) {
        self.deliver_voice_text();
        for op in caret_ops::drain() {
            self.run_caret_op(op);
        }
        for (key, mods) in crate::widgets::on_screen_keyboard::drain_synthetic_keys() {
            self.on_key_down(key, mods);
        }
    }

    fn run_caret_op(&mut self, op: CaretOp) {
        // The focus path may name a widget the app has since replaced.
        self.resolve_tracked_paths();
        match op {
            CaretOp::Snapshot => {
                // Only a selection the widget can also put back is kept
                // (setting it to itself is the proof); otherwise the gesture
                // falls back to the inverse keys.
                let restorable = self.focused_widget_mut().and_then(|w| {
                    let selection = w.text_selection()?;
                    w.set_text_selection(selection).then_some(selection)
                });
                caret_ops::record_snapshot(restorable);
            }
            CaretOp::Restore { give_back } => match caret_ops::snapshot() {
                Some(selection) => {
                    if let Some(w) = self.focused_widget_mut() {
                        w.set_text_selection(selection);
                    }
                }
                None => {
                    let (key, mods) = caret_ops::give_back_key();
                    for _ in 0..give_back {
                        self.on_key_down(key.clone(), mods);
                    }
                }
            },
            CaretOp::DeleteSelection => self.on_key_down(Key::Backspace, Modifiers::default()),
            CaretOp::EndGesture => caret_ops::record_snapshot(None),
            CaretOp::Step { key, mods, kind } => {
                let moved = self.run_step(key, mods, kind);
                caret_ops::report_step(kind, moved);
            }
        }
    }

    /// Send one step's key; whether it moved the caret. A widget that can't
    /// report its selection is taken at its word.
    fn run_step(&mut self, key: Key, mods: Modifiers, kind: StepKind) -> bool {
        let multi_line = self
            .focused_widget()
            .is_some_and(|w| w.is_multi_line_text());
        if kind == StepKind::Line && !multi_line {
            // A one-line field may take Up / Down for something else.
            return false;
        }
        let before = self.focused_widget().and_then(|w| w.text_selection());
        self.on_key_down(key, mods);
        let after = self.focused_widget().and_then(|w| w.text_selection());
        before.is_none() || before != after
    }

    fn focused_widget(&self) -> Option<&dyn Widget> {
        widget_at_path_ref(self.root.as_ref(), self.focus.as_deref()?)
    }

    fn focused_widget_mut(&mut self) -> Option<&mut dyn Widget> {
        let path = self.focus.clone()?;
        widget_at_path(&mut self.root, &path)
    }
}
