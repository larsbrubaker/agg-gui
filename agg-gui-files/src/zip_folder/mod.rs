//! A `.zip` file browsed as a read-only folder (the `zip` feature). [`ZipFolderLink`] is the
//! folder row of an archive, or of a directory inside one; [`ZipFolder`] the folder it opens,
//! listing one directory level of the archive; [`ZipEntry`] one file inside the archive, read out
//! of it on demand. With the `disk` feature too, a folder on disk lists each zip file as a
//! `ZipFolderLink` (`lists_as_zip_folder`).
//!
//! An archive keeps its items: it answers no place, so what leaves it is a copy and nothing lands
//! in it. A directory inside an archive is known by [`zip_location_key`]: the archive and the
//! directory, the same from its folder and its row.

mod entry;
mod folder;
mod link;

use std::path::{Path, PathBuf};

pub use entry::ZipEntry;
pub use folder::ZipFolder;
pub use link::ZipFolderLink;

use crate::disk_paths::disk_location_key;

/// A directory inside a zip archive's location key: the archive's key, a NUL, then the directory
/// ("" for the archive's top level). A folder on disk's key never holds a raw NUL, so this never
/// equals one. The archive is keyed by its lossy text, on both sides: a [`ZipFolder`] keeps only
/// that text of its archive's path, so its row is keyed the same way to match it.
pub fn zip_location_key(archive: &Path, directory: &str) -> String {
    let as_held = PathBuf::from(archive.to_string_lossy().into_owned());
    format!("{}\0{directory}", disk_location_key(&as_held))
}

/// Whether a file lists as a folder ([`ZipFolderLink`]) in a folder on disk: its extension holds
/// ".zip" (ignoring case).
pub fn lists_as_zip_folder(file: &Path) -> bool {
    file.extension().is_some_and(|e| {
        format!(".{}", e.to_string_lossy())
            .to_lowercase()
            .contains(".zip")
    })
}
