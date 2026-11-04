//! Reading a folder on disk's location key back: the path it names ([`disk_key_path`]) and
//! whether one folder's key lies within another's ([`disk_key_within`]). The key's spelling is
//! the crate's `disk_paths::disk_location_key`, shared with the zip provider; reading it back
//! serves only the disk provider, so it is compiled with the `disk` feature alone.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::disk_paths::drive_upper;

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

/// The path a folder's location key ([`disk_location_key`](super::disk_location_key)) names;
/// `None` for a text no folder keys, or a name whose bytes this computer can't hold as a path.
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

/// Whether the folder keyed `inner` ([`disk_location_key`](super::disk_location_key)) is the one
/// keyed `outer` or inside it: `outer`'s parts start `inner`'s, compared part by part as paths
/// are (so "Designs2" is beside "Designs"), a drive letter in either case naming one drive. The
/// parts are read from the keys' own text, so the rule is the same on every computer.
pub fn disk_key_within(inner: &str, outer: &str) -> bool {
    match (key_parts(inner), key_parts(outer)) {
        (Some(inner), Some(outer)) => inner.starts_with(&outer),
        _ => false,
    }
}
