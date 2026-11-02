//! Whether a folder on disk has folders of its own, for a folder tree's expand arrow, found from
//! the folder's names alone: what [`super::DiskFolder`]'s `load` would list as folders (with the
//! `zip` feature, zip files too), without
//! its per-file lookups (type, size, dates), which in a folder of thousands of files cost far
//! more. A network share is never read for this: its answer is unknown until the folder is
//! opened.

use std::path::{Component, Path, Prefix};

use super::folder::is_readable_directory;
use super::paths::lists_as_folder;

/// Whether `dir` is on a network share (`\\server\share`), where even listing names can stall.
fn is_network_share(dir: &Path) -> bool {
    matches!(
        dir.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..))
    )
}

/// Whether `load` would list a folder in `dir`, found by reading the folder's names only up to
/// the first sub-folder (or, with the `zip` feature, zip file). A folder `load` shows a message for (restricted, missing, unreadable)
/// lists none. `None` for a network share, which is not read.
pub fn folder_has_child_containers(dir: &Path) -> Option<bool> {
    if is_network_share(dir) {
        return None;
    }
    if !is_readable_directory(dir) {
        return Some(false);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Some(false);
    };
    for entry in entries {
        // An entry that cannot be read fails `load`'s whole listing, which then lists no folders.
        let Ok(entry) = entry else {
            return Some(false);
        };
        // The entry's own type needs no further look at the disk; a link is followed, as `load`
        // follows it.
        let path = entry.path();
        let found = match entry.file_type() {
            Ok(kind) if kind.is_dir() => true,
            Ok(kind) if kind.is_symlink() => path.is_dir() || lists_as_folder(&path),
            _ => lists_as_folder(&path),
        };
        if found {
            return Some(true);
        }
    }
    Some(false)
}
