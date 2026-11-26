//! The Windows Recycle Bin (feature `recycle-bin`): moving an item into it
//! and the two reads around that, each one Shell or file system call.
//!
//! agg-gui-files' trash (`src/disk/trash/windows.rs`) is the consumer: it
//! recycles an item with [`recycle`], then finds it in the drive's
//! `$Recycle.Bin` by its path (in both spellings, [`long_path_name`] giving the
//! long one) and tells a recycle from a permanent delete by the Recycle Bin's
//! item count ([`recycle_bin_item_count`]). The search, the messages and the
//! decisions stay there.
//!
//! A path holding a NUL character is refused before any call. In the list
//! `SHFileOperationW` takes, a NUL ends one name and starts the next, so such
//! a path could name other files as well; the other two calls read a path up
//! to its first NUL, so it would name a different one.

use std::ffi::OsString;
use std::fmt;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys_061::Win32::Foundation::S_OK;
use windows_sys_061::Win32::Storage::FileSystem::GetLongPathNameW;
use windows_sys_061::Win32::UI::Shell::{
    SHFileOperationW, SHQueryRecycleBinW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI,
    FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW, SHQUERYRBINFO,
};

/// Why [`recycle`] did not recycle an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecycleError {
    /// The path holds a NUL character; nothing was called.
    InteriorNul,
    /// `SHFileOperationW` returned `code` (non-zero on failure), or it
    /// reported that the operation was aborted (`aborted`), or both.
    Failed {
        /// The call's return code; 0 when only `aborted` is set.
        code: i32,
        /// `fAnyOperationsAborted` was set.
        aborted: bool,
    },
}

impl fmt::Display for RecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InteriorNul => write!(f, "the path holds a NUL character"),
            Self::Failed { code, aborted } => write!(
                f,
                "SHFileOperationW failed (code {code}{})",
                if *aborted { ", aborted" } else { "" }
            ),
        }
    }
}

impl std::error::Error for RecycleError {}

/// Moves the file or folder at `path` to the Recycle Bin:
/// `SHFileOperationW(FO_DELETE)` with `FOF_ALLOWUNDO | FOF_NOCONFIRMATION |
/// FOF_NOERRORUI | FOF_SILENT`, so no dialog is shown.
///
/// `FOF_ALLOWUNDO` recycles only "if possible": on a drive with no Recycle Bin,
/// or for an item too large for it, Windows deletes it permanently and still
/// returns `Ok`.
pub fn recycle(path: &Path) -> Result<(), RecycleError> {
    // The source list is double-null terminated.
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    if from.contains(&0) {
        return Err(RecycleError::InteriorNul);
    }
    from.extend([0, 0]);
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        // The flags fit the struct's 16-bit field: all are below 0x10000.
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        ..Default::default()
    };
    // SAFETY: `operation` is fully initialised and `from` (the only pointer it
    // carries) is a double-null-terminated list that outlives the call; the
    // call reads them and writes only `fAnyOperationsAborted`.
    let code = unsafe { SHFileOperationW(&mut operation) };
    let aborted = operation.fAnyOperationsAborted != 0;
    if code != 0 || aborted {
        return Err(RecycleError::Failed { code, aborted });
    }
    Ok(())
}

/// `path` with every short 8.3 component spelled out (`RUNNER~1` ->
/// `runneradmin`), through `GetLongPathNameW`; `None` when Windows can't say
/// (the path is missing, or the call fails) or the path holds a NUL.
pub fn long_path_name(path: &Path) -> Option<PathBuf> {
    let mut short: Vec<u16> = path.as_os_str().encode_wide().collect();
    if short.contains(&0) {
        return None;
    }
    short.push(0);
    let mut buffer: Vec<u16> = vec![0; 512];
    loop {
        // A path longer than u32::MAX characters can't exist; the clamp only
        // keeps the cast exact.
        let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: `short` is null terminated and `buffer` holds `capacity`
        // writable characters; the call writes at most that many.
        let written = unsafe { GetLongPathNameW(short.as_ptr(), buffer.as_mut_ptr(), capacity) };
        let written = written as usize;
        if written == 0 {
            return None;
        }
        if written < buffer.len() {
            buffer.truncate(written);
            return Some(PathBuf::from(OsString::from_wide(&buffer)));
        }
        // Too small: `written` is the size needed, null included.
        buffer = vec![0; written];
    }
}

/// How many items the Recycle Bin of the drive at `drive_root` holds
/// (`SHQueryRecycleBinW`), or `None` when Windows can't say or the path holds
/// a NUL.
pub fn recycle_bin_item_count(drive_root: &Path) -> Option<i64> {
    let mut root: Vec<u16> = drive_root.as_os_str().encode_wide().collect();
    if root.contains(&0) {
        return None;
    }
    root.push(0);
    let mut info = SHQUERYRBINFO {
        // The struct's size, a few dozen bytes: the cast can't overflow.
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `root` is null terminated and `info` is a writable struct of the
    // size it declares.
    let result = unsafe { SHQueryRecycleBinW(root.as_ptr(), &mut info) };
    (result == S_OK).then_some(info.i64NumItems)
}
