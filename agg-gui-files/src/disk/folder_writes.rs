//! A [`DiskFolder`]'s changes, and its [`WritableFolder`] answers. `add` copies content in and
//! makes the folders a [`CreateFolderItem`] names; `remove` sends items to the system trash
//! (`trash`); `rename` renames a file ([`DiskFile::set_name`]) or a folder
//! ([`DiskFolderLink::set_name`]); a move between two folders on disk renames each item into the
//! destination ([`DiskFolder::move_from_disk`]), and from anywhere else copies the items in and
//! keeps the originals (`copy`), which also runs a copy as a task (`copy_in`). `add_async` is
//! `add` awaiting each item's content. `move_from_disk`, `rename_on_disk` and `remove_to_trash` answer
//! what they did ([`MovedPath`], [`TrashedItem`]): the record an undo puts things back by.
//!
//! A folder whose [`super::DiskRules`] are read-only refuses every change: `allow_action` answers
//! no, and each write fails before touching the disk. Each write runs with the folder's watch
//! off (`DiskFolder::unwatched`), so its own changes are not reported back to it; after a write
//! that changed anything the folder lists itself again and raises content-changed.

use std::any::Any;
use std::path::PathBuf;
use std::rc::Rc;

use super::file::DiskFile;
use super::folder::DiskFolder;
use super::folder_link::DiskFolderLink;
use super::paths::{non_colliding_path, path_file_name};
use super::trash::{move_to_trash, TrashedItem};
use crate::create_folder_item::CreateFolderItem;
use crate::history::Recorded;
use crate::item::Item;
use crate::spawner::LocalTask;
use crate::translate::tr;
use crate::writable::{ContainerActions, CopyDone, WritableFolder};

/// One file or folder a move or rename put somewhere else on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovedPath {
    /// Where it was.
    pub from: PathBuf,
    /// Where it is now.
    pub to: PathBuf,
}

/// What an `add` has done so far.
#[derive(Default)]
struct AddProgress {
    is_dirty: bool,
    errors: Vec<String>,
}

/// The path of a file or folder on disk that `item` is, `None` for any other item.
pub(super) fn disk_path_of(item: &dyn Item) -> Option<PathBuf> {
    let any = item.as_any()?;
    if let Some(file) = any.downcast_ref::<DiskFile>() {
        Some(file.file_path().to_path_buf())
    } else {
        any.downcast_ref::<DiskFolderLink>()
            .map(|dir| dir.file_path().to_path_buf())
    }
}

impl DiskFolder {
    /// `Err` with the read-only message when the rules refuse changes. The message doesn't name
    /// the folder: the caller names the one the user acted on, whose shown name may not be its
    /// name on disk (an app's "History" row opening a folder named otherwise).
    pub(super) fn refuse_if_read_only(&self) -> Result<(), String> {
        if self.rules.read_only() {
            Err(tr("This folder can't be changed"))
        } else {
            Ok(())
        }
    }

    /// Adds items whose content is at hand: copies each file item's content into this folder
    /// (numbering a name that is taken), makes each folder a [`CreateFolderItem`] names, and
    /// moves each folder row's folder here, then lists the folder again. An item with no content
    /// adds nothing. A file that fails is skipped and reported in the error, and the rest carry
    /// on.
    pub fn add(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        self.refuse_if_read_only()?;
        self.unwatched(|folder| folder.add_now(items))
    }

