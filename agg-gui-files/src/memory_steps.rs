//! [`MemoryFolder`](super::MemoryFolder)'s recorded writes and their undo steps (a child of
//! `memory_folder.rs`, whose private nodes it moves).
//!
//! Every recorded write keeps identity: a moved, removed or restored file or folder is the same
//! entry (its ID unchanged), taken out of one folder and put into another, so a browser's
//! selection and the tree's rows still know it after an undo. A move between two memory folders
//! of any trees takes the entries across (a folder joins the place of the tree it lands in); a
//! name already taken where an entry lands gets a number ("gear (2).stl"), and the step
//! remembers the name each had so undo gives it back. A move from a folder that is not in memory
//! is the plain add-then-remove, unrecorded. Undo and redo finish at once, each handing back the
//! step that reverses it; an entry no longer where the step left it (changed meanwhile) is
//! named in the outcome's failures and left out of the kept step.

use std::rc::Rc;

use super::{MemoryFile, MemoryFolder, Node};
use crate::history::{HistoryStep, Recorded, StepDone, StepOutcome};
use crate::item::Item;
use crate::translate::tr;
use crate::writable::WritableFolder;

/// One file or folder, taken out of its folder.
#[derive(Clone)]
pub(super) enum Entry {
    File(Rc<MemoryFile>),
    Folder(Rc<Node>),
}

impl Entry {
    fn id(&self) -> &str {
        match self {
            Entry::File(file) => &file.id,
            Entry::Folder(node) => &node.id,
        }
    }

    fn name(&self) -> String {
        match self {
            Entry::File(file) => file.name.clone(),
            Entry::Folder(node) => node.name.borrow().clone(),
        }
    }
}

impl Node {
    /// Takes the entry `id` out of this folder (a folder out of its tree too).
    pub(super) fn take(&self, id: &str) -> Option<Entry> {
        let mut files = self.files.borrow_mut();
        if let Some(at) = files.iter().position(|f| f.id == id) {
            return Some(Entry::File(files.remove(at)));
        }
        drop(files);
        let mut folders = self.folders.borrow_mut();
        let at = folders.iter().position(|f| f.id == id)?;
        let node = folders.remove(at);
        node.detach();
        Some(Entry::Folder(node))
    }

    /// Puts `entry` here as `name` (or the first free name like it), its ID kept; a folder joins
    /// this folder's tree and place. Answers the name it got.
    pub(super) fn put(self: &Rc<Self>, entry: Entry, name: &str) -> String {
        let name = self.free_name(name);
        match entry {
            Entry::File(file) => {
                let file = match file.name == name {
                    true => file,
                    false => Rc::new(MemoryFile {
                        id: file.id.clone(),
                        name: name.clone(),
                        bytes: Rc::clone(&file.bytes),
                    }),
                };
                self.files.borrow_mut().push(file);
            }
            Entry::Folder(node) => {
                *node.name.borrow_mut() = name.clone();
                *node.parent.borrow_mut() = Rc::downgrade(self);
                node.share_place(&self.place.borrow());
                self.folders.borrow_mut().push(node);
            }
        }
        name
    }
}

/// "Moved gear.stl to Brackets" / "Moved 3 items to Brackets" and their kin.
fn names_text(names: &[String], one: String, several: String, place: &str) -> String {
    match names {
        [only] => one.replace("{0}", only).replace("{1}", place),
        _ => several
            .replace("{0}", &names.len().to_string())
            .replace("{1}", place),
    }
}

/// One recorded change in memory (see the module header).
pub(super) enum MemoryStep {
    /// Entries moved from `from` to `to`: each ID with the name it had and the name it got.
    Moved {
        from: Rc<Node>,
        to: Rc<Node>,
        moved: Vec<(String, String, String)>,
    },
    /// The entry `id` in `folder` renamed from `from` to `to`.
    Renamed {
        folder: Rc<Node>,
        id: String,
        from: String,
        to: String,
    },
    /// The folder `made` created in `folder`.
    Created { folder: Rc<Node>, made: Rc<Node> },
    /// The created folder taken back out, kept to put back on redo.
    Uncreated { folder: Rc<Node>, made: Rc<Node> },
    /// Entries removed from `folder`, kept to put back.
    Removed {
        folder: Rc<Node>,
        removed: Vec<Entry>,
    },
    /// Removed entries put back, by ID, to remove again on redo.
    Restored {
        folder: Rc<Node>,
        ids: Vec<(String, String)>,
    },
}

