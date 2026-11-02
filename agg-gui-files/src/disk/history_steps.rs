//! [`DiskStep`]: the recorded changes of a folder on disk, as a browser's undo history keeps
//! them ([`HistoryStep`]): what each change was, how the notice and the Undo and Redo buttons name
//! it, and how it is taken back and replayed. `DiskFolder`'s recorded writes (`folder_writes`)
//! hand them back. Every step records exact paths, so an undo puts each item back under the name
//! it had: a move that numbered an item to avoid a name already in the destination is moved back
//! to its old name, a rename goes back to the old name, a new folder is removed (only while it is
//! still empty), and a remove is restored from the system trash.
//!
//! An undo or redo never overwrites anything: an item whose old place has been taken since (or
//! that is gone) stays where it is and is named in the outcome's failures, and the step keeps
//! only the items that came back, so a later redo replays exactly those. A copy's undo sends the
//! copies it made to the system trash and its redo restores them; the originals are never part
//! of the step. Every step finishes within the call.

use std::path::{Path, PathBuf};

use super::file::DiskFile;
use super::folder_writes::MovedPath;
use super::trash::{move_to_trash, restore_from_trash, TrashedItem};
use crate::history::{HistoryStep, StepDone, StepOutcome};
use crate::translate::tr;

/// One recorded change on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiskStep {
    /// Items moved between folders on disk, into the folder named `target`.
    Moved {
        moved: Vec<MovedPath>,
        target: String,
    },
    /// A file or folder renamed.
    Renamed(MovedPath),
    /// A new, empty folder made.
    CreatedFolder(PathBuf),
    /// Files and folders sent to the system trash.
    Trashed(Vec<TrashedItem>),
    /// Copies made in the folder named `target`: `copies` while they are there, `trashed` once
    /// an undo has sent them to the system trash.
    Copied {
        copies: Vec<PathBuf>,
        trashed: Vec<TrashedItem>,
        target: String,
    },
}

/// The last part of a path, as the user knows the item.
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `one` ("... {0} to {1}", already translated) naming the one item, or `several` ("... {0}
/// items to {1}") counting them, with `place` for `{1}`.
fn moved_text(names: &[String], one: String, several: String, place: &str) -> String {
    match names {
        [only] => one.replace("{0}", only).replace("{1}", place),
        _ => several
            .replace("{0}", &names.len().to_string())
            .replace("{1}", place),
    }
}

/// The system trash's name on this system.
fn trash_name() -> String {
    if cfg!(windows) {
        tr("the Recycle Bin")
    } else {
        tr("the Trash")
    }
}

/// Whether something is at `path` (a broken link counts).
fn taken(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// "{name}: {why}" (`why` already translated), the line naming an item that could not be put
/// back.
fn failure(path: &Path, why: String) -> String {
    format!("{}: {why}", name_of(path))
}

/// The failure of an item whose name is taken where it would go.
fn name_taken(path: &Path) -> String {
    failure(path, tr("an item with this name is already there"))
}

/// The failure of an item that is gone.
fn gone(path: &Path) -> String {
    failure(path, tr("it is no longer there"))
}

/// Moves the file or folder at `from` to `to`, refusing when `to` is taken by something else (a
/// rename that only changes the case is the same item on a disk that ignores case). A file
/// renamed in its folder goes through [`DiskFile::set_name`], so whatever keeps it by path
/// follows it.
fn move_back(from: &Path, to: &Path) -> Result<(), String> {
    if !taken(from) {
        return Err(gone(from));
    }
    let same_item = from.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase();
    if taken(to) && !same_item {
        return Err(name_taken(to));
    }
    if !to.parent().is_some_and(Path::is_dir) {
        return Err(failure(to, tr("the folder it was in is gone")));
    }
    if from.is_file() && from.parent() == to.parent() {
        let mut file = DiskFile::new(from);
        file.set_name(&name_of(to))
            .map_err(|e| format!("{}: {e}", name_of(from)))?;
        return Ok(());
    }
    std::fs::rename(from, to).map_err(|e| format!("{}: {e}", name_of(from)))
}

/// Puts a trashed item back, refusing when its old name is taken.
fn restore(item: &TrashedItem) -> Result<(), String> {
    if taken(&item.original) {
        Err(name_taken(&item.original))
    } else {
        restore_from_trash(item).map_err(|e| format!("{}: {e}", name_of(&item.original)))
    }
}

/// Notes a failure; true when the item came back.
fn record(result: Result<(), String>, failures: &mut Vec<String>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            failures.push(e);
            false
        }
    }
}

/// Removes the folder at `path` when nothing is in it but the Finder's hidden `.DS_Store` (which
/// the Finder writes into any folder it shows, so it is not the user's work).
fn remove_empty_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(gone(path));
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|e| format!("{}: {e}", name_of(path)))?
        .flatten()
        .map(|e| e.path())
        .collect();
    if entries.iter().any(|e| name_of(e) != ".DS_Store") {
        return Err(failure(path, tr("the folder isn't empty")));
    }
    for entry in entries {
        std::fs::remove_file(&entry).map_err(|e| format!("{}: {e}", name_of(path)))?;
    }
    std::fs::remove_dir(path).map_err(|e| format!("{}: {e}", name_of(path)))
}

/// Each copy at `copies` sent to the trash; one that is gone is named in `failures`.
fn trash_copies(copies: &[PathBuf], failures: &mut Vec<String>) -> Vec<TrashedItem> {
    copies
        .iter()
        .filter_map(|path| match move_to_trash(path) {
            Ok(item) => Some(item),
            Err(_) => {
                failures.push(gone(path));
                None
            }
        })
        .collect()
}