    fn add_now(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        if items.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.full_path).map_err(|e| e.to_string())?;
        let mut added = AddProgress::default();
        for item in items {
            if !self.create_folder_here(item.as_ref(), &mut added)
                && !self.move_folder_here(item.as_ref(), &mut added)
            {
                if let Some(stream_item) = item.as_asset_stream() {
                    let content = stream_item.get_stream().map_err(String::from);
                    self.write_content(&stream_item.file_name(), content, &mut added);
                }
            }
        }
        self.finish_add(added)
    }

    /// As [`Self::add`], awaiting each item's content, so content that has to be downloaded
    /// first is waited for. Run on the caller's executor.
    pub async fn add_async(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        self.refuse_if_read_only()?;
        self.live.set_watch_enabled(false);
        let result = self.add_async_now(items).await;
        self.live.set_watch_enabled(true);
        result
    }

    async fn add_async_now(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        if items.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.full_path).map_err(|e| e.to_string())?;
        let mut added = AddProgress::default();
        for item in items {
            if !self.create_folder_here(item.as_ref(), &mut added)
                && !self.move_folder_here(item.as_ref(), &mut added)
            {
                if let Some(stream_item) = item.as_asset_stream() {
                    let file_name = stream_item.file_name();
                    let content = stream_item.get_stream_async().await.map_err(String::from);
                    self.write_content(&file_name, content, &mut added);
                }
            }
        }
        self.finish_add(added)
    }

    /// The end of an add: lists the folder again when anything landed, and reports the failures.
    fn finish_add(&mut self, added: AddProgress) -> Result<(), String> {
        if added.is_dirty {
            self.reload_content();
        }
        if added.errors.is_empty() {
            Ok(())
        } else {
            Err(added.errors.join("\n"))
        }
    }

    /// `add`'s [`CreateFolderItem`] case: makes the named folder here (an existing one is kept,
    /// so adding a folder twice is harmless). False when `item` is not a new folder.
    fn create_folder_here(&self, item: &dyn Item, added: &mut AddProgress) -> bool {
        let Some(new_folder) = item
            .as_any()
            .and_then(|a| a.downcast_ref::<CreateFolderItem>())
        else {
            return false;
        };
        let target_folder_path = self.full_path.join(new_folder.name());
        match std::fs::create_dir_all(&target_folder_path) {
            Ok(()) => added.is_dirty = true,
            Err(e) => added
                .errors
                .push(format!("{}: {e}", target_folder_path.to_string_lossy())),
        }
        true
    }

    /// `add`'s folder case: moves a folder row's folder into this one. False when `item` is not
    /// a folder row on disk.
    fn move_folder_here(&self, item: &dyn Item, added: &mut AddProgress) -> bool {
        let Some(dir_link) = item
            .as_any()
            .and_then(|a| a.downcast_ref::<DiskFolderLink>())
        else {
            return false;
        };
        let dest_dir = self.full_path.join(path_file_name(dir_link.file_path()));
        if dir_link.file_path().is_dir() && !dest_dir.is_dir() {
            match std::fs::rename(dir_link.file_path(), &dest_dir) {
                Ok(()) => added.is_dirty = true,
                Err(e) => added.errors.push(e.to_string()),
            }
        }
        true
    }

    /// `add`'s file case: writes one item's content under a name that is not taken.
    fn write_content(
        &self,
        file_name: &str,
        content: Result<Option<Vec<u8>>, String>,
        added: &mut AddProgress,
    ) {
        let target_path = non_colliding_path(&self.full_path.join(file_name));
        // The content is in hand before the file is made, so an unavailable source never leaves
        // an empty file behind.
        let written = content.and_then(|content| match content {
            Some(bytes) => std::fs::write(&target_path, bytes)
                .map(|()| true)
                .map_err(|e| e.to_string()),
            None => Ok(false),
        });
        match written {
            Ok(true) => added.is_dirty = true,
            Ok(false) => {}
            Err(e) => added.errors.push(format!(
                "Error adding file: {}\r\n{e}",
                target_path.to_string_lossy()
            )),
        }
    }

    /// Moves items from `source` between two folders on disk: each file and folder is renamed
    /// into this folder (numbered when the name is taken) and both listings reload, even when a
    /// move fails part way. Items that are not files or folders on disk stay put and are named in
    /// the error. Answers each move made, where the item was and where it is now (its numbered
    /// name when it was taken here): what an undo moves back.
    pub fn move_from_disk(
        &mut self,
        items: &[Rc<dyn Item>],
        source: &mut DiskFolder,
    ) -> (Vec<MovedPath>, Result<(), String>) {
        if let Err(e) = self
            .refuse_if_read_only()
            .and_then(|()| source.refuse_if_read_only())
        {
            return (Vec::new(), Err(e));
        }
        source.live.set_watch_enabled(false);
        let answer = self.unwatched(|folder| folder.move_from_disk_now(items, source));
        source.live.set_watch_enabled(true);
        answer
    }

    fn move_from_disk_now(
        &mut self,
        items: &[Rc<dyn Item>],
        source: &mut DiskFolder,
    ) -> (Vec<MovedPath>, Result<(), String>) {
        if let Err(e) = std::fs::create_dir_all(&self.full_path) {
            return (Vec::new(), Err(e.to_string()));
        }
        let mut skipped = Vec::new();
        let mut moved = Vec::new();
        let mut failure = None;
        for item in items {
            let Some(from) = disk_path_of(item.as_ref()) else {
                skipped.push(item.name().to_string());
                continue;
            };
            let destination = non_colliding_path(&self.full_path.join(path_file_name(&from)));
            if let Err(e) = std::fs::rename(&from, &destination) {
                failure = Some(format!("{}: {e}", from.to_string_lossy()));
                break;
            }
            moved.push(MovedPath {
                from,
                to: destination,
            });
        }
        if !moved.is_empty() {
            self.reload_content();
            source.reload_content();
        }
        let result = if let Some(failure) = failure {
            Err(failure)
        } else if !skipped.is_empty() {
            Err(format!(
                "{}: {}",
                tr("These items could not be moved"),
                skipped.join(", ")
            ))
        } else {
            Ok(())
        };
        (moved, result)
    }

    /// Renames a file or folder of this folder to `new_name` (numbered when the name is taken),
    /// then lists the folder again. Answers the rename made, if any (the same name again, or an
    /// item with no name on disk, makes none): what an undo renames back.
    pub fn rename_on_disk(
        &mut self,
        item: &Rc<dyn Item>,
        new_name: &str,
    ) -> (Option<MovedPath>, Result<(), String>) {
        if let Err(e) = self.refuse_if_read_only() {
            return (None, Err(e));
        }
        self.unwatched(|folder| folder.rename_on_disk_now(item, new_name))
    }

    fn rename_on_disk_now(
        &mut self,
        item: &Rc<dyn Item>,
        new_name: &str,
    ) -> (Option<MovedPath>, Result<(), String>) {
        let any = item.as_any();
        let renamed = if let Some(file) = any.and_then(|a| a.downcast_ref::<DiskFile>()) {
            let mut renamed = file.clone();
            renamed.set_name(new_name).map(|()| {
                (
                    file.file_path().to_path_buf(),
                    renamed.file_path().to_path_buf(),
                )
            })
        } else if let Some(dir) = any.and_then(|a| a.downcast_ref::<DiskFolderLink>()) {
            let mut renamed = dir.clone();
            renamed.set_name(new_name).map(|()| {
                (
                    dir.file_path().to_path_buf(),
                    renamed.file_path().to_path_buf(),
                )
            })
        } else {
            Ok((PathBuf::new(), PathBuf::new()))
        };
        self.reload_content();
        match renamed {
            Ok((from, to)) if from != to => (Some(MovedPath { from, to }), Ok(())),
            Ok(_) => (None, Ok(())),
            Err(e) => (None, Err(e)),
        }
    }

    /// Sends each file or folder (with everything in it) to the system trash
    /// ([`super::move_to_trash`]), then lists the folder again. Answers where each one went (what
    /// an undo puts back with [`super::restore_from_trash`]) and the last failure; an item that
    /// failed stays where it was, and one already gone from the disk is passed over.
    pub fn remove_to_trash(
        &mut self,
        items: &[Rc<dyn Item>],
    ) -> (Vec<TrashedItem>, Result<(), String>) {
        if let Err(e) = self.refuse_if_read_only() {
            return (Vec::new(), Err(e));
        }
        self.unwatched(|folder| folder.remove_to_trash_now(items))
    }

    fn remove_to_trash_now(
        &mut self,
        items: &[Rc<dyn Item>],
    ) -> (Vec<TrashedItem>, Result<(), String>) {
        let mut result = Ok(());
        let mut trashed = Vec::new();
        for item in items {
            let Some(path) = disk_path_of(item.as_ref()) else {
                continue;
            };
            if std::fs::symlink_metadata(&path).is_err() {
                // Already gone (deleted outside the app): nothing to trash.
                continue;
            }
            match move_to_trash(&path) {
                Ok(item) => trashed.push(item),
                Err(e) => result = Err(e),
            }
        }
        self.reload_content();
        (trashed, result)
    }
}

