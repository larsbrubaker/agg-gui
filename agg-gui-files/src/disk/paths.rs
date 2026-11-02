//! The naming a folder on disk's writes need: an extension kept through a rename, and a free
//! name for a copy or a move ("name (1).ext"); and whether a file lists as a folder (a zip file). The names, dates, IDs and location keys every disk
//! item shares are the crate's `disk_paths`, re-exported here for the disk modules.

use std::path::{Path, PathBuf};

pub(crate) use crate::disk_paths::{
    disk_item_id, disk_location_key, display_name, file_dates, path_file_name,
};

/// The extension without its dot, lower case ("" for none).
pub fn extension_lower(file_name: &str) -> String {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    match name.rfind('.') {
        Some(dot) => name[dot..].trim_matches('.').to_lowercase(),
        None => String::new(),
    }
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

/// Whether a file in a folder on disk lists as a folder rather than a file: a zip file, with the
/// `zip` feature ([`crate::zip_folder::lists_as_zip_folder`]). The listing, the tree's quick look
/// and the search all ask this, so they agree.
#[cfg(feature = "zip")]
pub(crate) fn lists_as_folder(file: &Path) -> bool {
    crate::zip_folder::lists_as_zip_folder(file)
}

/// Without the `zip` feature every file lists as a file.
#[cfg(not(feature = "zip"))]
pub(crate) fn lists_as_folder(_file: &Path) -> bool {
    false
}
