//! The caret operations a keyboard caret gesture (`cursor_slide.rs`) asks the
//! App to run against the focused field, and the answers that come back.
//!
//! A gesture can't see the field, so, like a typed key (`events.rs`), it
//! queues what it wants and the [`App`](crate::widget::App) runs the queue
//! after the event or at the start of the frame (`widget/app/keyboard_caret.rs`).
//! Unlike a typed key, a gesture needs to know what happened: whether a step
//! moved the caret (a haptic tick, a word counted) and the selection it
//! started from (put back when the gesture is taken back or cancelled). The
//! App reports each step with [`report_step`] and the starting selection
//! with [`record_snapshot`]; both reach the gesture before its next move, as
//! the App runs the queue after every event. A step that moved the caret is
//! also what gives a haptic tick ([`on_haptic_tick`]).
//!
//! A widget that can't report and put back its selection
//! ([`Widget::text_selection`](crate::widget::Widget::text_selection)) leaves
//! the snapshot empty; the gesture then gives words back with the inverse
//! keys and a cancel deletes nothing, without restoring.

use std::cell::{Cell, RefCell};

use crate::event::{Key, Modifiers};
use crate::widget::TextSelection;

/// What a gesture step is, which decides what the App does with it and what
/// its answer means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepKind {
    /// A character step: a plain Left or Right.
    Char,
    /// A line step: Up or Down, sent only to a multi-line field.
    Line,
    /// A word the Backspace slide selects.
    SelectWord,
    /// A word the Backspace slide selects again after giving words back
    /// (no haptic tick of its own: the give-back ticked once).
    ReselectWord,
    /// A word the Backspace slide gives back with the inverse key, for a
    /// widget that can't put its selection back.
    GiveBackWord,
}

/// One operation on the focused field.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CaretOp {
    /// Remember the field's selection, to put back with [`CaretOp::Restore`].
    Snapshot,
    /// Send `key` with `mods` and report whether the caret moved.
    Step {
        key: Key,
        mods: Modifiers,
        kind: StepKind,
    },
    /// Put back the selection [`CaretOp::Snapshot`] remembered; for a widget
    /// that can't, give `give_back` selected words back with the inverse key.
    Restore { give_back: u32 },
    /// Delete the selection: the Backspace slide's lift.
    DeleteSelection,
    /// The gesture is over: forget the snapshot and the widget it was taken
    /// in.
    EndGesture,
}

thread_local! {
    static QUEUE: RefCell<Vec<CaretOp>> = const { RefCell::new(Vec::new()) };
    static SNAPSHOT: Cell<Option<TextSelection>> = const { Cell::new(None) };
    static OWNER: RefCell<Option<Vec<usize>>> = const { RefCell::new(None) };
}

/// Shift with the platform's word modifier (Option on a Mac's bindings,
/// Control elsewhere): a word-wise selecting arrow in every text widget.
pub(crate) fn word_select_modifiers() -> Modifiers {
    let mut mods = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    if crate::platform::use_mac_key_bindings() {
        mods.alt = true;
    } else {
        mods.ctrl = true;
    }
    mods
}

/// The key that gives one selected word back (Shift + word modifier +
/// Right), for a widget that can't put its selection back.
pub(crate) fn give_back_key() -> (Key, Modifiers) {
    (Key::ArrowRight, word_select_modifiers())
}

pub(crate) fn push(op: CaretOp) {
    QUEUE.with(|q| q.borrow_mut().push(op));
}

/// Take every queued operation, in order. Called by the App.
pub(crate) fn drain() -> Vec<CaretOp> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}

/// The App ran a [`CaretOp::Snapshot`] (or a [`CaretOp::EndGesture`]): the
/// field's selection, or `None` when the focused widget can't report it and
/// put it back.
pub(crate) fn record_snapshot(selection: Option<TextSelection>) {
    SNAPSHOT.with(|s| s.set(selection));
}

/// The selection the gesture's [`CaretOp::Snapshot`] remembered; `None`
/// before it ran, after the gesture ended, or for a widget that can't put
/// its selection back.
pub(crate) fn snapshot() -> Option<TextSelection> {
    SNAPSHOT.with(|s| s.get())
}

/// The App ran a [`CaretOp::Snapshot`]: the focus path of the widget it
/// was taken in, which alone the gesture's steps, restore and delete reach
/// (`None` when the gesture ends).
pub(crate) fn record_owner(path: Option<Vec<usize>>) {
    OWNER.with(|o| *o.borrow_mut() = path);
}

/// The focus path of the widget the gesture's snapshot was taken in.
pub(crate) fn owner() -> Option<Vec<usize>> {
    OWNER.with(|o| o.borrow().clone())
}

/// The App ran a [`CaretOp::Step`] of `kind`: whether the caret moved.
pub(crate) fn report_step(kind: StepKind, moved: bool) {
    super::cursor_slide::step_result(kind, moved);
}

type TickListener = Box<dyn Fn()>;

thread_local! {
    static TICK_LISTENERS: RefCell<Vec<TickListener>> = RefCell::new(Vec::new());
}

/// Call `listener` for every step of a keyboard caret gesture that moved the
/// caret (or selected a word): the platform shell turns it into a haptic
/// tick (the web shell vibrates where the browser can). A step that went
/// nowhere (at the end of the text, a line step in a one-line field) ticks
/// nothing.
pub fn on_haptic_tick(listener: impl Fn() + 'static) {
    TICK_LISTENERS.with(|l| l.borrow_mut().push(Box::new(listener)));
}

pub(super) fn notify_ticks(steps: u32) {
    if steps == 0 {
        return;
    }
    // Taken out while they run, so a listener may register another.
    let listeners = TICK_LISTENERS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    for _ in 0..steps {
        for listener in &listeners {
            listener();
        }
    }
    TICK_LISTENERS.with(|l| {
        let mut current = l.borrow_mut();
        let added = std::mem::take(&mut *current);
        *current = listeners;
        current.extend(added);
    });
}

#[cfg(test)]
pub(super) fn clear_listeners() {
    TICK_LISTENERS.with(|l| l.borrow_mut().clear());
}

#[cfg(test)]
pub(super) fn clear() {
    QUEUE.with(|q| q.borrow_mut().clear());
    SNAPSHOT.with(|s| s.set(None));
    OWNER.with(|o| *o.borrow_mut() = None);
}
