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

use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::S_OK;
use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;
use windows_sys::Win32::UI::Shell::{
    SHFileOperationW, SHQueryRecycleBinW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI,
    FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW, SHQUERYRBINFO,
};

use super::TrashedItem;

pub(super) fn trash(original: &Path) -> Result<TrashedItem, String> {
    use std::os::windows::ffi::OsStrExt;
    let root = original
        .ancestors()
        .last()
        .ok_or_else(|| format!("{}: no drive", original.display()))?
        .to_path_buf();
    // Both spellings are read while the item still exists: the long form needs it on disk.
    let mut wanted = vec![original.to_string_lossy().to_string()];
    if let Some(long) = long_path(original).filter(|l| l.as_path() != original) {
        wanted.push(long.to_string_lossy().to_string());
    }
    let items_before = recycle_bin_items(&root);
    // The source list is double-null terminated.
    let mut from: Vec<u16> = original.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        // The flags fit the struct's 16-bit field: all are below 0x10000.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        ..Default::default()
    };
    // SAFETY: `operation` is fully initialised and `from` (the only pointer it carries) outlives
    // the call; the call reads them and writes only `fAnyOperationsAborted`.
    let code = unsafe { SHFileOperationW(&mut operation) };
    if code != 0 || operation.fAnyOperationsAborted != 0 {
        return Err(format!(
            "{}: couldn't move it to the Recycle Bin (error {code})",
            original.display()
        ));
    }
    let Some((in_trash, info_file)) = find_recycled(&root, &wanted) else {
        let recycled = match (items_before, recycle_bin_items(&root)) {
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

/// `path` with every short 8.3 component spelled out (`RUNNER~1` → `runneradmin`), or `None`
/// when Windows can't say (the path is missing, or the call fails).
fn long_path(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let short: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut buffer: Vec<u16> = vec![0; 512];
    loop {
        // A path longer than u32::MAX characters can't exist; the clamp only keeps the cast exact.
        let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: `short` is null terminated and `buffer` holds `capacity` writable characters;
        // the call writes at most that many.
        let written = unsafe { GetLongPathNameW(short.as_ptr(), buffer.as_mut_ptr(), capacity) };
        let written = written as usize;
        if written == 0 {
            return None;
        }
        if written < buffer.len() {
            buffer.truncate(written);
            return Some(PathBuf::from(std::ffi::OsString::from_wide(&buffer)));
        }
        // Too small: `written` is the size needed, null included.
        buffer = vec![0; written];
    }
}

/// How many items the Recycle Bin of the drive at `root` holds, or `None` when Windows can't say.
fn recycle_bin_items(root: &Path) -> Option<i64> {
    use std::os::windows::ffi::OsStrExt;
    let root: Vec<u16> = root.as_os_str().encode_wide().chain([0]).collect();
    let mut info = SHQUERYRBINFO {
        // The struct's size, a few dozen bytes: the cast can't overflow.
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `root` is null terminated and `info` is a writable struct of the size it declares.
    let result = unsafe { SHQueryRecycleBinW(root.as_ptr(), &mut info) };
    (result == S_OK).then_some(info.i64NumItems)
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
