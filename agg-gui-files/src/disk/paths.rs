//! What every disk item takes from its path: the shown name, the dates, the ID, a free name for a
//! copy or a move, and the location key a folder tree knows a folder by. `file` and
//! `folder_link` build their items from these; `folder` and `folder_writes` name what they list
//! and where writes land.
//!
//! The ID is kept exactly as it has always been computed (an apps' thumbnail cache is keyed by
//! it): a 64-bit FNV-1a hash over the UTF-16 bytes of the path followed by the last write time in
//! round-trip UTC form, so the ID changes when the file is written.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::sort::system_time_ticks;
use crate::translate::tr;

/// FNV-1a's 64-bit offset basis, where every ID hash starts.
const FNV64_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
/// FNV-1a's 64-bit prime.
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a over `bytes`. FNV is defined modulo 2^64, so the multiply wraps.
fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = FNV64_OFFSET_BASIS;
    for &value in bytes {
        hash ^= u64::from(value);
        hash = hash.wrapping_mul(FNV64_PRIME);
    }
    hash
}

/// The extension without its dot, lower case ("" for none).
pub fn extension_lower(file_name: &str) -> String {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    match name.rfind('.') {
        Some(dot) => name[dot..].trim_matches('.').to_lowercase(),
        None => String::new(),
    }
}

/// The last path segment, empty for a root such as `/` or `C:\`.
pub(crate) fn path_file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The file or folder name shown in a listing, "{first character} Drive" for a drive root and
/// "Unknown" for an empty path.
pub(crate) fn display_name(path: &Path) -> String {
    let final_directory = path_file_name(path);
    if !final_directory.is_empty() {
        return final_directory;
    }
    let text = path.to_string_lossy();
    match text.chars().next() {
        Some(first) => format!("{first} {}", tr("Drive")),
        None => tr("Unknown"),
    }
}

/// The item's creation and last modified dates as ticks (`sort::system_time_ticks`): the file's
/// creation and last write times. Where the platform keeps no creation time the write time stands
/// in; a path that cannot be read answers 0, so it has no date to sort on rather than a made-up
/// one.
pub(crate) fn file_dates(path: &Path) -> (i64, i64) {
    let Ok(meta) = std::fs::metadata(path) else {
        return (0, 0);
    };
    let modified = meta.modified().map(system_time_ticks).unwrap_or(0);
    let created = meta.created().map(system_time_ticks).unwrap_or(modified);
    (created, modified)
}

/// The ID of the file or folder at `path` (see the file header). A path whose write time cannot
/// be read uses now.
pub fn disk_item_id(path: &Path) -> String {
    let since_epoch = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .or_else(|| {
            web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .ok()
        })
        .unwrap_or_default();
    disk_item_id_at(path, since_epoch)
}

/// The ID of `path` last written `since_epoch` after 1970-01-01 UTC: [`disk_item_id`] with the
/// write time given, so the hash can be checked without a file.
pub fn disk_item_id_at(path: &Path, since_epoch: Duration) -> String {
    let text = format!("{}{}", path.to_string_lossy(), round_trip_utc(since_epoch));
    let bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    fnv1a_64(&bytes).to_string()
}

/// The round-trip text of a UTC time `since_epoch` after 1970-01-01:
/// `yyyy-MM-ddTHH:mm:ss.fffffffZ`, to the 100 ns tick.
pub fn round_trip_utc(since_epoch: Duration) -> String {
    let secs = since_epoch.as_secs();
    let ticks = since_epoch.subsec_nanos() / 100;
    // Seconds since 1970 over a day's seconds fit an i64 for any time a SystemTime can hold.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ticks:07}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// A folder on disk's location key: its path, spelled one way however it was written. A doubled
/// or trailing separator and an inner "." are left out, as comparing the paths themselves leaves
/// them out, so a folder and its row agree. The key is the path's escaped, quoted text, which
/// keeps every byte apart: two folders whose names differ only in bytes that are not text
/// (possible on Linux) get different keys, where lossy text would make both the same. The
/// escaping never writes a raw NUL, so a key built from it and a NUL never equals a folder's. The
/// key is opaque and never saved.
pub fn disk_location_key(path: &Path) -> String {
    format!("{:?}", path.components().collect::<PathBuf>())
}

/// `name` less a trailing `extension` (ignoring case).
pub(crate) fn without_extension<'a>(name: &'a str, extension: &str) -> &'a str {
    if name.len() >= extension.len()
        && name.is_char_boundary(name.len() - extension.len())
        && name[name.len() - extension.len()..].eq_ignore_ascii_case(extension)
    {
        &name[..name.len() - extension.len()]
    } else {
        name
    }
}

/// `name` with `extension` appended - not swapped in, which would take a dot in the typed name
/// ("My.Part") for an extension and drop everything after it. A name already ending in the
/// extension keeps just the one.
pub fn with_extension(name: &str, extension: &str) -> String {
    format!("{}{extension}", without_extension(name, extension))
}

/// The last segment's extension with its dot, "" for none.
pub(crate) fn extension_with_dot(path: &Path) -> String {
    path.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

/// `name` without a trailing `" (n)"`, so numbering "part (1)" gives "part (2)", not
/// "part (1) (1)". Digits are ASCII only: the numbers written here are.
fn strip_trailing_number(name: &str) -> &str {
    let Some(rest) = name.trim_end().strip_suffix(')') else {
        return name;
    };
    let digits_start = rest.trim_end_matches(|c: char| c.is_ascii_digit());
    if digits_start.len() == rest.len() {
        return name;
    }
    match digits_start.strip_suffix('(') {
        Some(prefix) => &name[..prefix.len()],
        None => name,
    }
}

/// `desired` when `is_free` accepts it; otherwise its base (less a trailing " (n)") followed by
/// " (1)", " (2)", ..., whichever `is_free` accepts first.
pub(crate) fn non_colliding_name(desired: &str, mut is_free: impl FnMut(&str) -> bool) -> String {
    if is_free(desired) {
        return desired.to_string();
    }
    let base = if desired.contains('(') {
        strip_trailing_number(desired).trim()
    } else {
        desired
    };
    let mut next = 1u64;
    loop {
        let candidate = format!("{base} ({next})");
        next += 1;
        if is_free(&candidate) {
            return candidate;
        }
    }
}

/// A free path for `destination` in its folder: unchanged when nothing is there, otherwise
/// "name (n).ext", numbered before the extension so the copy keeps its type.
pub(crate) fn non_colliding_path(destination: &Path) -> PathBuf {
    let taken = |p: &Path| p.exists();
    if !taken(destination) {
        return destination.to_path_buf();
    }
    let parent = destination
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let is_dir = destination.is_dir();
    let stem = if is_dir {
        path_file_name(destination)
    } else {
        destination
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let extension = match destination.extension() {
        Some(e) if !is_dir => format!(".{}", e.to_string_lossy()),
        _ => String::new(),
    };
    let name = non_colliding_name(&stem, |candidate| {
        !taken(&parent.join(format!("{candidate}{extension}")))
    });
    parent.join(format!("{name}{extension}"))
}
