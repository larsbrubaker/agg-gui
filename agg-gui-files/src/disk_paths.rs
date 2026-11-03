//! What every item on the computer's disk takes from its path, for the disk provider (`disk`)
//! and the zip provider (`zip_folder`) alike: the shown name, the dates, the ID, and the location
//! key a folder tree knows a folder by.
//!
//! The ID's exact text is fixed, since an app's thumbnail cache is keyed by it (golden IDs pin
//! it): a 64-bit FNV-1a hash over the UTF-16 bytes of the path followed by the last write time in
//! round-trip UTC form, so the ID changes when the file is written. An entry inside a zip archive
//! hashes the archive's path and the entry's path the same way ([`utf16_hash`]).

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
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
    utf16_hash(&format!(
        "{}{}",
        path.to_string_lossy(),
        round_trip_utc(since_epoch)
    ))
}

/// The 64-bit FNV-1a hash of `text`'s UTF-16 (little-endian) bytes, as decimal text.
pub(crate) fn utf16_hash(text: &str) -> String {
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
/// them out, so a folder and its row agree; a drive letter is upper case, as Windows reads either
/// case as one drive. The key is the path's parts in order, each followed by "/": the root is an
/// empty part, a drive or share prefix its text, a name its text with "%", "/" and NUL written
/// "%25", "%2F" and "%00" (so a key never holds a raw NUL) and every byte that is not text
/// "%XX", so every byte stays apart (two folders whose names differ only in bytes that are not
/// text, possible on Linux, get different keys). `/a/b` is `/a/b/`, `C:\x` is `C://x/`. The key
/// lives only in memory and is never saved; [`disk_key_path`] reads it back and
/// [`disk_key_within`] compares two.
pub fn disk_location_key(path: &Path) -> String {
    let mut key = String::new();
    for part in path.components() {
        match part {
            Component::RootDir => {}
            Component::Prefix(prefix) => key.push_str(&drive_upper(&escape(prefix.as_os_str()))),
            other => key.push_str(&escape(other.as_os_str())),
        }
        key.push('/');
    }
    key
}

/// `part`'s bytes as key text (see [`disk_location_key`]).
fn escape(part: &OsStr) -> String {
    let mut out = String::new();
    for chunk in part.as_encoded_bytes().utf8_chunks() {
        for c in chunk.valid().chars() {
            match c {
                '%' => out.push_str("%25"),
                '/' => out.push_str("%2F"),
                '\0' => out.push_str("%00"),
                c => out.push(c),
            }
        }
        for byte in chunk.invalid() {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A drive part ("c:", or "\\?\c:") with its letter upper case; any other part as it is.
fn drive_upper(part: &str) -> String {
    let bytes = part.as_bytes();
    let letter_at = match bytes {
        [_, b':'] => Some(0),
        [b'\\', b'\\', b'?', b'\\', _, b':'] => Some(4),
        _ => None,
    };
    match letter_at.filter(|&i| bytes[i].is_ascii_alphabetic()) {
        Some(i) => {
            let mut out = part.to_string();
            out.replace_range(i..=i, &part[i..=i].to_ascii_uppercase());
            out
        }
        None => part.to_string(),
    }
}

/// A key's parts, the first (a drive) upper case; `None` for a text no folder keys.
fn key_parts(key: &str) -> Option<Vec<String>> {
    if !key.is_empty() && !key.ends_with('/') {
        return None;
    }
    let mut parts: Vec<String> = key.split_terminator('/').map(str::to_string).collect();
    if let Some(first) = parts.first_mut() {
        *first = drive_upper(first);
    }
    Some(parts)
}

/// A part's bytes from its key text; `None` for a malformed escape.
fn unescape(part: &str) -> Option<Vec<u8>> {
    let (bytes, mut out, mut i) = (part.as_bytes(), Vec::new(), 0);
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = part.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

/// The path a folder's location key ([`disk_location_key`]) names; `None` for a text no folder
/// keys, or a name whose bytes this computer can't hold as a path.
pub fn disk_key_path(key: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in key_parts(key)? {
        if part.is_empty() {
            path.push(std::path::MAIN_SEPARATOR_STR);
            continue;
        }
        path.push(os_string(unescape(&part)?)?);
    }
    Some(path)
}

/// `bytes` as a name: text anywhere, other bytes as they are where a name holds any bytes.
fn os_string(bytes: Vec<u8>) -> Option<OsString> {
    match String::from_utf8(bytes) {
        Ok(text) => Some(OsString::from(text)),
        #[cfg(unix)]
        Err(e) => Some(std::os::unix::ffi::OsStringExt::from_vec(e.into_bytes())),
        #[cfg(not(unix))]
        Err(_) => None,
    }
}

/// Whether the folder keyed `inner` ([`disk_location_key`]) is the one keyed `outer` or inside
/// it: `outer`'s parts start `inner`'s, compared part by part as paths are (so "Designs2" is
/// beside "Designs"), a drive letter in either case naming one drive. The parts are read from the
/// keys' own text, so the rule is the same on every computer.
pub fn disk_key_within(inner: &str, outer: &str) -> bool {
    match (key_parts(inner), key_parts(outer)) {
        (Some(inner), Some(outer)) => inner.starts_with(&outer),
        _ => false,
    }
}
