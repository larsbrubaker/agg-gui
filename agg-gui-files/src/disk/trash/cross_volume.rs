//! The move a trash directory set with `set_trash_directory` makes (`mod.rs`'s
//! `move_into_directory` into it, and `restore_from_trash` back out): a rename, and when the rename
//! reports that the two paths are on different volumes, a copy and then the removal of the
//! original. A rename cannot cross volumes, and a test's trash under the system temp folder with
//! its files on another drive (GitHub's Windows runners: temp on `C:`, the workspace on `D:`)
//! would otherwise fail every trash. The system trashes keep their own moves: the freedesktop
//! step picks a trash on the item's own drive, and macOS and Windows move the item themselves.
//!
//! The rename and the file copy are parameters so the tests (`cross_volume_tests.rs`) can report a
//! cross-device rename or fail a copy part way without two volumes; the callers pass
//! `std::fs::rename` and `std::fs::copy` ([`SYSTEM`]).

use std::io;
use std::path::Path;

/// The file operations a move uses: `std::fs`'s own ([`SYSTEM`]), or a test's.
pub(super) struct FileOps<'a> {
    /// Renames the first path to the second (`std::fs::rename`).
    pub rename: &'a dyn Fn(&Path, &Path) -> io::Result<()>,
    /// Copies one file's bytes and permissions to a new path (`std::fs::copy`).
    pub copy_file: &'a dyn Fn(&Path, &Path) -> io::Result<u64>,
}

/// `std::fs::rename` and `std::fs::copy`: what every move outside the tests uses.
pub(super) const SYSTEM: FileOps<'static> = FileOps {
    rename: &|from, to| std::fs::rename(from, to),
    copy_file: &|from, to| std::fs::copy(from, to),
};

/// Moves the file or folder at `from` to `to`: a rename, or, when the rename fails with
/// [`io::ErrorKind::CrossesDevices`] and only then, a copy of everything at `from` followed by its
/// removal. Any other rename error is returned as it is, with nothing copied.
///
/// The copy differs from a rename in what it keeps. A file keeps its bytes and permissions (what
/// `std::fs::copy` copies; on macOS and Windows its other attributes too) and, where the file
/// can be opened to write, its modified and accessed times. A folder keeps its permissions, set
/// once all of it is copied, but takes the time of the copy. Only regular files and folders are
/// copied; anything else, at `from` or anywhere inside a folder, is refused
/// (`io::ErrorKind::Unsupported`) before it is opened, so the item is left where it was. A
/// symbolic link would copy what it points at (a folder outside the item, or a loop) and
/// recreating it needs each system's own call; a named pipe blocks whoever opens it until a writer
/// comes, and a socket or a device has no bytes of its own to copy.
///
/// The copy never touches a destination it did not make: it makes `to` itself, as a new folder or
/// a new empty file, in a way that fails when anything is already there (another thread trashing
/// a same-named item into the same directory may have taken the name since the caller saw it
/// free). When that fails, or anything before it does, the error is returned and `to` is left as
/// it was.
///
/// A failed move never loses the item. A copy that fails part way removes what it made at `to`
/// and returns its error, leaving `from` as it was. Once the copy is whole, removing the original
/// can still fail: then a file's copy is removed again while the file is still at `from`; a
/// folder's copy stays at `to`, since removing the folder may have taken part of it, and so does
/// the copy of a file that is no longer at `from` (or whose copy can't be removed), the error
/// naming where that copy is.
pub(super) fn move_item(from: &Path, to: &Path, ops: &FileOps) -> io::Result<()> {
    match (ops.rename)(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => copy_then_remove(from, to, ops),
        other => other,
    }
}