impl WritableFolder for DiskFolder {
    fn add(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        DiskFolder::add(self, items)
    }

    /// [`DiskFolder::remove_to_trash`].
    fn remove(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        self.remove_to_trash(items).1
    }

    /// [`DiskFolder::rename_on_disk`]. Other items have no name on disk to change.
    fn rename(&mut self, item: &Rc<dyn Item>, new_name: &str) -> Result<(), String> {
        self.rename_on_disk(item, new_name).1
    }

    /// Nothing when the rules are read-only; everything otherwise.
    fn allow_action(&self, action: ContainerActions) -> bool {
        match action {
            ContainerActions::AddItems
            | ContainerActions::AddContainers
            | ContainerActions::RemoveItems
            | ContainerActions::RenameItems => !self.rules.read_only(),
        }
    }

    /// A rename on disk when `source` is a folder on disk too (the item is never missing from
    /// both places). From anywhere else the items are copied and the originals stay: a drag
    /// between the computer and another place copies, and removing the original after a copy
    /// that may not have landed would lose it.
    fn move_items(
        &mut self,
        items: &[Rc<dyn Item>],
        source: &mut dyn WritableFolder,
    ) -> Result<(), String> {
        match source
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<DiskFolder>())
        {
            Some(disk_source) => self.move_from_disk(items, disk_source).1,
            None => self.copy_items_in(items).1,
        }
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }

    /// The moves (or, from a folder not on disk, the copies), recorded (`recorded`).
    fn move_in_recorded(
        &mut self,
        items: &[Rc<dyn Item>],
        source: &mut dyn WritableFolder,
    ) -> Recorded {
        self.move_in_recording(items, source)
    }

    /// The rename, recorded (`recorded`).
    fn rename_recorded(&mut self, item: &Rc<dyn Item>, new_name: &str) -> Recorded {
        self.rename_recording(item, new_name)
    }

    /// The new folder, recorded when it is new (`recorded`).
    fn create_folder_recorded(&mut self, name: &str) -> Recorded {
        self.create_folder_recording(name)
    }

    /// The remove, recorded with where each item went in the trash (`recorded`).
    fn remove_recorded(&mut self, items: &[Rc<dyn Item>]) -> Recorded {
        self.remove_recording(items)
    }

    /// [`DiskFolder::copy_in_async`] as a task, through a folder of its own on the same path and
    /// rules; once it has finished, this folder relists at its next `pump_live_refresh` and
    /// `done` hears the outcome, with the step that sends the copies to the trash.
    fn copy_in(&self, items: Vec<Rc<dyn Item>>, done: CopyDone) -> Option<LocalTask> {
        Some(self.copy_task(items, done))
    }
}
