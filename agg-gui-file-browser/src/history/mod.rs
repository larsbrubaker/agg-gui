//! The browser's undo history: [`BrowserHistory`], one list of recorded changes
//! ([`agg_gui_files::HistoryStep`]s) shared by every browser view an app gives it to
//! (`BrowserConfig::history`), so a change made in one view (a dialog's Move, a popup's drag) can
//! be taken back from another. Each user action is one step, however many items it touched. A
//! new step empties the redo list.
//!
//! Steps arrive two ways. A write the browser runs itself hands its step back at once
//! ([`agg_gui_files::Recorded`], kept by [`BrowserHistory::keep`] or
//! `BrowserState::write_recorded`). A change whose outcome comes later (a server has to answer
//! before anything is known to have moved) is recorded when it lands, through a
//! [`HistoryRecorder`]: a weak handle the app's code takes now and records through from its
//! callback, so the history never waits on a write and a browser closed meanwhile is never kept
//! alive. A step recorded later goes on top of the history when it lands, like any new step.
//!
//! An undo or redo may also finish later (`HistoryStep::undo` hands its outcome to a callback).
//! Until it has, the history is busy: `can_undo` and `can_redo` answer false and the keys and
//! buttons wait rather than start another (`run.rs`). When it finishes, the step keeps only what
//! came back and goes on the other list (nothing goes when nothing came back), the notice says
//! what happened and names what failed, and the writable folders on the browser's path list
//! themselves again. A step recorded while one is still finishing is the newest action: an undo
//! landing after it leaves nothing to redo (the redo list stays empty after a new step, whatever
//! order things land in), and a redo landing after it goes below it. A step that lets its
//! callback go without calling it frees the history all the same, and the notice says the undo
//! (or redo) didn't finish (`run.rs`).
//!
//! Every change shows a [`Notice`] ("Moved 3 items to Brackets", with Undo; "Undone: ..." with
//! Redo) that the notice widget (`notice.rs`) reads, and bumps [`BrowserHistory::generation`],
//! which a view folds into its rebuild key so its Undo and Redo buttons follow a change made in
//! another view. The history holds no borrow while a step runs or a notice is read, so a step's
//! callback may record or read freely.

pub mod notice;
pub mod run;

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use agg_gui_files::{HistoryStep, Recorded};

/// Which button the notice offers: Undo after an action or a redo, Redo after an undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    Undo,
    Redo,
}

/// The notice announcing the latest change.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    /// What happened, for the user ("Moved 3 items to Brackets").
    pub text: String,
    /// The button it offers, when the step it names can still be taken back (or replayed).
    pub action: Option<NoticeAction>,
    /// Counts up with every notice, so the widget restarts its fade for a new one.
    pub id: u64,
    /// The UI clock's instant when it was shown (`agg_gui::clock::now`): its hold and fade count
    /// from here.
    pub shown_at: web_time::Instant,
}

#[derive(Default)]
struct Lists {
    undo: Vec<Box<dyn HistoryStep>>,
    redo: Vec<Box<dyn HistoryStep>>,
    notice: Option<Notice>,
    generation: u64,
    /// An undo or redo has started and not yet finished.
    busy: bool,
    /// How many steps the undo list held when the running undo or redo started.
    started_at: usize,
    /// A step was recorded while an undo or redo was finishing.
    recorded_while_busy: bool,
}

impl Lists {
    fn show(&mut self, text: String, action: Option<NoticeAction>) {
        let id = self.notice.as_ref().map_or(1, |n| n.id + 1);
        self.notice = Some(Notice {
            text,
            action,
            id,
            shown_at: agg_gui::clock::now(),
        });
        self.generation += 1;
        agg_gui::animation::request_draw();
    }

    fn push(&mut self, step: Box<dyn HistoryStep>, text: String) {
        self.recorded_while_busy |= self.busy;
        self.undo.push(step);
        self.redo.clear();
        self.show(text, Some(NoticeAction::Undo));
    }
}

/// The undo history (see the module header). Shared as an `Rc`; every method takes `&self`.
#[derive(Default)]
pub struct BrowserHistory {
    lists: RefCell<Lists>,
}

