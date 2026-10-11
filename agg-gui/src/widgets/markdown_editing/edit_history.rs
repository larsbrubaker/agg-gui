//! The rich editor's undo/redo history, [`RichEditHistory`], and the kinds of
//! edit it merges, [`RichEditKind`].  Ports agg-sharp's `RichEditHistory.cs`.
//!
//! Each step is a snapshot: a [`RichDocument::deep_clone`] taken just before
//! the edit, with the selections before and after it.  Restoring a snapshot
//! brings back each block's dirty flag and group objects as they were, so
//! undoing to the start writes the file back byte for byte.
//!
//! # Protocol
//!
//! The editor calls [`RichEditHistory::record`] with the document as it is
//! before mutating it, mutates it, then calls [`RichEditHistory::committed`]
//! with the resulting selection, passing `changed: false` when the edit did
//! nothing (Backspace at the document's start) so no empty step is left.  The
//! editor calls [`RichEditHistory::break_coalescing`] when the caret or
//! selection moves other than by editing, and when the pending style changes
//! (picking Bold mid-word starts a new step).  A line-start shortcut ("# " and
//! the like) is its own step: after the space is recorded and committed as
//! typing, the editor records [`RichEditKind::Other`], applies the shortcut,
//! then commits — so one undo reverts just the auto-format and leaves the
//! typed "# ".  Loading a new document calls [`RichEditHistory::clear`].
//! [`RichEditHistory::undo`] and [`RichEditHistory::redo`] return a fresh
//! document for the editor to adopt in place of its current one.
//!
//! # Merging
//!
//! As most text editors do it: typing one character at the caret where the
//! previous typing ended extends that step, except that a word character typed
//! after a space or punctuation starts a new step — so "hello world" is two
//! steps, "hello " and "world", and each undo removes about a word.  Typing
//! over a selection starts a step that the rest of that word extends, so the
//! first undo removes the replacement's first word and brings back the
//! replaced text together.  Backspace and Delete runs from a caret merge the
//! same way (each with its own kind); Backspace or Delete of a selection is
//! its own step.  Typed text longer than one character (a paste) or containing
//! a line break counts as [`RichEditKind::Other`].  Any Other edit, undo, redo
//! or `break_coalescing` ends a run.  A merged keystroke takes no snapshot, so
//! typing costs a document clone per word, not per character.
//!
//! # Memory
//!
//! A step on the undo side holds one whole-document snapshot (before); an
//! undone step waiting on the redo side holds two (before and after), and
//! drops after again once redone.  The history keeps at most
//! [`RichEditHistory::max_steps`] undo steps (default 200), dropping the
//! oldest.
//!
//! # Differences from C#
//!
//! C# builds this on agg-sharp's `UndoBuffer`, whose commands take and return
//! nothing, so its steps read the replay's input from a shared "active call"
//! object.  Here the two stacks are held directly and undo/redo pass the
//! documents by value, which needs neither; `CaptureState`/`RestoreState`
//! (what `committed(changed: false)` puts back) becomes remembering the redo
//! steps and any capped-off oldest step the new step displaced.  The C#
//! constructor throws `ArgumentOutOfRangeException` for `max_steps` 0;
//! [`RichEditHistory::new`] returns [`RichEditError::MaxStepsOutOfRange`].
//! A `Changed` handler receives the history (`&mut`), so it can read
//! `can_undo`/`can_redo` or even undo again; a change raised while the
//! handlers run is not delivered to them a second time.

use super::document::RichDocument;
use super::edit_error::RichEditError;
use super::selection::RichSelection;

/// What kind of edit the rich editor is about to make, so
/// [`RichEditHistory`] knows which edits merge into one undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichEditKind {
    /// One typed character.  Runs of it merge.
    Typing,
    /// Backspace.  A run of Backspaces from a caret merges.
    Backspace,
    /// Forward Delete.  A run of Deletes from a caret merges.
    Delete,
    /// Anything else — Enter, paste, a style or block change, deleting a
    /// selection: always its own step.
    Other,
}

/// A handler for [`RichEditHistory::on_changed`].
pub type ChangedHandler = Box<dyn FnMut(&mut RichEditHistory)>;

/// One undo step: the document before it, and — only while it waits to be
/// redone — the document after it, taken from the editor at undo time.
struct SnapshotStep {
    before: RichDocument,
    selection_before: RichSelection,
    after: Option<RichDocument>,
    selection_after: RichSelection,
    kind: RichEditKind,
    extendable: bool,
    last_text: Option<String>,
}