impl DiskStep {
    /// Copies made in the folder named `target`.
    pub fn copied(copies: Vec<PathBuf>, target: &str) -> Self {
        Self::Copied {
            copies,
            trashed: Vec::new(),
            target: target.to_string(),
        }
    }

    /// What the step did, as the notice and the buttons' tooltips say it.
    pub fn describe(&self) -> String {
        match self {
            Self::Moved { moved, target } => {
                let names: Vec<String> = moved.iter().map(|m| name_of(&m.from)).collect();
                moved_text(
                    &names,
                    tr("Moved {0} to {1}"),
                    tr("Moved {0} items to {1}"),
                    target,
                )
            }
            Self::Renamed(renamed) => tr("Renamed {0} to {1}")
                .replace("{0}", &name_of(&renamed.from))
                .replace("{1}", &name_of(&renamed.to)),
            Self::CreatedFolder(path) => tr("Created folder {0}").replace("{0}", &name_of(path)),
            Self::Trashed(items) => {
                let names: Vec<String> = items.iter().map(|t| name_of(&t.original)).collect();
                moved_text(
                    &names,
                    tr("Moved {0} to {1}"),
                    tr("Moved {0} items to {1}"),
                    &trash_name(),
                )
            }
            Self::Copied {
                copies,
                trashed,
                target,
            } => {
                let names: Vec<String> = copies
                    .iter()
                    .chain(trashed.iter().map(|t| &t.original))
                    .map(|p| name_of(p))
                    .collect();
                moved_text(
                    &names,
                    tr("Copied {0} to {1}"),
                    tr("Copied {0} items to {1}"),
                    target,
                )
            }
        }
    }

    /// Takes the step back: the step that replays what came back (`None` when nothing did) and
    /// a line per item that could not.
    pub fn take_back(self) -> (Option<DiskStep>, Vec<String>) {
        let mut failures = Vec::new();
        let kept = match self {
            Self::Moved { moved, target } => {
                // Last moved first, so items that traded places come back in order.
                let mut back: Vec<MovedPath> = moved
                    .into_iter()
                    .rev()
                    .filter(|m| record(move_back(&m.to, &m.from), &mut failures))
                    .collect();
                back.reverse();
                (!back.is_empty()).then_some(Self::Moved {
                    moved: back,
                    target,
                })
            }
            Self::Renamed(renamed) => record(move_back(&renamed.to, &renamed.from), &mut failures)
                .then_some(Self::Renamed(renamed)),
            Self::CreatedFolder(path) => record(remove_empty_folder(&path), &mut failures)
                .then_some(Self::CreatedFolder(path)),
            Self::Trashed(items) => {
                let back: Vec<TrashedItem> = items
                    .into_iter()
                    .filter(|t| record(restore(t), &mut failures))
                    .collect();
                (!back.is_empty()).then_some(Self::Trashed(back))
            }
            // The copies were made by this step, so taking them away loses nothing; they go to
            // the system trash all the same, where a redo finds them.
            Self::Copied { copies, target, .. } => {
                let trashed = trash_copies(&copies, &mut failures);
                (!trashed.is_empty()).then_some(Self::Copied {
                    copies: Vec::new(),
                    trashed,
                    target,
                })
            }
        };
        (kept, failures)
    }

    /// Replays the step after an undo: the step that takes back what was replayed (`None` when
    /// nothing was) and a line per item that could not be.
    pub fn replay(self) -> (Option<DiskStep>, Vec<String>) {
        let mut failures = Vec::new();
        let kept = match self {
            Self::Moved { moved, target } => {
                let again: Vec<MovedPath> = moved
                    .into_iter()
                    .filter(|m| record(move_back(&m.from, &m.to), &mut failures))
                    .collect();
                (!again.is_empty()).then_some(Self::Moved {
                    moved: again,
                    target,
                })
            }
            Self::Renamed(renamed) => record(move_back(&renamed.from, &renamed.to), &mut failures)
                .then_some(Self::Renamed(renamed)),
            Self::CreatedFolder(path) => {
                let made = if taken(&path) {
                    Err(name_taken(&path))
                } else {
                    std::fs::create_dir(&path).map_err(|e| format!("{}: {e}", name_of(&path)))
                };
                record(made, &mut failures).then_some(Self::CreatedFolder(path))
            }
            Self::Trashed(items) => {
                let originals: Vec<PathBuf> = items.into_iter().map(|t| t.original).collect();
                let again = trash_copies(&originals, &mut failures);
                (!again.is_empty()).then_some(Self::Trashed(again))
            }
            Self::Copied {
                trashed, target, ..
            } => {
                let copies: Vec<PathBuf> = trashed
                    .into_iter()
                    .filter_map(|t| record(restore(&t), &mut failures).then_some(t.original))
                    .collect();
                (!copies.is_empty()).then(|| Self::copied(copies, &target))
            }
        };
        (kept, failures)
    }
}

/// The outcome a [`HistoryStep`] hands on.
fn outcome((kept, failures): (Option<DiskStep>, Vec<String>)) -> StepOutcome {
    StepOutcome {
        kept: kept.map(|step| Box::new(step) as Box<dyn HistoryStep>),
        failures,
    }
}

impl HistoryStep for DiskStep {
    fn description(&self) -> String {
        self.describe()
    }

    /// [`DiskStep::take_back`], finished before this returns.
    fn undo(self: Box<Self>, done: StepDone) {
        done(outcome(self.take_back()));
    }

    /// [`DiskStep::replay`], finished before this returns.
    fn redo(self: Box<Self>, done: StepDone) {
        done(outcome(self.replay()));
    }
}
