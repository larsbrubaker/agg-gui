//! [`DiskFolderLink`]: a sub-folder's row in a folder on disk - the link a [`super::DiskFolder`]
//! lists for each folder inside it, which opens a `DiskFolder` on that folder with the same
//! [`super::DiskRules`], and renames the folder beside where it is ([`DiskFolderLink::set_name`]).
//! Like its folder it lives on the computer ([`PlaceId::COMPUTER`]) and is known by its path
//! (`paths::disk_location_key`), so a folder and its row answer the same key.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::folder::DiskFolder;
use super::paths::{disk_item_id, disk_location_key, display_name, file_dates};
use super::rules::DiskRules;
use crate::folder::Folder;
use crate::item::{FolderLink, Item};
use crate::place::PlaceId;

/// A sub-folder's row in a folder on disk.
#[derive(Clone)]
pub struct DiskFolderLink {
    file_path: PathBuf,
    id: String,
    name: String,
    use_incremented_name_during_type_change: bool,
    /// The folder's creation and last write times.
    dates: (i64, i64),
    /// The rules the folder it opens lists and allows changes by.
    rules: Rc<dyn DiskRules>,
}

impl DiskFolderLink {
    /// The row for the folder at `path`, whose folder follows `rules`.
    pub fn new(path: impl Into<PathBuf>, rules: Rc<dyn DiskRules>) -> Self {
        let file_path = path.into();
        let dates = file_dates(&file_path);
        Self {
            id: disk_item_id(&file_path),
            name: display_name(&file_path),
            file_path,
            use_incremented_name_during_type_change: false,
            dates,
            rules,
        }
    }

    /// Sets `use_incremented_name_during_type_change`, passed on to the folder it opens.
    pub fn with_incremented_name_during_type_change(mut self, value: bool) -> Self {
        self.use_incremented_name_during_type_change = value;
        self
    }

    /// Sets the name the link shows, which can differ from the folder's own name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The folder's path.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// The rules the folder it opens follows.
    pub fn rules(&self) -> &Rc<dyn DiskRules> {
        &self.rules
    }

    /// Renames the folder to `value` beside where it is, numbering the name " (1)", " (2)", ...
    /// while a folder has it. Nothing happens when the name is the same or the folder is gone.
    /// Answers the move's error when the disk refuses it.
    pub fn set_name(&mut self, value: &str) -> Result<(), String> {
        if self.name == value || !self.file_path.is_dir() {
            return Ok(());
        }
        let source = self.file_path.clone();
        let parent_dir = source.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut dest = parent_dir.join(value);
        let mut increment = 0;
        while dest.is_dir() {
            increment += 1;
            dest = parent_dir.join(format!("{value} ({increment})"));
            if source == dest {
                break;
            }
        }
        if source != dest {
            std::fs::rename(&source, &dest).map_err(|e| format!("{}: {e}", source.display()))?;
            self.file_path = dest;
            self.id = disk_item_id(&self.file_path);
            self.name = display_name(&self.file_path);
        }
        Ok(())
    }
}

impl Item for DiskFolderLink {
    fn id(&self) -> &str {
        &self.id
    }

    fn date_created(&self) -> i64 {
        self.dates.0
    }

    fn date_modified(&self) -> i64 {
        self.dates.1
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_protected(&self) -> bool {
        false
    }

    fn is_visible(&self) -> bool {
        true
    }

    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        Some(self)
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl FolderLink for DiskFolderLink {
    /// The folder's container, named as the row is, following the row's rules.
    fn get_container(&self) -> Result<Box<dyn Folder>, String> {
        Ok(Box::new(
            DiskFolder::new(&self.file_path, self.rules.clone())
                .with_incremented_name_during_type_change(
                    self.use_incremented_name_during_type_change,
                )
                .with_name(self.name.clone()),
        ))
    }

    /// Read-only when its rules are.
    fn is_read_only(&self) -> bool {
        self.rules.read_only()
    }

    /// A folder on disk is on the computer.
    fn place(&self) -> Option<PlaceId> {
        Some(PlaceId::COMPUTER)
    }

    /// The folder's path, as its folder answers it.
    fn location_key(&self) -> Option<String> {
        Some(disk_location_key(&self.file_path))
    }
}
