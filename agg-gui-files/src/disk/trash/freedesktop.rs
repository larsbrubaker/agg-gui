//! The freedesktop.org Trash (Linux and other Unix desktops): the item moves into
//! `$XDG_DATA_HOME/Trash/files` (default `~/.local/share/Trash`) beside a
//! `Trash/info/<name>.trashinfo` record of its original path and deletion time, which file managers
//! read to list it and restore it. An item on another drive than the home folder goes to that
//! drive's `$topdir/.Trash-$uid` instead, since a rename cannot cross drives and copying would make
//! a delete slow and fallible.

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use super::TrashedItem;

#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(super) fn trash(original: &Path) -> Result<TrashedItem, String> {
    let home_trash = home_trash().ok_or_else(|| "No home folder to hold the Trash".to_string())?;
    let item_device = std::fs::symlink_metadata(original)
        .map_err(|e| format!("{}: {e}", original.display()))?
        .dev();
    let home_device = existing_ancestor_device(&home_trash);
    let trash_root = if home_device == Some(item_device) {
        home_trash
    } else {
        let top = mount_top(original, item_device);
        let uid = std::fs::metadata(home_dir().unwrap_or_default())
            .map(|m| m.uid())
            .map_err(|e| format!("{}: {e}", original.display()))?;
        top.join(format!(".Trash-{uid}"))
    };
    trash_into(original, &trash_root)
}

/// Moves `original` into the trash rooted at `root` (its `files` and `info` folders).
pub(super) fn trash_into(original: &Path, root: &Path) -> Result<TrashedItem, String> {
    let files = root.join("files");
    let info = root.join("info");
    for dir in [root, &files, &info] {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let name = original
        .file_name()
        .ok_or_else(|| format!("{}: no name", original.display()))?;
    // The record is created first, exclusively, so its name reserves the item's name in `files`.
    let (stored, info_file, mut record) = (1u64..)
        .map(|n| {
            let stored = super::numbered_name(Path::new(name), n);
            let info_file = info.join(format!("{}.trashinfo", stored.to_string_lossy()));
            (stored, info_file)
        })
        .take(1000)
        .find_map(|(stored, info_file)| {
            if std::fs::symlink_metadata(files.join(&stored)).is_ok() {
                return None;
            }
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&info_file)
                .ok()
                .map(|f| (stored, info_file, f))
        })
        .ok_or_else(|| format!("{}: no free name in the Trash", original.display()))?;
    let text = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode(original),
        deletion_date(web_time::SystemTime::now()),
    );
    let written = record.write_all(text.as_bytes());
    drop(record);
    let in_trash = files.join(&stored);
    let moved = written
        .map_err(|e| format!("{}: {e}", info_file.display()))
        .and_then(|()| {
            std::fs::rename(original, &in_trash).map_err(|e| format!("{}: {e}", original.display()))
        });
    if let Err(e) = moved {
        let _ = std::fs::remove_file(&info_file);
        return Err(e);
    }
    Ok(TrashedItem {
        original: original.to_path_buf(),
        in_trash,
        info_file: Some(info_file),
    })
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn home_trash() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".local/share")))?;
    Some(data.join("Trash"))
}

/// The device of the nearest existing folder at or above `path` (the Trash may not exist yet).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn existing_ancestor_device(path: &Path) -> Option<u64> {
    path.ancestors()
        .find_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.dev())
}

/// The top folder of the drive holding `path`: the highest ancestor still on `device`.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn mount_top(path: &Path, device: u64) -> PathBuf {
    let mut top = path.parent().unwrap_or(path).to_path_buf();
    for ancestor in path.ancestors().skip(1) {
        match std::fs::metadata(ancestor) {
            Ok(m) if m.dev() == device => top = ancestor.to_path_buf(),
            _ => break,
        }
    }
    top
}

/// The path as the spec's URL-style escaped text: every byte except unreserved ones and `/`.
fn percent_encode(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `YYYY-MM-DDThh:mm:ss` for `time`. The spec asks for local time; with no time-zone database in
/// the build this writes UTC, which file managers still read (only the shown hour can differ).
fn deletion_date(time: web_time::SystemTime) -> String {
    let secs = time
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a civil date (proleptic Gregorian), in 400-year eras.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}
