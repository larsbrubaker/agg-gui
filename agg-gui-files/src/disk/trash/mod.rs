//! Recoverable deletes for folders on disk: a remove sends files and folders to the operating
//! system's Trash (macOS), Recycle Bin (Windows) or freedesktop Trash (Linux and other Unix
//! desktops) instead of erasing them, and remembers where each one went so an undo can put it
//! back (`restore_from_trash`). The per-system moves are `macos.rs`, `windows.rs` and
//! `freedesktop.rs`; `DiskFolder::remove_to_trash` (`folder_writes`) is the caller. The
//! freedesktop step and the Recycle Bin record reader and matcher (`parse_recycle_bin_record`,
//! `newest_recycle_bin_match`) are plain file work, so they compile on every Unix and every host
//! respectively, where their tests run.
//!
//! A trash directory set with `set_trash_directory` replaces the system's for the whole process:
//! tests point it at a temporary folder so they never fill the developer's real Trash. The browser
//! build has no disk folders, so there is nothing to trash there and every call fails.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(unix)]
mod freedesktop;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// A file or folder that went to the trash: where it was, and where it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrashedItem {
    /// The path it had before the delete, and gets back on restore.
    pub original: PathBuf,
    /// The path of the item inside the trash.
    pub in_trash: PathBuf,
    /// The trash's record of the item (a freedesktop `.trashinfo` or a Recycle Bin `$I` file),
    /// removed on restore so the trash no longer lists it.
    pub info_file: Option<PathBuf>,
}

