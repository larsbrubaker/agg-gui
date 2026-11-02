//! Running the history (`mod.rs`) against a browser: undo and redo, the notice an undo or redo
//! ends with, listing the browser's folders again after it, the keys that run them, and the
//! browser's own recorded writes.
//!
//! An undo or redo takes the latest step off its list and runs it, with neither the history nor
//! the browser's state borrowed, so a step may finish at once or later. When it finishes, the
//! notice says "Undone: ..." or "Redone: ..." (and "Couldn't put back: ..." naming what failed),
//! and the writable folders on the path of the browser it was started from list themselves again
//! (`after_write`, so a folder tree beside it lists again too); a read-only folder can't have
//! changed. A browser closed meanwhile is left alone.
//!
//! The keys, taken by the list and the tree while they have the keyboard: Command+Z or Control+Z
//! undoes, Command+Shift+Z, Control+Shift+Z, Command+Y or Control+Y redoes. They are taken even
//! with nothing to undo, so they never reach an app's own undo (a design's) while the browser has
//! the keyboard.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use agg_gui::{EventResult, Key, Modifiers};
use agg_gui_files::{tr, HistoryStep, Recorded, StepOutcome, WritableFolder};

use super::BrowserHistory;
use crate::state::BrowserState;

impl BrowserHistory {
    /// Takes back the latest step, then lists `state`'s folders again once it has finished.
    /// False when there was none, or another undo or redo is still finishing.
    pub fn undo(self: &Rc<Self>, state: &Rc<RefCell<BrowserState>>) -> bool {
        self.run(state, true)
    }

    /// Replays the latest step undone, then lists `state`'s folders again once it has finished.
    /// False when there was none, or another undo or redo is still finishing.
    pub fn redo(self: &Rc<Self>, state: &Rc<RefCell<BrowserState>>) -> bool {
        self.run(state, false)
    }

    fn run(self: &Rc<Self>, state: &Rc<RefCell<BrowserState>>, undo: bool) -> bool {
        let Some(step) = self.start(undo) else {
            return false;
        };
        crate::trace(|| {
            let what = if undo { "undo" } else { "redo" };
            format!("history {what} starts: {}", step.description())
        });
        let (history, state) = (Rc::downgrade(self), Rc::downgrade(state));
        let done = Box::new(move |outcome: StepOutcome| finish(&history, &state, outcome, undo));
        match undo {
            true => step.undo(done),
            false => step.redo(done),
        }
        true
    }
}

/// What the notice says after an undo or redo: what came back, then what failed.
fn finish_text(kept: Option<&dyn HistoryStep>, failures: &[String], undo: bool) -> String {
    let done = if undo { tr("Undone") } else { tr("Redone") };
    let mut text = match kept {
        Some(step) => format!("{done}: {}", step.description()),
        None => String::new(),
    };
    if !failures.is_empty() {
        if !text.is_empty() {
            text.push_str(". ");
        }
        text.push_str(&format!(
            "{}: {}",
            tr("Couldn't put back"),
            failures.join(", ")
        ));
    }
    text
}

/// The end of an undo or redo (see the module header).
fn finish(
    history: &Weak<BrowserHistory>,
    state: &Weak<RefCell<BrowserState>>,
    outcome: StepOutcome,
    undo: bool,
) {
    let StepOutcome { kept, failures } = outcome;
    let text = finish_text(kept.as_deref(), &failures, undo);
    if let Some(history) = history.upgrade() {
        history.land(kept, text, undo);
    }
    if let Some(state) = state.upgrade() {
        relist(&state);
    }
}

/// Lists every writable folder on the browser's path again and shows the listing (a listing that
/// arrives later shows itself).
fn relist(state: &Rc<RefCell<BrowserState>>) {
    let mut state = state.borrow_mut();
    let depth = state.depth();
    for d in 0..depth {
        if let Some(mut folder) = state.container_at_mut(d) {
            if folder.as_writable().is_some() {
                folder.load();
            }
        }
    }
    state.after_write(String::new());
}

/// The history keys (see the module header) for the browser `state`; `None` for any other key.
pub fn history_key_event(
    state: &Rc<RefCell<BrowserState>>,
    key: &Key,
    modifiers: Modifiers,
) -> Option<EventResult> {
    if !(modifiers.ctrl || modifiers.meta) {
        return None;
    }
    let redo = match key {
        Key::Char(c) if c.eq_ignore_ascii_case(&'z') => modifiers.shift,
        Key::Char(c) if c.eq_ignore_ascii_case(&'y') => true,
        _ => return None,
    };
    match redo {
        true => BrowserState::redo(state),
        false => BrowserState::undo(state),
    };
    Some(EventResult::Consumed)
}

impl BrowserState {
    /// The browser's history (`BrowserConfig::history`), shared with every view given it.
    pub fn history(&self) -> Rc<BrowserHistory> {
        Rc::clone(&self.config().history)
    }

    /// Undoes the latest step of `state`'s history, listing `state` again after it.
    pub fn undo(state: &Rc<RefCell<Self>>) -> bool {
        let history = state.borrow().history();
        history.undo(state)
    }

    /// Redoes the latest step undone in `state`'s history, listing `state` again after it.
    pub fn redo(state: &Rc<RefCell<Self>>) -> bool {
        let history = state.borrow().history();
        history.redo(state)
    }

    /// Runs `write` on the folder at `depth` (0 = the root) of the open path, records the step
    /// it hands back in the history, and lists the browser again (`after_write`), its failure on
    /// the status line. A folder that takes no changes refuses with "This folder can't be
    /// changed". Answers the write's result.
    pub fn write_recorded(
        state: &Rc<RefCell<Self>>,
        depth: usize,
        write: impl FnOnce(&mut dyn WritableFolder) -> Recorded,
    ) -> Result<(), String> {
        let recorded = {
            let mut s = state.borrow_mut();
            let writable = s
                .container_at_mut(depth)
                .map(|mut folder| folder.as_writable_mut().map(write));
            match writable.flatten() {
                Some(recorded) => recorded,
                None => Recorded::unrecorded(Err(tr("This folder can't be changed"))),
            }
        };
        let history = state.borrow().history();
        let result = history.keep(recorded);
        let status = result.clone().err().unwrap_or_default();
        state.borrow_mut().after_write(status);
        result
    }
}
