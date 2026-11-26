//! The Windows Recycle Bin: `SHFileOperationW` with `FOF_ALLOWUNDO` recycles the item (Explorer's
//! Restore works on it). That call does not say where the item went, so the item is then found in
//! the drive's `$Recycle.Bin`: each recycled item is a `$R...` file or folder beside an `$I...`
//! record holding its original path and deletion time; the newest record naming our path is ours
//! (`super::newest_recycle_bin_match`). The record holds the path's long form, so a path given in
//! its short 8.3 form (`C:\Users\RUNNER~1\...`, common in `%TEMP%`) is looked up by both spellings.
//!
//! `FOF_ALLOWUNDO` only recycles "if possible": on a drive with no Recycle Bin, or for an item too
//! large for it, Windows deletes it permanently and still reports success. The Recycle Bin's item
//! count, read before and after, tells the two apart, so the caller is never told an item is in
//! the Recycle Bin when Windows erased it.
//!
//! The three Windows calls (the recycle, a path's long form, the item count) are agg-os's
//! `windows::recycle_bin`, the workspace's home for unsafe code; the search and the messages are
//! here.

use std::path::{Path, PathBuf};

use agg_os::windows::recycle_bin::{long_path_name, recycle, recycle_bin_item_count, RecycleError};

use super::TrashedItem;

pub(super) fn trash(original: &Path) -> Result<TrashedItem, String> {
    let root = original
        .ancestors()
        .last()
        .ok_or_else(|| format!("{}: no drive", original.display()))?
        .to_path_buf();
    // Both spellings are read while the item still exists: the long form needs it on disk.
    let mut wanted = vec![original.to_string_lossy().to_string()];
    if let Some(long) = long_path_name(original).filter(|l| l.as_path() != original) {
        wanted.push(long.to_string_lossy().to_string());
    }
    let items_before = recycle_bin_item_count(&root);
    match recycle(original) {
        Ok(()) => {}
        Err(RecycleError::Failed { code, .. }) => {
            return Err(format!(
                "{}: couldn't move it to the Recycle Bin (error {code})",
                original.display()
            ));
        }
        Err(RecycleError::InteriorNul) => {
            return Err(format!(
                "{}: couldn't move it to the Recycle Bin (its path holds a NUL character)",
                original.display()
            ));
        }
        Err(other) => {
            return Err(format!(
                "{}: couldn't move it to the Recycle Bin ({other})",
                original.display()
            ));
        }
    }
    let Some((in_trash, info_file)) = find_recycled(&root, &wanted) else {
        let recycled = match (items_before, recycle_bin_item_count(&root)) {
            (Some(before), Some(after)) => after > before,
            // Without counts nothing says it was erased; the plainer claim is that it went in.
            _ => true,
        };
        return Err(if recycled {
            format!(
                "{}: moved to the Recycle Bin, but MatterCAD couldn't find it there to put back; \
                 restore it from the Recycle Bin",
                original.display()
            )
        } else {
            format!(
                "{}: Windows deleted it permanently instead of moving it to the Recycle Bin \
                 (this drive has no Recycle Bin, or the item is too large for it), so it can't \
                 be put back",
                original.display()
            )
        });
    };
    Ok(TrashedItem {
        original: original.to_path_buf(),
        in_trash,
        info_file: Some(info_file),
    })
}

/// The `$R` item and `$I` record of the newest recycle of the path spelled `wanted`, searched in
/// the `$Recycle.Bin` of the drive at `root`.
fn find_recycled(root: &Path, wanted: &[String]) -> Option<(PathBuf, PathBuf)> {
    let bin = root.join("$Recycle.Bin");
    // One folder per user (named by the user's SID); others' folders are unreadable and skipped.
    let records = std::fs::read_dir(&bin)
        .ok()?
        .flatten()
        .filter_map(|user| std::fs::read_dir(user.path()).ok())
        .flat_map(|entries| entries.flatten())
        // Only the small `$I` records are read, never the recycled `$R` items themselves.
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("$I"))
        .filter_map(|entry| Some((entry.path(), std::fs::read(entry.path()).ok()?)));
    super::newest_recycle_bin_match(records, wanted)
}