/// What `committed(changed: false)` puts back after a new step was recorded:
/// the redo steps the new step cleared, the oldest step the cap dropped, and
/// whether the typing run was broken.
struct BeforeRecord {
    redo: Vec<SnapshotStep>,
    dropped: Option<SnapshotStep>,
    coalescing_broken: bool,
}

/// The rich editor's undo/redo history; see the module header.
pub struct RichEditHistory {
    max_steps: usize,
    undo: Vec<SnapshotStep>,
    redo: Vec<SnapshotStep>,
    // Set once the top step must not be extended: after undo, redo, a caret move or clear.
    coalescing_broken: bool,
    before_record: Option<BeforeRecord>,
    // For a merged keystroke, the top step's previous last_text (outer Some: a merge happened).
    merged_last_text: Option<Option<String>>,
    changed: Vec<ChangedHandler>,
}

impl RichEditHistory {
    /// The step cap [`Self::with_default_steps`] uses.
    pub const DEFAULT_MAX_STEPS: usize = 200;

    /// A history keeping at most `max_steps` undo steps.  0 is
    /// [`RichEditError::MaxStepsOutOfRange`]: the history must keep at least
    /// one step.
    pub fn new(max_steps: usize) -> Result<Self, RichEditError> {
        if max_steps == 0 {
            return Err(RichEditError::MaxStepsOutOfRange);
        }

        Ok(Self {
            max_steps,
            undo: Vec::new(),
            redo: Vec::new(),
            coalescing_broken: true,
            before_record: None,
            merged_last_text: None,
            changed: Vec::new(),
        })
    }

    /// A history keeping [`Self::DEFAULT_MAX_STEPS`] steps (C#'s
    /// `new RichEditHistory()`).
    pub fn with_default_steps() -> Self {
        Self {
            max_steps: Self::DEFAULT_MAX_STEPS,
            undo: Vec::new(),
            redo: Vec::new(),
            coalescing_broken: true,
            before_record: None,
            merged_last_text: None,
            changed: Vec::new(),
        }
    }

    /// Adds a handler run when `can_undo` or `can_redo` may have changed (a new
    /// step, undo, redo, clear), for toolbar enablement.  Not run when a
    /// keystroke merges into the top step, which changes neither.
    pub fn on_changed(&mut self, handler: impl FnMut(&mut RichEditHistory) + 'static) {
        self.changed.push(Box::new(handler));
    }

    /// The most undo steps kept.
    pub fn max_steps(&self) -> usize {
        self.max_steps
    }

    /// True when there is a step to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// True when there is a step to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// How many steps can be undone.
    pub fn undo_count(&self) -> usize {
        self.undo.len()
    }

    /// How many steps can be redone.
    pub fn redo_count(&self) -> usize {
        self.redo.len()
    }

    /// Call just before mutating `before`.  Either extends the top step (a
    /// continuing typing, Backspace or Delete run — nothing is copied) or
    /// snapshots the document as a new step, which clears redo.  `text` is the
    /// typed text for [`RichEditKind::Typing`]; it decides word breaks.
    pub fn record(
        &mut self,
        before: &RichDocument,
        selection_before: RichSelection,
        kind: RichEditKind,
        text: Option<&str>,
    ) {
        let kind = if kind == RichEditKind::Typing && !is_one_character(text) {
            // A paste or a line break (Enter) is always its own step, and typing never merges into it.
            RichEditKind::Other
        } else {
            kind
        };

        if self.extends(selection_before, kind, text) {
            if let Some(top) = self.undo.last_mut() {
                self.before_record = None;
                let previous = std::mem::replace(&mut top.last_text, text.map(str::to_string));
                self.merged_last_text = Some(previous);
                return;
            }
        }

        self.merged_last_text = None;
        let step = SnapshotStep {
            before: before.deep_clone(),
            selection_before,
            after: None,
            selection_after: selection_before,
            kind,
            // Backspace or Delete of a selection removes the selection; nothing extends that step.
            extendable: kind == RichEditKind::Typing
                || (kind != RichEditKind::Other && selection_before.is_empty()),
            last_text: text.map(str::to_string),
        };
        let redo = std::mem::take(&mut self.redo);
        self.undo.push(step);
        let dropped = (self.undo.len() > self.max_steps).then(|| self.undo.remove(0));
        self.before_record = Some(BeforeRecord {
            redo,
            dropped,
            coalescing_broken: self.coalescing_broken,
        });
        self.coalescing_broken = false;
        self.raise_changed();
    }