impl BrowserHistory {
    /// An empty history.
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Lists) -> R) -> R {
        f(&mut self.lists.borrow_mut())
    }

    /// Empties the history: no steps, no notice (an app starting a new session).
    pub fn clear(&self) {
        self.with(|h| {
            let generation = h.generation + 1;
            *h = Lists {
                generation,
                ..Lists::default()
            };
        });
    }

    /// Counts every change to the history (a step recorded, undone or redone, a notice shown,
    /// an undo or redo starting).
    pub fn generation(&self) -> u64 {
        self.with(|h| h.generation)
    }

    /// The notice announcing the latest change, if any.
    pub fn notice(&self) -> Option<Notice> {
        self.with(|h| h.notice.clone())
    }

    /// Whether there is a step to undo (none while an undo or redo is still finishing).
    pub fn can_undo(&self) -> bool {
        self.with(|h| !h.busy && !h.undo.is_empty())
    }

    /// Whether there is a step to redo (none while an undo or redo is still finishing).
    pub fn can_redo(&self) -> bool {
        self.with(|h| !h.busy && !h.redo.is_empty())
    }

    /// Whether an undo or redo has started and not yet finished.
    pub fn is_busy(&self) -> bool {
        self.with(|h| h.busy)
    }

    /// What the next undo takes back ("Moved 3 items to Brackets"): the Undo button's tooltip.
    pub fn undo_description(&self) -> Option<String> {
        self.with(|h| h.undo.last().map(|s| s.description()))
    }

    /// What the next redo replays: the Redo button's tooltip.
    pub fn redo_description(&self) -> Option<String> {
        self.with(|h| h.redo.last().map(|s| s.description()))
    }

    /// How many steps there are to undo and to redo.
    pub fn depths(&self) -> (usize, usize) {
        self.with(|h| (h.undo.len(), h.redo.len()))
    }

    /// Records a step the user just took, announces it, and empties the redo list.
    pub fn record(&self, step: Box<dyn HistoryStep>) {
        self.record_with_note(step, None);
    }

    /// [`Self::record`], the notice adding `note` after the step's description (what part of
    /// the action did not happen).
    pub fn record_with_note(&self, step: Box<dyn HistoryStep>, note: Option<String>) {
        let mut text = step.description();
        if let Some(note) = note {
            text.push_str(". ");
            text.push_str(&note);
        }
        self.with(|h| h.push(step, text));
    }

    /// [`Self::record`], the notice saying `text` in place of the step's description (a copy
    /// made because a move couldn't happen says why), still with Undo.
    pub fn record_saying(&self, step: Box<dyn HistoryStep>, text: String) {
        self.with(|h| h.push(step, text));
    }

    /// Shows `text` in the notice with no button: an action that changed nothing to take back.
    pub fn announce(&self, text: String) {
        self.with(|h| h.show(text, None));
    }

    /// Records `recorded`'s step, when the write left one, and answers its result.
    pub fn keep(&self, recorded: Recorded) -> Result<(), String> {
        if let Some(step) = recorded.step {
            self.record(step);
        }
        recorded.result
    }

    /// A handle to record into this history later, from a callback (see the module header).
    pub fn recorder(self: &Rc<Self>) -> HistoryRecorder {
        HistoryRecorder {
            history: Rc::downgrade(self),
        }
    }

    /// Takes the latest step off the undo list (or the redo list), marking the history busy
    /// until it finishes; `None` when there is none or another is still finishing.
    fn start(&self, undo: bool) -> Option<Box<dyn HistoryStep>> {
        self.with(|h| {
            if h.busy {
                return None;
            }
            let step = match undo {
                true => h.undo.pop(),
                false => h.redo.pop(),
            }?;
            h.busy = true;
            h.started_at = h.undo.len();
            h.recorded_while_busy = false;
            h.generation += 1;
            Some(step)
        })
    }

    /// The end of an undo or redo: `kept` goes on the other list and `text` shows, with the
    /// button for `kept` when there is one.
    /// A step recorded meanwhile (see the module header) makes an undone step stale, and puts a
    /// redone one below it.
    fn land(&self, kept: Option<Box<dyn HistoryStep>>, text: String, undo: bool) {
        self.with(|h| {
            h.busy = false;
            let newer = std::mem::take(&mut h.recorded_while_busy);
            let action = match undo {
                true => NoticeAction::Redo,
                false => NoticeAction::Undo,
            };
            let kept = kept.filter(|_| !(undo && newer));
            let offered = kept.is_some().then_some(action);
            if let Some(step) = kept {
                match (undo, newer) {
                    (true, _) => h.redo.push(step),
                    (false, false) => h.undo.push(step),
                    (false, true) => {
                        let at = h.started_at.min(h.undo.len());
                        h.undo.insert(at, step);
                    }
                }
            }
            h.show(text, offered);
        });
    }
}

/// A weak handle on a [`BrowserHistory`] for recording a change whose outcome arrives later (a
/// server move recorded once the server has answered). Recording through it after the history
/// is gone does nothing.
#[derive(Clone)]
pub struct HistoryRecorder {
    history: Weak<BrowserHistory>,
}

impl HistoryRecorder {
    /// [`BrowserHistory::record`], if the history still exists.
    pub fn record(&self, step: Box<dyn HistoryStep>) {
        if let Some(history) = self.history.upgrade() {
            history.record(step);
        }
    }

    /// [`BrowserHistory::record_with_note`], if the history still exists.
    pub fn record_with_note(&self, step: Box<dyn HistoryStep>, note: Option<String>) {
        if let Some(history) = self.history.upgrade() {
            history.record_with_note(step, note);
        }
    }

    /// [`BrowserHistory::record_saying`], if the history still exists.
    pub fn record_saying(&self, step: Box<dyn HistoryStep>, text: String) {
        if let Some(history) = self.history.upgrade() {
            history.record_saying(step, text);
        }
    }

    /// [`BrowserHistory::announce`], if the history still exists.
    pub fn announce(&self, text: String) {
        if let Some(history) = self.history.upgrade() {
            history.announce(text);
        }
    }

    /// [`BrowserHistory::keep`]: records `recorded`'s step if the history still exists, and
    /// answers its result either way.
    pub fn keep(&self, recorded: Recorded) -> Result<(), String> {
        match self.history.upgrade() {
            Some(history) => history.keep(recorded),
            None => recorded.result,
        }
    }
}
