//! [`ZipFolderLink`]: the folder row of a `.zip` file, and of each directory inside one. It is an
//! item of the archive file (its ID and dates are the archive's, as a disk item's are) that opens
//! a [`ZipFolder`] at `current_directory` inside it. Setting the directory names the row for its
//! last segment. It is read-only and not protected. Its picture is the whole `zip_folder.png`,
//! named by `tile_icon` for the app's look to draw, as the model holds no images.

use std::any::Any;
use std::path::{Path, PathBuf};

use super::folder::ZipFolder;
use super::zip_location_key;
use crate::disk_paths::{disk_item_id, display_name, file_dates};
use crate::folder::Folder;
use crate::item::{FolderLink, Item};

/// The folder row of a `.zip` file or a directory inside one.
#[derive(Debug, Clone)]
pub struct ZipFolderLink {
    file_path: PathBuf,
    id: String,
    name: String,
    current_directory: String,
    dates: (i64, i64),
}

impl ZipFolderLink {
    /// The row for the archive at `file_path`, named `name_override` when given.
    pub fn new(file_path: impl Into<PathBuf>, name_override: Option<&str>) -> Self {
        let file_path = file_path.into();
        Self {
            id: disk_item_id(&file_path),
            name: name_override.map_or_else(|| display_name(&file_path), str::to_string),
            dates: file_dates(&file_path),
            current_directory: String::new(),
            file_path,
        }
    }

    /// The archive.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// The directory inside the archive the row opens.
    pub fn current_directory(&self) -> &str {
        &self.current_directory
    }

    /// Sets the directory: also names the row for the directory's last segment (after the last
    /// '/').
    pub fn set_current_directory(&mut self, value: String) {
        self.name = value.rsplit('/').next().unwrap_or(&value).to_string();
        self.current_directory = value;
    }
}

impl Item for ZipFolderLink {
    fn id(&self) -> &str {
        &self.id
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
    fn date_created(&self) -> i64 {
        self.dates.0
    }
    fn date_modified(&self) -> i64 {
        self.dates.1
    }
    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        Some(self)
    }
    fn into_container_link(self: std::rc::Rc<Self>) -> Option<std::rc::Rc<dyn FolderLink>> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl FolderLink for ZipFolderLink {
    /// The archive at this row's directory (not yet loaded).
    fn get_container(&self) -> Result<Box<dyn Folder>, String> {
        Ok(Box::new(ZipFolder::new(
            self.file_path.to_string_lossy(),
            self.current_directory.clone(),
        )))
    }
    fn is_read_only(&self) -> bool {
        true
    }
    fn tile_icon(&self) -> Option<&str> {
        Some("zip_folder.png")
    }
    /// The archive and the directory inside it, as the container it opens answers.
    fn location_key(&self) -> Option<String> {
        Some(zip_location_key(&self.file_path, &self.current_directory))
    }
}
