//! Copying items into a [`DiskFolder`] while the originals stay where they are: what a move from
//! a folder that is not on disk does (`folder_writes`' `move_items`), so an item is never missing
//! from both places. A move between two folders on disk is `DiskFolder::move_from_disk`; this
//! file never removes anything from the source.
//!
//! Each copy lands under a free name (`paths::non_colliding_path`, the numbering a move uses), and
//! the copy answers every path it made, the record an undo sends to the trash.
//! [`DiskFolder::copy_items_in`] reads only the content each item has at hand and names the items
//! whose content isn't, rather than leaving them out silently. [`DiskFolder::copy_in_async`]
//! awaits each item's content, so an item that has to be downloaded first is (on the caller's
//! executor, never blocking the frame), and also copies down a folder that is not on disk with
//! everything in it and its structure kept: it is read into a tree first (`copy_tree::gather_item`)
//! and then written here. `copy_task` is that copy as a task, for `WritableFolder::copy_in`.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::copy_tree::{gather_item, sanitize_file_name, write_node};
use super::folder::DiskFolder;
use super::folder_link::DiskFolderLink;
use super::paths::{non_colliding_path, path_file_name};
use crate::history::Recorded;
use crate::item::Item;
use crate::spawner::LocalTask;
use crate::translate::tr;
use crate::writable::CopyDone;

/// What a copy made and what it couldn't.
#[derive(Default)]
struct CopyProgress {
    made: Vec<PathBuf>,
    errors: Vec<String>,
    skipped: Vec<String>,
}

impl CopyProgress {
    fn finish(self, folder: &mut DiskFolder) -> (Vec<PathBuf>, Result<(), String>) {
        if !self.made.is_empty() {
            folder.reload_content();
        }
        let mut errors = self.errors;
        if !self.skipped.is_empty() {
            errors.push(format!(
                "{}: {}",
                tr("These items could not be copied"),
                self.skipped.join(", ")
            ));
        }
        let result = if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        };
        (self.made, result)
    }
}

/// Copies the folder at `from` (with everything in it) to `to`.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

impl DiskFolder {
    /// A folder on disk copied in whole; false when `item` is not a folder on disk.
    fn copy_folder_in(&self, item: &dyn Item, progress: &mut CopyProgress) -> bool {
        let Some(dir) = item
            .as_any()
            .and_then(|a| a.downcast_ref::<DiskFolderLink>())
        else {
            return false;
        };
        let from = dir.file_path().to_path_buf();
        // A folder copied into itself would copy forever.
        if self.full_path().starts_with(&from) {
            progress.skipped.push(item.name().to_string());
            return true;
        }
        let to = non_colliding_path(&self.full_path().join(path_file_name(&from)));
        match copy_dir(&from, &to) {
            Ok(()) => progress.made.push(to),
            Err(e) => {
                // A part-made copy is not left behind as if it were the item.
                let _ = std::fs::remove_dir_all(&to);
                progress.errors.push(format!("{}: {e}", item.name()));
            }
        }
        true
    }

    /// Writes one file's content under a free name.
    fn write_copy(
        &self,
        item: &dyn Item,
        file_name: &str,
        content: Option<Vec<u8>>,
        progress: &mut CopyProgress,
    ) {
        let Some(bytes) = content else {
            progress.errors.push(format!(
                "{}: {}",
                item.name(),
                tr("its content couldn't be downloaded")
            ));
            return;
        };
        let to = non_colliding_path(&self.full_path().join(file_name));
        match std::fs::write(&to, bytes) {
            Ok(()) => progress.made.push(to),
            Err(e) => progress.errors.push(format!("{}: {e}", item.name())),
        }
    }

    /// Copies `items` in, keeping the originals, with the content each item has at hand. Answers
    /// every path made and the failures; items that are neither files nor folders on disk and
    /// have no content stream are named in the error. A read-only folder copies nothing.
    pub fn copy_items_in(&mut self, items: &[Rc<dyn Item>]) -> (Vec<PathBuf>, Result<(), String>) {
        if let Err(e) = self.refuse_if_read_only() {
            return (Vec::new(), Err(e));
        }
        self.unwatched(|folder| folder.copy_items_in_now(items))
    }