    /// Call after the mutation [`Self::record`] announced, with the selection
    /// it produced — where the caret goes back to on redo, and where a
    /// following keystroke must be to merge.  Pass `changed` false when the
    /// edit changed nothing: the step just recorded is dropped and the history
    /// (redo included) and typing run are as they were before `record`.
    pub fn committed(&mut self, selection_after: RichSelection, changed: bool) {
        let before_record = self.before_record.take();
        let merged_last_text = self.merged_last_text.take();
        if !changed {
            if let Some(state) = before_record {
                self.undo.pop();
                if let Some(dropped) = state.dropped {
                    self.undo.insert(0, dropped);
                }
                self.redo = state.redo;
                self.coalescing_broken = state.coalescing_broken;
                self.raise_changed();
            } else if let (Some(previous), Some(top)) = (merged_last_text, self.undo.last_mut()) {
                top.last_text = previous;
            }
        } else if let Some(top) = self.undo.last_mut() {
            top.selection_after = selection_after;
        }
    }

    /// Ends the current typing, Backspace or Delete run, so the next edit is a
    /// new step.  The editor calls this when the user moves the caret or
    /// changes the selection other than by editing, and when the pending style
    /// changes.
    pub fn break_coalescing(&mut self) {
        self.coalescing_broken = true;
    }

    /// Forgets every step, for a newly loaded document.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.before_record = None;
        self.merged_last_text = None;
        self.coalescing_broken = true;
        self.raise_changed();
    }

    /// Steps back: returns the document and selection from before the latest
    /// step, for the editor to adopt in place of `current`.  A copy of the
    /// current state is kept for [`Self::redo`].  `None` when there is nothing
    /// to undo.
    pub fn undo(
        &mut self,
        current: &RichDocument,
        current_selection: RichSelection,
    ) -> Option<(RichDocument, RichSelection)> {
        let mut step = self.undo.pop()?;
        self.coalescing_broken = true;
        // Copy the current state, since the editor may keep editing the document it hands over; hand out a
        // copy of before, so a later redo then undo still restores the untouched snapshot.
        step.after = Some(current.deep_clone());
        step.selection_after = current_selection;
        let result = (step.before.deep_clone(), step.selection_before);
        self.redo.push(step);
        self.raise_changed();
        Some(result)
    }

    /// Re-applies the latest undone step: returns the document and selection
    /// it produced.  `None` when there is nothing to redo.  `current` is not
    /// read (the step already holds what it produced); it is taken, as in C#,
    /// so undo and redo are called alike.
    pub fn redo(
        &mut self,
        _current: &RichDocument,
        _current_selection: RichSelection,
    ) -> Option<(RichDocument, RichSelection)> {
        // The editor now owns after; the next undo recaptures it, so the step holds only before meanwhile.
        // Every step on the redo side holds one (undo put it there).
        let document = self.redo.last_mut()?.after.take()?;
        let step = self.redo.pop()?;
        self.coalescing_broken = true;
        let result = (document, step.selection_after);
        self.undo.push(step);
        if self.undo.len() > self.max_steps {
            self.undo.remove(0);
        }
        self.raise_changed();
        Some(result)
    }

    fn extends(
        &self,
        selection_before: RichSelection,
        kind: RichEditKind,
        text: Option<&str>,
    ) -> bool {
        let Some(top) = self.undo.last() else {
            return false;
        };
        if self.coalescing_broken
            || !top.extendable
            || top.kind != kind
            || kind == RichEditKind::Other
        {
            return false;
        }

        // Only a keystroke at the caret the previous one left behind continues the run.
        if !selection_before.is_empty() || selection_before != top.selection_after {
            return false;
        }

        if kind == RichEditKind::Typing {
            // A word starts a new step once the previous keystroke ended in a space or punctuation.
            let previous = top.last_text.as_deref().and_then(|t| t.chars().last());
            let first = text.and_then(|t| t.chars().next());
            return match (first, previous) {
                (Some(first), Some(previous)) => !(is_word_char(first) && !is_word_char(previous)),
                _ => false,
            };
        }

        true
    }

    fn raise_changed(&mut self) {
        let mut handlers = std::mem::take(&mut self.changed);
        for handler in &mut handlers {
            handler(self);
        }
        // Keep any handler added while these ran, after the existing ones.
        handlers.append(&mut self.changed);
        self.changed = handlers;
    }
}

impl Default for RichEditHistory {
    fn default() -> Self {
        Self::with_default_steps()
    }
}

/// One keystroke's text: a single character (an emoji outside the BMP is one
/// `char`, C#'s surrogate pair), never a line break.
fn is_one_character(text: Option<&str>) -> bool {
    let Some(text) = text else {
        return false;
    };
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c != '\n' && c != '\r',
        _ => false,
    }
}

/// The apostrophe counts so "don't" stays one step.
fn is_word_char(c: char) -> bool {
    super::char_class::is_letter_or_digit(c) || c == '_' || c == '\''
}
