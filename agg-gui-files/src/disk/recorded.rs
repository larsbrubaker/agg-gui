//! A [`DiskFolder`]'s recorded writes: each change runs as its plain form does (`folder_writes`,
//! `copy`) and hands back a [`Recorded`] with the [`DiskStep`] that takes it back, when anything
//! changed. The `WritableFolder` recorded methods and `copy_in` answer these.
//!
//! A move from another folder on disk records the moves (each item's old and new path); a move
//! from anywhere else copies, keeping the originals, and records the copies made. Create Folder
//! records the folder only when it is new: a folder already there is kept as it is, and undoing
//! must not take it away.

use std::path::PathBuf;
use std::rc::Rc;

use super::folder::DiskFolder;
use super::history_steps::DiskStep;
use crate::create_folder_item::CreateFolderItem;
use crate::folder::Folder;
use crate::history::{HistoryStep, Recorded};
use crate::item::Item;
use crate::writable::WritableFolder;

/// `result` with `step` when the change left something to take back.
fn recorded(result: Result<(), String>, step: Option<DiskStep>) -> Recorded {
    Recorded {
        result,
        step: step.map(|step| Box::new(step) as Box<dyn HistoryStep>),
    }
}

/// The step of copies made in `target`, when any were.
pub(super) fn copies_step(made: Vec<PathBuf>, target: &str) -> Option<DiskStep> {
    (!made.is_empty()).then(|| DiskStep::copied(made, target))
}

impl DiskFolder {
    /// `move_items`, recorded: the moves from another folder on disk, the copies from anywhere
    /// else.
    pub(super) fn move_in_recording(
        &mut self,
        items: &[Rc<dyn Item>],
        source: &mut dyn WritableFolder,
    ) -> Recorded {
        let target = self.name().to_string();
        match source
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<DiskFolder>())
        {
            Some(disk_source) => {
                let (moved, result) = self.move_from_disk(items, disk_source);
                let step = (!moved.is_empty()).then_some(DiskStep::Moved { moved, target });
                recorded(result, step)
            }
            None => {
                let (made, result) = self.copy_items_in(items);
                recorded(result, copies_step(made, &target))
            }
        }
    }

    /// `rename`, recorded when a name changed.
    pub(super) fn rename_recording(&mut self, item: &Rc<dyn Item>, new_name: &str) -> Recorded {
        let (renamed, result) = self.rename_on_disk(item, new_name);
        recorded(result, renamed.map(DiskStep::Renamed))
    }

    /// Create Folder, recorded when the folder is new.
    pub(super) fn create_folder_recording(&mut self, name: &str) -> Recorded {
        let path = self.full_path().join(name);
        let existed = path.exists();
        let item: Rc<dyn Item> = Rc::new(CreateFolderItem::new(name));
        let result = self.add(&[item]);
        let step = (!existed && path.is_dir()).then_some(DiskStep::CreatedFolder(path));
        recorded(result, step)
    }

    /// `remove`, recorded with where each item went in the trash.
    pub(super) fn remove_recording(&mut self, items: &[Rc<dyn Item>]) -> Recorded {
        let (trashed, result) = self.remove_to_trash(items);
        recorded(
            result,
            (!trashed.is_empty()).then_some(DiskStep::Trashed(trashed)),
        )
    }
}