fn copy_then_remove(from: &Path, to: &Path, ops: &FileOps) -> io::Result<()> {
    // Read once: the copy and the original's removal both go by it, so no later read can fail
    // with a whole copy made.
    let metadata = std::fs::symlink_metadata(from)?;
    refuse_unless_copyable(from, &metadata)?;
    // Fails when anything is at `to`; until it succeeds this move has made nothing to remove.
    if metadata.is_dir() {
        std::fs::create_dir(to)?;
    } else {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to)?;
    }
    let mut folders = Vec::new();
    let copied = copy_into(from, to, &metadata, ops, &mut folders).and_then(|()| {
        // Deepest first, so a read-only folder is made read-only after what is in it is written.
        for (folder, permissions) in folders.into_iter().rev() {
            std::fs::set_permissions(&folder, permissions)?;
        }
        Ok(())
    });
    if let Err(e) = copied {
        remove_made(to);
        return Err(e);
    }
    if metadata.is_dir() {
        return std::fs::remove_dir_all(from).map_err(|e| copy_kept_at(e, "folder", to));
    }
    std::fs::remove_file(from).map_err(|e| {
        // The copy goes again only while the file is surely still at `from`.
        if std::fs::symlink_metadata(from).is_ok() && std::fs::remove_file(to).is_ok() {
            e
        } else {
            copy_kept_at(e, "file", to)
        }
    })
}

/// `e` with a note that a whole copy of the item (a `kind`) stays at `to`.
fn copy_kept_at(e: io::Error, kind: &str, to: &Path) -> io::Error {
    io::Error::new(
        e.kind(),
        format!("{e} (a whole copy of the {kind} is at {})", to.display()),
    )
}

/// Refuses an entry that is neither a regular file nor a folder, by its metadata alone (nothing
/// is opened): see [`move_item`] for why.
fn refuse_unless_copyable(path: &Path, metadata: &std::fs::Metadata) -> io::Result<()> {
    let file_type = metadata.file_type();
    if file_type.is_dir() || file_type.is_file() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!(
            "{}: {} can't be moved to another volume",
            path.display(),
            special_kind(&file_type)
        ),
    ))
}

/// What a file that is neither a regular file nor a folder is, for the refusal's message.
fn special_kind(file_type: &std::fs::FileType) -> &'static str {
    if file_type.is_symlink() {
        return "a symbolic link";
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if file_type.is_fifo() {
            return "a named pipe";
        }
        if file_type.is_socket() {
            return "a socket";
        }
        if file_type.is_block_device() || file_type.is_char_device() {
            return "a device file";
        }
    }
    "a special file"
}

/// Copies what `from` holds into `to`, which this move has made already (a folder for a folder, an
/// empty file for a file); `metadata` is `from`'s. A folder's entries are made inside it, each
/// checked first, and each folder is noted in `folders` with the permissions it is to get once the
/// copy is whole. The file copy may replace the empty file this move made.
fn copy_into(
    from: &Path,
    to: &Path,
    metadata: &std::fs::Metadata,
    ops: &FileOps,
    folders: &mut Vec<(std::path::PathBuf, std::fs::Permissions)>,
) -> io::Result<()> {
    if !metadata.is_dir() {
        (ops.copy_file)(from, to)?;
        keep_file_times(metadata, to);
        return Ok(());
    }
    folders.push((to.to_path_buf(), metadata.permissions()));
    for entry in std::fs::read_dir(from)? {
        let name = entry?.file_name();
        let (from, to) = (from.join(&name), to.join(&name));
        let metadata = std::fs::symlink_metadata(&from)?;
        refuse_unless_copyable(&from, &metadata)?;
        if metadata.is_dir() {
            std::fs::create_dir(&to)?;
        }
        copy_into(&from, &to, &metadata, ops, folders)?;
    }
    Ok(())
}

/// Gives the copy at `to` the original's modified and accessed times, where the copy can be
/// opened to write (a read-only file keeps the time of the copy). The bytes are what matter; a
/// time that can't be kept is no reason to fail the move.
fn keep_file_times(original: &std::fs::Metadata, to: &Path) {
    let mut times = std::fs::FileTimes::new();
    if let Ok(modified) = original.modified() {
        times = times.set_modified(modified);
    }
    if let Ok(accessed) = original.accessed() {
        times = times.set_accessed(accessed);
    }
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(to) {
        let _ = file.set_times(times);
    }
}

/// Removes what a failed copy made at `to`. Called only once this move has made `to` itself
/// (`copy_then_remove`), so whatever is there is this move's.
fn remove_made(to: &Path) {
    match std::fs::symlink_metadata(to) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(to);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(to);
        }
        Err(_) => {}
    }
}