/// Moves `ids` (with the names to give them) from `from` to `to`; answers what moved (ID, name
/// before, name after) and the failures.
fn shift(
    from: &Rc<Node>,
    to: &Rc<Node>,
    ids: &[(String, String)],
) -> (Vec<(String, String, String)>, Vec<String>) {
    let (mut moved, mut failures) = (Vec::new(), Vec::new());
    for (id, name) in ids {
        match from.take(id) {
            Some(entry) => {
                let before = entry.name();
                let after = to.put(entry, name);
                moved.push((id.clone(), before, after));
            }
            None => failures.push(tr("{0} is not in this folder").replace("{0}", name)),
        }
    }
    from.content_changed.raise();
    to.content_changed.raise();
    (moved, failures)
}

impl MemoryStep {
    /// The step that reverses this one, after doing the reversal, with any failures.
    fn reverse(self) -> (Option<MemoryStep>, Vec<String>) {
        match self {
            MemoryStep::Moved { from, to, moved } => {
                let back: Vec<(String, String)> = moved
                    .into_iter()
                    .map(|(id, before, _)| (id, before))
                    .collect();
                let (moved, failures) = shift(&to, &from, &back);
                let kept = (!moved.is_empty()).then_some(MemoryStep::Moved {
                    from: to,
                    to: from,
                    moved,
                });
                (kept, failures)
            }
            MemoryStep::Renamed {
                folder,
                id,
                from,
                to,
            } => match folder.take(&id) {
                Some(entry) => {
                    let got = folder.put(entry, &from);
                    folder.content_changed.raise();
                    let step = MemoryStep::Renamed {
                        folder,
                        id,
                        from: to,
                        to: got,
                    };
                    (Some(step), Vec::new())
                }
                None => (
                    None,
                    vec![tr("{0} is not in this folder").replace("{0}", &to)],
                ),
            },
            MemoryStep::Created { folder, made } => match folder.take(&made.id) {
                Some(_) => {
                    folder.content_changed.raise();
                    (Some(MemoryStep::Uncreated { folder, made }), Vec::new())
                }
                None => {
                    let name = made.name.borrow().clone();
                    (
                        None,
                        vec![tr("{0} is not in this folder").replace("{0}", &name)],
                    )
                }
            },
            MemoryStep::Uncreated { folder, made } => {
                let name = made.name.borrow().clone();
                folder.put(Entry::Folder(Rc::clone(&made)), &name);
                folder.content_changed.raise();
                (Some(MemoryStep::Created { folder, made }), Vec::new())
            }
            MemoryStep::Removed { folder, removed } => {
                let ids: Vec<(String, String)> = removed
                    .into_iter()
                    .map(|entry| {
                        let (id, name) = (entry.id().to_string(), entry.name());
                        let got = folder.put(entry, &name);
                        (id, got)
                    })
                    .collect();
                folder.content_changed.raise();
                (Some(MemoryStep::Restored { folder, ids }), Vec::new())
            }
            MemoryStep::Restored { folder, ids } => {
                let mut failures = Vec::new();
                let removed: Vec<Entry> = ids
                    .iter()
                    .filter_map(|(id, name)| {
                        let entry = folder.take(id);
                        if entry.is_none() {
                            failures.push(tr("{0} is not in this folder").replace("{0}", name));
                        }
                        entry
                    })
                    .collect();
                folder.content_changed.raise();
                let kept = (!removed.is_empty()).then_some(MemoryStep::Removed { folder, removed });
                (kept, failures)
            }
        }
    }

    fn finish(self, done: StepDone) {
        let (kept, failures) = self.reverse();
        done(StepOutcome {
            kept: kept.map(|step| Box::new(step) as Box<dyn HistoryStep>),
            failures,
        });
    }
}

