//! A [`DiskFolder`] searching itself: walks every directory under the folder for the files its
//! rules list whose names match the query ([`file_name_contains_filter`]) and the folders whose
//! names match it (matched whole, reported as a [`DiskFolderLink`] that opens the folder with the
//! same rules), reporting them in batches. A zip file the listing shows as a folder
//! (`paths::lists_as_folder`, with the `zip` feature) is found as that folder row, by the name the
//! row shows, so the listing and the search agree. The folder's own listing is untouched: a
//! search across folders must not disturb the folder the user is looking at.
//!
//! The walk is a local future on the search's executor (the app's, pumped each frame), so it
//! yields after every directory to keep the frame moving. The order is a directory's matching
//! sub-directories, its matching zip folders, then its files, then each sub-directory walked,
//! depth first, each sorted by
//! path so results come in a stable order. Hidden folders ('.git'), '$' system folders, links
//! (how a walk loops or wanders off the drive) and, on Windows, hidden or system folders are
//! neither reported nor descended into.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::file::DiskFile;
use super::folder::{is_readable_directory, DiskFolder};
use super::folder_link::DiskFolderLink;
use super::paths::lists_as_folder;
use crate::item::Item;
use crate::search::{yield_now, SearchCancel, SearchFuture, SearchProvider, SearchReport};
use crate::search_terms::name_matches;

/// Matches collected before a batch is handed over. Small enough that the first hits show while
/// a big drive is still walked, large enough that a folder of matches is not one notification
/// per file.
pub const SEARCH_BATCH_SIZE: usize = 25;

/// How deep below the folder a search descends. Stops a directory junction loop from walking
/// forever.
pub const MAX_SEARCH_DEPTH: usize = 16;

/// Every space separated term of `filter` appears in the file name of `filename` (a name or a
/// path), extension excluded, so "stl" does not match every mesh and a folder name on the path
/// does not match at all.
pub fn file_name_contains_filter(filename: &str, filter: &str) -> bool {
    name_matches(file_name_without_extension(filename), filter, None)
}

/// The file name without its extension: the last path segment, up to its last dot.
fn file_name_without_extension(path: &str) -> &str {
    let name = path
        .rsplit(|c| c == '/' || (cfg!(windows) && c == '\\'))
        .next()
        .unwrap_or(path);
    match name.rfind('.') {
        Some(dot) => &name[..dot],
        None => name,
    }
}

impl SearchProvider for DiskFolder {
    /// Searches for `query`: a blank query or a cancelled token reports nothing.
    fn search_async<'a>(
        &'a self,
        query: &str,
        report: SearchReport,
        cancel: SearchCancel,
    ) -> SearchFuture<'a> {
        let filter = query.trim().to_string();
        let root = self.full_path().to_path_buf();
        let rules = Rc::clone(self.rules());
        Box::pin(async move {
            if filter.is_empty() || cancel.is_cancelled() {
                return Ok(());
            }
            let mut batch = Vec::new();
            // Depth first: a stack of (directory, depth), children pushed in reverse so they pop
            // in listing order.
            let mut stack = vec![(root, 0usize)];
            while let Some((directory, depth)) = stack.pop() {
                if cancel.is_cancelled() {
                    return Ok(());
                }
                if depth > MAX_SEARCH_DEPTH || !is_readable_directory(&directory) {
                    continue;
                }
                let (files, sub_directories) = list(&directory);
                let sub_directories: Vec<PathBuf> = sub_directories
                    .into_iter()
                    .filter(|sub_directory| should_descend_into(sub_directory))
                    .collect();
                // A folder is named whole: it has no extension to drop.
                for sub_directory in &sub_directories {
                    let name = sub_directory
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if name_matches(&name, filter.as_str(), None) {
                        let link = DiskFolderLink::new(sub_directory.clone(), Rc::clone(&rules));
                        batch.push(Rc::new(link) as Rc<dyn Item>);
                        if batch.len() >= SEARCH_BATCH_SIZE {
                            flush_batch(&report, &mut batch, &cancel);
                        }
                    }
                }
                // A zip file the listing shows as a folder is found as that folder row.
                for file in files.iter().filter(|file| lists_as_folder(file)) {
                    if let Some(link) = zip_folder_result(file, &filter) {
                        batch.push(link);
                        if batch.len() >= SEARCH_BATCH_SIZE {
                            flush_batch(&report, &mut batch, &cancel);
                        }
                    }
                }
                for file in files {
                    if cancel.is_cancelled() {
                        return Ok(());
                    }
                    let file_name = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if file_name_contains_filter(&file_name, &filter)
                        && rules.lists_file(&file_name)
                        && !lists_as_folder(&file)
                    {
                        batch.push(Rc::new(DiskFile::new(file)) as Rc<dyn Item>);
                        if batch.len() >= SEARCH_BATCH_SIZE {
                            flush_batch(&report, &mut batch, &cancel);
                        }
                    }
                }
                for sub_directory in sub_directories.into_iter().rev() {
                    stack.push((sub_directory, depth + 1));
                }
                yield_now().await;
            }
            // Whatever the last directory left short of a full batch.
            flush_batch(&report, &mut batch, &cancel);
            Ok(())
        })
    }
}

/// The folder row of a zip file that lists as a folder, when the name that row shows matches
/// `filter` (whole, as a folder's name is).
#[cfg(feature = "zip")]
fn zip_folder_result(file: &Path, filter: &str) -> Option<Rc<dyn Item>> {
    let link = crate::zip_folder::ZipFolderLink::new(file, None);
    name_matches(link.name(), filter, None).then(|| Rc::new(link) as Rc<dyn Item>)
}

/// Without the `zip` feature no file lists as a folder.
#[cfg(not(feature = "zip"))]
fn zip_folder_result(_file: &Path, _filter: &str) -> Option<Rc<dyn Item>> {
    None
}

/// A directory's files and sub-directories, each sorted by path, and nothing for one the file
/// system refuses: a folder that cannot be read has no results in it.
fn list(directory: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut directories = Vec::new();
    if let Ok(entries) = std::fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => directories.push(path),
                Ok(kind) if kind.is_file() => files.push(path),
                // A link to a file is a file, listed with the others; to a folder it is a link,
                // which `should_descend_into` skips anyway.
                Ok(kind) if kind.is_symlink() && path.is_file() => files.push(path),
                _ => {}
            }
        }
    }
    files.sort();
    directories.sort();
    (files, directories)
}

/// Not hidden ('.git'), not a '$' system folder, not a link, and not hidden or system by
/// attribute on Windows.
fn should_descend_into(directory: &Path) -> bool {
    let name = directory
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.starts_with('.') || name.starts_with('$') {
        return false;
    }
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) => !metadata.file_type().is_symlink() && !hidden_or_system(&metadata),
        // A directory whose attributes cannot be read is not one the walk can list.
        Err(_) => false,
    }
}

#[cfg(windows)]
fn hidden_or_system(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    const REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & (HIDDEN | SYSTEM | REPARSE_POINT) != 0
}

#[cfg(not(windows))]
fn hidden_or_system(_metadata: &std::fs::Metadata) -> bool {
    // Hidden is the leading dot, checked above; there is no system attribute.
    false
}

/// Hands the collected matches over and starts a new batch; nothing once cancelled.
fn flush_batch(report: &SearchReport, batch: &mut Vec<Rc<dyn Item>>, cancel: &SearchCancel) {
    if batch.is_empty() || cancel.is_cancelled() {
        return;
    }
    report(std::mem::take(batch));
}
