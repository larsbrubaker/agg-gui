//! The macOS Trash: `NSFileManager trashItemAtURL:resultingItemURL:error:` moves the item into the
//! Trash of its volume (Finder's Put Back works on it) and answers the item's new URL, which is all
//! a restore needs.

use std::path::Path;

use objc2_foundation::{NSFileManager, NSString, NSURL};

use super::TrashedItem;

pub(super) fn trash(original: &Path) -> Result<TrashedItem, String> {
    let path = NSString::from_str(&original.to_string_lossy());
    let url = NSURL::fileURLWithPath(&path);
    let manager = NSFileManager::defaultManager();
    let mut resulting = None;
    manager
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|e| format!("{}: {}", original.display(), e.localizedDescription()))?;
    let in_trash = resulting
        .and_then(|url| url.path())
        .map(|p| p.to_string())
        .ok_or_else(|| {
            format!(
                "{}: the Trash did not say where it went",
                original.display()
            )
        })?;
    Ok(TrashedItem {
        original: original.to_path_buf(),
        in_trash: in_trash.into(),
        info_file: None,
    })
}
