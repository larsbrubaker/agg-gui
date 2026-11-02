//! Recorded changes: a [`HistoryStep`] takes back (or replays) one change a writable folder made
//! (`writable`'s `*_recorded` methods hand it back in a [`Recorded`]), for a browser's undo stack.
//! A step may finish later than the call that runs it (a server has to answer), so undo and redo
//! hand their [`StepOutcome`] to a callback rather than returning it; a step that finishes at
//! once calls it before returning. Steps on disk and steps an app adds (cloud folders) are the
//! same trait, so one stack holds both.

/// One recorded change, able to take itself back and to replay itself.
///
/// `undo` and `redo` must call their `done` exactly once, now or later, whatever happens: the
/// history is busy (it starts no other undo or redo) until `done` is called. A step that fails
/// calls it with nothing kept and the failures named. A callback dropped uncalled is a bug in the
/// step; a browser's history guards against it (it frees itself and says the undo didn't
/// finish), but anything else waiting on the outcome never hears it.
pub trait HistoryStep {
    /// What the change was, for the user ("Moved 3 items to Brackets"), already translated.
    fn description(&self) -> String;
    /// Takes the change back, now or later; `done` gets the step that replays it.
    fn undo(self: Box<Self>, done: StepDone);
    /// Makes the change again, now or later; `done` gets the step that takes it back.
    fn redo(self: Box<Self>, done: StepDone);
}

/// What an undo or redo did: the step with only what came back (`None` when nothing did, so
/// nothing is left to take back or replay), and a line per item that could not, naming it and
/// why.
#[must_use = "an outcome's kept step goes back on the history and its failures to the user"]
pub struct StepOutcome {
    pub kept: Option<Box<dyn HistoryStep>>,
    pub failures: Vec<String>,
}

/// Takes an undo's or redo's outcome once it has finished; called exactly once (see
/// [`HistoryStep`]).
pub type StepDone = Box<dyn FnOnce(StepOutcome)>;

/// A change's result, and the step that takes it back when the folder recorded one. A change
/// that partly failed still hands back the step for the part that landed.
#[must_use = "a recorded change carries its failure and the step that undoes it"]
pub struct Recorded {
    pub result: Result<(), String>,
    pub step: Option<Box<dyn HistoryStep>>,
}

impl Recorded {
    /// A change nothing records: `result` and no step.
    pub fn unrecorded(result: Result<(), String>) -> Self {
        Self { result, step: None }
    }
}
