//! [`DiskFile`]: a file on disk as an item - its path, shown name, ID (`paths::disk_item_id`),
//! dates, size, content type (its lower-case extension) and bytes. The folder rows beside it are
//! `folder_link`'s [`super::DiskFolderLink`].
//!
//! A file's rename ([`DiskFile::set_name`]) keeps the extension, numbers the name when another
//! file has it, and raises the file-renamed event so whatever keeps a file by path (an app's
//! favorites) follows it. The event's listeners are per thread ([`subscribe_file_renamed`]), as a
//! browser is driven from its UI thread.

use std::any::Any;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::paths::{
    disk_item_id, display_name, extension_lower, extension_with_dot, file_dates, path_file_name,
    without_extension,
};
use crate::error::LoadError;
use crate::item::{Asset, AssetStream, Item};

/// A `FileRenamed` listener: called with the old and the new path.
pub type FileRenamedListener = Rc<dyn Fn(&Path, &Path)>;

thread_local! {
    static FILE_RENAMED: RefCell<Vec<FileRenamedListener>> = const { RefCell::new(Vec::new()) };
}

/// Subscribes to file renames: `listener` runs after any [`DiskFile`] on this thread renames its
/// file. Per thread rather than per item because the item renamed is often not the one that kept
/// the path: a browser row and a favorite are separate items for one file.
pub fn subscribe_file_renamed(listener: FileRenamedListener) {
    FILE_RENAMED.with(|l| l.borrow_mut().push(listener));
}

fn raise_file_renamed(old_path: &Path, new_path: &Path) {
    // Copied first, so a listener may subscribe another.
    let listeners = FILE_RENAMED.with(|l| l.borrow().clone());
    for listener in listeners {
        listener(old_path, new_path);
    }
}

/// Whether `dest` names a file other than `source`. On a case-insensitive disk (mac, Windows)
/// "crate.mcx" exists when only "Crate.mcx" - the file being renamed - is there, which would
/// number a case-only rename away to "crate (1)". The listing tells the two apart: it has the
/// exact name only when a second file really carries it.
fn is_taken_by_another_file(dest: &Path, source: &Path) -> bool {
    // The same name again (a favorite renamed to what it already shows) is no rename at all.
    if !dest.is_file() || dest == source {
        return false;
    }
    if dest.to_string_lossy().to_lowercase() != source.to_string_lossy().to_lowercase() {
        return true;
    }
    let exact = path_file_name(dest);
    dest.parent()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .is_some_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy() == exact)
        })
}

/// A file on disk.
#[derive(Debug, Clone)]
pub struct DiskFile {
    file_path: PathBuf,
    asset_path: String,
    id: String,
    name: String,
    content_type: String,
    file_size: u64,
    /// The file's creation and last write times (`paths::file_dates`).
    dates: (i64, i64),
}

impl DiskFile {
    /// The item for the file at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let file_path = path.into();
        let asset_path = file_path.to_string_lossy().into_owned();
        Self {
            id: disk_item_id(&file_path),
            name: display_name(&file_path),
            content_type: extension_lower(&asset_path),
            file_size: std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0),
            dates: file_dates(&file_path),
            asset_path,
            file_path,
        }
    }

    /// The file's path.
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Renames the file to `value` in its folder, keeping its extension (`value` may end in it
    /// already) and numbering the name " (1)", " (2)", ... while another file has it; then
    /// raises the file-renamed event. Nothing happens when the name is the same or the file is
    /// gone. Answers the move's error when the disk refuses it.
    pub fn set_name(&mut self, value: &str) -> Result<(), String> {
        if self.name == value || !self.file_path.is_file() {
            return Ok(());
        }
        let source = self.file_path.clone();
        let extension = extension_with_dot(&source);
        let base_name = without_extension(value, &extension).to_string();
        let directory = source.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut dest = directory.join(format!("{base_name}{extension}"));
        let mut increment = 0;
        while is_taken_by_another_file(&dest, &source) {
            increment += 1;
            dest = directory.join(format!("{base_name} ({increment}){extension}"));
            if source == dest {
                break;
            }
        }
        if source != dest {
            std::fs::rename(&source, &dest).map_err(|e| format!("{}: {e}", source.display()))?;
            *self = DiskFile::new(&dest);
            raise_file_renamed(&source, &dest);
        }
        Ok(())
    }

    /// The file's length in bytes when the item was made (0 when missing).
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

impl Item for DiskFile {
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

    fn as_asset(&self) -> Option<&dyn Asset> {
        Some(self)
    }

    fn as_asset_stream(&self) -> Option<&dyn AssetStream> {
        Some(self)
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl Asset for DiskFile {
    fn content_type(&self) -> &str {
        &self.content_type
    }

    /// A disk file has no category.
    fn category(&self) -> &str {
        ""
    }

    fn file_name(&self) -> String {
        path_file_name(&self.file_path)
    }

    fn asset_path(&self) -> &str {
        &self.asset_path
    }
}

impl AssetStream for DiskFile {
    /// The file's bytes, `None` when no file is there (a folder, or a file gone since listing).
    fn get_stream(&self) -> Result<Option<Vec<u8>>, LoadError> {
        if self.file_path.is_file() {
            std::fs::read(&self.file_path)
                .map(Some)
                .map_err(|e| LoadError::Failed(format!("{}: {e}", self.asset_path)))
        } else {
            Ok(None)
        }
    }
}