impl HistoryStep for MemoryStep {
    fn description(&self) -> String {
        match self {
            MemoryStep::Moved { to, moved, .. } => {
                let names: Vec<String> = moved.iter().map(|m| m.2.clone()).collect();
                names_text(
                    &names,
                    tr("Moved {0} to {1}"),
                    tr("Moved {0} items to {1}"),
                    &to.name.borrow(),
                )
            }
            MemoryStep::Renamed { from, to, .. } => tr("Renamed {0} to {1}")
                .replace("{0}", from)
                .replace("{1}", to),
            MemoryStep::Created { made, .. } | MemoryStep::Uncreated { made, .. } => {
                tr("Created folder {0}").replace("{0}", &made.name.borrow())
            }
            MemoryStep::Removed { removed, .. } => {
                let names: Vec<String> = removed.iter().map(Entry::name).collect();
                names_text(&names, tr("Removed {0}"), tr("Removed {0} items"), "")
            }
            MemoryStep::Restored { ids, .. } => {
                let names: Vec<String> = ids.iter().map(|(_, n)| n.clone()).collect();
                names_text(&names, tr("Removed {0}"), tr("Removed {0} items"), "")
            }
        }
    }

    /// Reverses the change, finished before this returns.
    fn undo(self: Box<Self>, done: StepDone) {
        (*self).finish(done);
    }

    /// Makes the change again, finished before this returns.
    fn redo(self: Box<Self>, done: StepDone) {
        (*self).finish(done);
    }
}

fn recorded(result: Result<(), String>, step: Option<MemoryStep>) -> Recorded {
    Recorded {
        result,
        step: step.map(|s| Box::new(s) as Box<dyn HistoryStep>),
    }
}

impl MemoryFolder {
    /// Moves `items` here from the memory folder `source`, IDs kept (see the module header).
    pub(super) fn move_in_from(&self, items: &[Rc<dyn Item>], source: &MemoryFolder) -> Recorded {
        for item in items {
            let inside = source
                .node
                .folders
                .borrow()
                .iter()
                .find(|f| f.id == item.id())
                .is_some_and(|moved| self.node.is_self_or_above(moved));
            if inside {
                return Recorded::unrecorded(Err(tr("A folder cannot be copied into itself")));
            }
        }
        let ids: Vec<(String, String)> = items
            .iter()
            .map(|i| (i.id().to_string(), i.name().to_string()))
            .collect();
        let (moved, failures) = shift(&source.node, &self.node, &ids);
        let result = match failures.first() {
            Some(first) => Err(first.clone()),
            None => Ok(()),
        };
        let step = (!moved.is_empty()).then(|| MemoryStep::Moved {
            from: Rc::clone(&source.node),
            to: Rc::clone(&self.node),
            moved,
        });
        recorded(result, step)
    }

    pub(super) fn rename_recording(&mut self, item: &Rc<dyn Item>, new_name: &str) -> Recorded {
        let from = item.name().to_string();
        let result = self.rename(item, new_name);
        let step = result.is_ok().then(|| MemoryStep::Renamed {
            folder: Rc::clone(&self.node),
            id: item.id().to_string(),
            from,
            to: new_name.to_string(),
        });
        recorded(result, step)
    }

    pub(super) fn create_folder_recording(&mut self, name: &str) -> Recorded {
        let existed = self
            .node
            .folders
            .borrow()
            .iter()
            .any(|f| *f.name.borrow() == name);
        let made = self.changing(|| self.node.folder_named(name).map(|_| ()));
        let step = match (&made, existed) {
            (Ok(()), false) => self
                .node
                .folders
                .borrow()
                .iter()
                .find(|f| *f.name.borrow() == name)
                .map(|made| MemoryStep::Created {
                    folder: Rc::clone(&self.node),
                    made: Rc::clone(made),
                }),
            _ => None,
        };
        recorded(made, step)
    }

    pub(super) fn remove_recording(&mut self, items: &[Rc<dyn Item>]) -> Recorded {
        let mut removed = Vec::new();
        let mut result = Ok(());
        for item in items {
            match self.node.take(item.id()) {
                Some(entry) => removed.push(entry),
                None => {
                    result = Err(tr("{0} is not in this folder").replace("{0}", item.name()));
                    break;
                }
            }
        }
        self.node.content_changed.raise();
        let step = (!removed.is_empty()).then(|| MemoryStep::Removed {
            folder: Rc::clone(&self.node),
            removed,
        });
        recorded(result, step)
    }
}

/// The source of a move, if it is a memory folder.
pub(super) fn memory_source(source: &mut dyn WritableFolder) -> Option<MemoryFolder> {
    source
        .as_any_mut()
        .and_then(|a| a.downcast_mut::<MemoryFolder>())
        .map(|m| m.clone())
}
