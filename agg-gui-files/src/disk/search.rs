//! A [`DiskFolder`] searching itself: walks every directory under the folder for the files its
//! rules list whose names match the query ([`file_name_contains_filter`]), reporting them in
//! batches. The folder's own listing is untouched: a search across folders must not disturb the
//! folder the user is looking at.
//!
//! The walk is a local future on the search's executor (the app's, pumped each frame), so it
//! yields after every directory to keep the frame moving. The order is a directory's files, then
//! each sub-directory, depth first, each sorted by path so results come in a stable order. Hidden
//! folders ('.git'), '$' system folders, links (how a walk loops or wanders off the drive) and,
//! on Windows, hidden or system folders are not descended into.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::file::DiskFile;
use super::folder::{is_readable_directory, DiskFolder};
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
                    {
                        batch.push(Rc::new(DiskFile::new(file)) as Rc<dyn Item>);
                        if batch.len() >= SEARCH_BATCH_SIZE {
                            flush_batch(&report, &mut batch, &cancel);
                        }
                    }
                }
                for sub_directory in sub_directories.into_iter().rev() {
                    if should_descend_into(&sub_directory) {
                        stack.push((sub_directory, depth + 1));
                    }
                }
                yield_now().await;
            }
            // Whatever the last directory left short of a full batch.
            flush_batch(&report, &mut batch, &cancel);
            Ok(())
        })
    }
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