    fn copy_items_in_now(&mut self, items: &[Rc<dyn Item>]) -> (Vec<PathBuf>, Result<(), String>) {
        let mut progress = CopyProgress::default();
        if let Err(e) = std::fs::create_dir_all(self.full_path()) {
            return (Vec::new(), Err(e.to_string()));
        }
        for item in items {
            if self.copy_folder_in(item.as_ref(), &mut progress) {
                continue;
            }
            match item.as_asset_stream() {
                Some(stream) => match stream.get_stream() {
                    Ok(content) => {
                        self.write_copy(item.as_ref(), &stream.file_name(), content, &mut progress)
                    }
                    Err(e) => progress
                        .errors
                        .push(format!("{}: {}", item.name(), String::from(e))),
                },
                None => progress.skipped.push(item.name().to_string()),
            }
        }
        progress.finish(self)
    }

    /// As [`Self::copy_items_in`], awaiting each item's content, so an item that has to be
    /// downloaded first is; a folder that is not on disk is copied down whole (its folders that
    /// list later through the rules' `list_for_copy`). Run on the caller's executor.
    pub async fn copy_in_async(
        &mut self,
        items: &[Rc<dyn Item>],
    ) -> (Vec<PathBuf>, Result<(), String>) {
        if let Err(e) = self.refuse_if_read_only() {
            return (Vec::new(), Err(e));
        }
        self.live.set_watch_enabled(false);
        let answer = self.copy_in_async_now(items).await;
        self.live.set_watch_enabled(true);
        answer
    }

    async fn copy_in_async_now(
        &mut self,
        items: &[Rc<dyn Item>],
    ) -> (Vec<PathBuf>, Result<(), String>) {
        let mut progress = CopyProgress::default();
        if let Err(e) = std::fs::create_dir_all(self.full_path()) {
            return (Vec::new(), Err(e.to_string()));
        }
        let rules = Rc::clone(self.rules());
        for item in items {
            if self.copy_folder_in(item.as_ref(), &mut progress) {
                continue;
            }
            if item.as_container_link().is_some() {
                let gathered =
                    gather_item(Rc::clone(item), rules.as_ref(), &mut progress.errors).await;
                if let Some(node) = gathered {
                    let to =
                        non_colliding_path(&self.full_path().join(sanitize_file_name(item.name())));
                    match write_node(&to, &node) {
                        Ok(()) => progress.made.push(to),
                        Err(e) => {
                            // A part-made copy is not left behind as if it were the folder.
                            let _ = std::fs::remove_dir_all(&to);
                            progress.errors.push(format!("{}: {e}", item.name()));
                        }
                    }
                }
                continue;
            }
            match item.as_asset_stream() {
                Some(stream) => {
                    let file_name = stream.file_name();
                    match stream.get_stream_for_person().await {
                        Ok(content) => {
                            self.write_copy(item.as_ref(), &file_name, content, &mut progress)
                        }
                        Err(e) => {
                            progress
                                .errors
                                .push(format!("{}: {}", item.name(), String::from(e)))
                        }
                    }
                }
                None => progress.skipped.push(item.name().to_string()),
            }
        }
        progress.finish(self)
    }

    /// [`Self::copy_in_async`] as a task through a folder of its own on this path and rules,
    /// which asks this folder to relist once the copy has landed (`live_refresh`'s relist flag)
    /// and hands `done` the outcome. A read-only folder's copy fails at once, inside the task.
    pub(super) fn copy_task(&self, items: Vec<Rc<dyn Item>>, done: CopyDone) -> LocalTask {
        let mut target = DiskFolder::new(self.full_path(), Rc::clone(self.rules()));
        let relist = self.live.relist_handle();
        Box::pin(async move {
            let (made, result) = target.copy_in_async(&items).await;
            if !made.is_empty() {
                relist.set(true);
            }
            done(Recorded::unrecorded(result));
        })
    }
}