/// The process-wide replacement for the system trash (`None`: the system's).
static TRASH_DIRECTORY: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Sends every later trash in this process to `directory` instead of the system trash (`None`
/// goes back to the system's). Items get unique names inside it, so tests sharing one directory
/// do not collide.
pub fn set_trash_directory(directory: Option<PathBuf>) {
    let mut slot = TRASH_DIRECTORY.lock().unwrap_or_else(|e| e.into_inner());
    *slot = directory;
}

fn trash_directory() -> Option<PathBuf> {
    TRASH_DIRECTORY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Moves `path` into the freedesktop trash rooted at `root` (its `files` and `info` folders, with
/// a `.trashinfo` record): the Linux system trash's own step, open to any Unix for its tests.
#[cfg(unix)]
pub fn move_to_freedesktop_trash(path: &Path, root: &Path) -> Result<TrashedItem, String> {
    let original = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
    freedesktop::trash_into(&original, root)
}

/// Moves the file or folder at `path` to the system trash, ignoring `set_trash_directory`.
pub fn move_to_system_trash(path: &Path) -> Result<TrashedItem, String> {
    let original = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
    system_trash(&original)
}

/// Moves the file or folder at `path` to the trash and answers where it went.
pub fn move_to_trash(path: &Path) -> Result<TrashedItem, String> {
    let original = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if std::fs::symlink_metadata(&original).is_err() {
        return Err(format!("{}: not found", original.display()));
    }
    if let Some(directory) = trash_directory() {
        return move_into_directory(&original, &directory);
    }
    system_trash(&original)
}

#[cfg(target_os = "macos")]
fn system_trash(original: &Path) -> Result<TrashedItem, String> {
    macos::trash(original)
}

#[cfg(windows)]
fn system_trash(original: &Path) -> Result<TrashedItem, String> {
    windows::trash(original)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn system_trash(original: &Path) -> Result<TrashedItem, String> {
    freedesktop::trash(original)
}

#[cfg(not(any(unix, windows)))]
fn system_trash(original: &Path) -> Result<TrashedItem, String> {
    Err(format!("{}: this system has no trash", original.display()))
}

/// Puts a trashed item back at its original path. Refuses when something new already has that
/// path, so a restore never overwrites the user's work.
pub fn restore_from_trash(item: &TrashedItem) -> Result<(), String> {
    if std::fs::symlink_metadata(&item.original).is_ok() {
        return Err(format!(
            "{}: an item with this name is already there",
            item.original.display()
        ));
    }
    if let Some(parent) = item.original.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::rename(&item.in_trash, &item.original)
        .map_err(|e| format!("{}: {e}", item.original.display()))?;
    if let Some(info) = &item.info_file {
        // The item is back; a leftover record would only list a missing file in the trash.
        let _ = std::fs::remove_file(info);
    }
    Ok(())
}

/// The trash as a plain folder (the test directory): the item moves in under a name no other
/// item there has.
fn move_into_directory(original: &Path, directory: &Path) -> Result<TrashedItem, String> {
    std::fs::create_dir_all(directory).map_err(|e| format!("{}: {e}", directory.display()))?;
    let name = original
        .file_name()
        .ok_or_else(|| format!("{}: no name", original.display()))?;
    let in_trash = unique_path(directory, Path::new(name));
    std::fs::rename(original, &in_trash).map_err(|e| format!("{}: {e}", original.display()))?;
    Ok(TrashedItem {
        original: original.to_path_buf(),
        in_trash,
        info_file: None,
    })
}

/// `directory/name`, or `directory/<stem> 2<.ext>`, `... 3`, ... when that is taken.
fn unique_path(directory: &Path, name: &Path) -> PathBuf {
    (1u64..)
        .map(|n| directory.join(numbered_name(name, n)))
        .find(|candidate| std::fs::symlink_metadata(candidate).is_err())
        .unwrap_or_else(|| directory.join(name))
}

/// `name` for `n` = 1, else `<stem> <n><.ext>`.
pub(super) fn numbered_name(name: &Path, n: u64) -> PathBuf {
    if n == 1 {
        return name.to_path_buf();
    }
    let stem = name.file_stem().unwrap_or_default().to_string_lossy();
    let extension = name
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    PathBuf::from(format!("{stem} {n}{extension}"))
}

/// An `$I` record's deletion time and original path. Version 1 holds the path in a fixed
/// 260-character field; version 2 (Windows 10 and later) as a length then the characters.
pub fn parse_recycle_bin_record(bytes: &[u8]) -> Option<(u64, String)> {
    let u64_at = |at: usize| -> Option<u64> {
        Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
    };
    let version = u64_at(0)?;
    let deleted = u64_at(16)?;
    let (start, chars) = match version {
        1 => (24, 260),
        2 => {
            let len = u32::from_le_bytes(bytes.get(24..28)?.try_into().ok()?);
            (28, len as usize)
        }
        _ => return None,
    };
    let units: Vec<u16> = bytes
        .get(start..start + chars * 2)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    Some((deleted, String::from_utf16_lossy(&units)))
}

/// Of a Recycle Bin's `$I` records (each record file's path and bytes), the one recording the
/// newest recycle of the path whose spellings are `wanted`: that record and the `$R` item beside
/// it. One path has several spellings on Windows (the short `RUNNER~1` form a caller may hold and
/// the long form the Recycle Bin records), and Windows paths ignore case, so a record matches
/// when its path equals any of `wanted` without case. Two records of one path with the same
/// deletion time (never seen in practice) resolve to the record path that sorts last, so the
/// answer does not depend on the order the folder lists its files.
pub fn newest_recycle_bin_match(
    records: impl IntoIterator<Item = (PathBuf, Vec<u8>)>,
    wanted: &[String],
) -> Option<(PathBuf, PathBuf)> {
    let wanted: Vec<String> = wanted.iter().map(|w| w.to_lowercase()).collect();
    let mut best: Option<(u64, PathBuf)> = None;
    for (record, bytes) in records {
        // `$I...` are the records; the `$R...` items beside them and `desktop.ini` are not.
        let is_record = record
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("$I"));
        if !is_record {
            continue;
        }
        let Some((deleted, path)) = parse_recycle_bin_record(&bytes) else {
            continue;
        };
        if !wanted.contains(&path.to_lowercase()) {
            continue;
        }
        let newer = best
            .as_ref()
            .is_none_or(|(time, at)| (deleted, &record) > (*time, at));
        if newer {
            best = Some((deleted, record));
        }
    }
    let (_, record) = best?;
    let name = record.file_name()?.to_str()?;
    let item = record.with_file_name(format!("$R{}", name.strip_prefix("$I")?));
    Some((item, record))
}
