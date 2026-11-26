//! A trash directory on another volume than the item (`cross_volume.rs`), tested on one volume: the
//! move into the directory (`move_into_directory_with`) and the restore out of it (`restore_with`)
//! are given a rename that reports a cross-device move, as one between two volumes does, or a file
//! copy that fails part way. Unit tests rather than `tests/disk/trash.rs` because those two
//! operations are a parameter the public functions fix to `std::fs`'s own. Every test works in its
//! own scratch folder under the system temp folder and never touches the process's trash
//! directory (`set_trash_directory`), which `tests/disk` sets.

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::cross_volume::{FileOps, SYSTEM};
use super::{move_into_directory_with, restore_with, TrashedItem};

/// A fresh folder under the system temp folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "agg-gui-files-cross-volume-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the scratch folder is made");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cross_device(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::CrossesDevices))
}

/// What a folder holds, every path in it relative to it, with each file's bytes (`None` for a
/// folder), sorted; a file alone is one entry with an empty path.
fn tree(path: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<(String, Option<Vec<u8>>)>) {
        let relative = at
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if at.is_dir() {
            out.push((relative, None));
            for entry in std::fs::read_dir(at).unwrap() {
                walk(root, &entry.unwrap().path(), out);
            }
        } else {
            out.push((relative, Some(std::fs::read(at).unwrap())));
        }
    }
    let mut out = Vec::new();
    walk(path, path, &mut out);
    out.sort();
    out
}

/// `Brackets/` with two files, a folder holding a file and a folder with a file, and an empty
/// folder.
fn brackets(parent: &Path) -> PathBuf {
    let folder = parent.join("Brackets");
    std::fs::create_dir_all(folder.join("Sub").join("Deeper")).unwrap();
    std::fs::create_dir_all(folder.join("Empty")).unwrap();
    std::fs::write(folder.join("a.stl"), b"solid a\nendsolid a\n").unwrap();
    std::fs::write(folder.join("notes.txt"), b"two of these").unwrap();
    std::fs::write(folder.join("Sub").join("b.stl"), b"solid b\nendsolid b\n").unwrap();
    std::fs::write(folder.join("Sub").join("Deeper").join("c.stl"), b"c").unwrap();
    folder
}

/// A file whose modified time is a fixed whole second, so a kept time is seen to be kept.
fn part_file(parent: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = parent.join(name);
    std::fs::write(&path, bytes).unwrap();
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(old).unwrap();
    path
}

fn modified(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

#[test]
fn a_file_trashed_across_volumes_is_copied_in_and_removed() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let folder = scratch.path().join("Parts");
    std::fs::create_dir_all(&folder).unwrap();
    let ops = FileOps {
        rename: &cross_device,
        ..SYSTEM
    };

    // A same-volume trash first takes the plain name, so the cross-volume one is numbered the
    // same way a rename is.
    let first = part_file(&folder, "Part.stl", b"first");
    let same_volume = move_into_directory_with(&first, &trash, &SYSTEM).unwrap();
    let path = part_file(&folder, "Part.stl", b"solid p\nendsolid p\n");
    let time = modified(&path);
    let trashed = move_into_directory_with(&path, &trash, &ops).unwrap();

    assert_eq!(same_volume.in_trash, trash.join("Part.stl"));
    assert_eq!(
        trashed,
        TrashedItem {
            original: path.clone(),
            in_trash: trash.join("Part 2.stl"),
            info_file: None,
        }
    );
    assert!(!path.exists(), "the original is gone from its folder");
    assert_eq!(
        std::fs::read(&trashed.in_trash).unwrap(),
        b"solid p\nendsolid p\n"
    );
    assert_eq!(
        modified(&trashed.in_trash),
        time,
        "the modified time is kept"
    );
}

#[test]
fn a_folder_trashed_across_volumes_is_copied_in_whole_and_removed() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let folder = brackets(scratch.path());
    let before = tree(&folder);
    let ops = FileOps {
        rename: &cross_device,
        ..SYSTEM
    };

    let trashed = move_into_directory_with(&folder, &trash, &ops).unwrap();

    assert_eq!(
        trashed,
        TrashedItem {
            original: folder.clone(),
            in_trash: trash.join("Brackets"),
            info_file: None,
        }
    );
    assert!(!folder.exists(), "the original is gone from its folder");
    assert_eq!(tree(&trashed.in_trash), before);
}

#[test]
fn items_restore_from_a_trash_on_another_volume() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let parts = scratch.path().join("Parts");
    std::fs::create_dir_all(&parts).unwrap();
    let file = part_file(&parts, "Part.stl", b"solid p\nendsolid p\n");
    let time = modified(&file);
    let folder = brackets(&parts);
    let folder_before = tree(&folder);
    let ops = FileOps {
        rename: &cross_device,
        ..SYSTEM
    };

    for (path, into) in [(&file, &SYSTEM), (&folder, &ops)] {
        // One goes in by a rename and one by a copy; both come back by a copy.
        let trashed = move_into_directory_with(path, &trash, into).unwrap();
        restore_with(&trashed, &ops).unwrap();
        assert!(
            !trashed.in_trash.exists(),
            "{:?} is gone from the trash",
            trashed.in_trash
        );
    }

    assert_eq!(std::fs::read(&file).unwrap(), b"solid p\nendsolid p\n");
    assert_eq!(modified(&file), time, "the modified time is kept");
    assert_eq!(tree(&folder), folder_before);
    assert_eq!(
        tree(&trash),
        vec![(String::new(), None)],
        "the trash is empty"
    );
}

#[test]
fn a_copy_that_fails_part_way_leaves_the_original_and_nothing_half_copied() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let folder = brackets(scratch.path());
    let before = tree(&folder);
    let copies = Cell::new(0);
    // The second file copied fails after writing part of its bytes, as a full disk does.
    let failing_copy = |from: &Path, to: &Path| {
        copies.set(copies.get() + 1);
        if copies.get() == 2 {
            std::fs::write(to, b"half")?;
            return Err(io::Error::other("the disk is full"));
        }
        std::fs::copy(from, to)
    };
    let ops = FileOps {
        rename: &cross_device,
        copy_file: &failing_copy,
    };

    let error = move_into_directory_with(&folder, &trash, &ops).unwrap_err();
    assert_eq!(copies.get(), 2, "the copy was attempted and stopped");
    assert_eq!(
        error,
        format!("{}: the disk is full", folder.display()),
        "the copy's error"
    );
    assert_eq!(tree(&folder), before, "the original is as it was");
    assert_eq!(
        tree(&trash),
        vec![(String::new(), None)],
        "nothing is left in the trash"
    );

    // A restore whose copy fails leaves the item in the trash and nothing at its old path.
    let trashed = move_into_directory_with(&folder, &trash, &SYSTEM).unwrap();
    let in_trash = tree(&trashed.in_trash);
    copies.set(0);
    assert!(restore_with(&trashed, &ops).is_err());
    assert_eq!(copies.get(), 2, "the copy back was attempted and stopped");
    assert!(!folder.exists(), "nothing half-copied at the old path");
    assert_eq!(
        tree(&trashed.in_trash),
        in_trash,
        "the item stays in the trash"
    );
}

#[test]
fn other_rename_errors_are_returned_with_no_copy() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let path = part_file(scratch.path(), "Part.stl", b"p");
    let copies = Cell::new(0);
    let counting_copy = |from: &Path, to: &Path| {
        copies.set(copies.get() + 1);
        std::fs::copy(from, to)
    };
    let denied = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::PermissionDenied));
    let ops = FileOps {
        rename: &denied,
        copy_file: &counting_copy,
    };

    let error = move_into_directory_with(&path, &trash, &ops).unwrap_err();
    assert_eq!(
        error,
        format!(
            "{}: {}",
            path.display(),
            io::Error::from(io::ErrorKind::PermissionDenied)
        )
    );
    assert_eq!(copies.get(), 0, "no copy is attempted");
    assert_eq!(std::fs::read(&path).unwrap(), b"p");
    assert_eq!(tree(&trash), vec![(String::new(), None)]);

    let trashed = move_into_directory_with(&path, &trash, &SYSTEM).unwrap();
    assert!(restore_with(&trashed, &ops).is_err());
    assert_eq!(copies.get(), 0, "no copy is attempted on restore");
    assert!(trashed.in_trash.exists());
}

/// A link inside a folder is neither followed nor recreated across volumes: the trash fails and
/// leaves the folder as it was.
#[cfg(unix)]
#[test]
fn a_folder_holding_a_symbolic_link_stays_put_across_volumes() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let folder = brackets(scratch.path());
    std::os::unix::fs::symlink(folder.join("a.stl"), folder.join("link.stl")).unwrap();
    let ops = FileOps {
        rename: &cross_device,
        ..SYSTEM
    };

    let error = move_into_directory_with(&folder, &trash, &ops).unwrap_err();
    assert!(error.contains("symbolic link"), "{error}");
    assert!(std::fs::symlink_metadata(folder.join("link.stl"))
        .unwrap()
        .is_symlink());
    assert_eq!(
        std::fs::read(folder.join("a.stl")).unwrap(),
        b"solid a\nendsolid a\n"
    );
    assert_eq!(tree(&trash), vec![(String::new(), None)]);
}

/// What another thread trashing a same-named folder into the same directory leaves at the
/// destination when it takes the name between `unique_path`'s look and this move: a folder of its
/// own, with a file in it.
fn their_folder(to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to.join("Theirs"))?;
    std::fs::write(to.join("Theirs").join("t.stl"), b"theirs")
}

/// A cross-device move onto a folder that appeared at the chosen name meanwhile fails and leaves
/// that folder and everything in it as it was, and the original too.
#[test]
fn a_folder_never_replaces_a_destination_taken_meanwhile() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let folder = brackets(scratch.path());
    let before = tree(&folder);
    let taken = |_: &Path, to: &Path| -> io::Result<()> {
        their_folder(to)?;
        Err(io::Error::from(io::ErrorKind::CrossesDevices))
    };
    let ops = FileOps {
        rename: &taken,
        ..SYSTEM
    };

    assert!(move_into_directory_with(&folder, &trash, &ops).is_err());
    assert_eq!(
        tree(&trash.join("Brackets")),
        vec![
            (String::new(), None),
            ("Theirs".to_string(), None),
            ("Theirs/t.stl".to_string(), Some(b"theirs".to_vec())),
        ],
        "the folder that was there is untouched"
    );
    assert_eq!(tree(&folder), before, "the original is as it was");
}

/// The same for a file: the file that appeared at the chosen name keeps its bytes.
#[test]
fn a_file_never_overwrites_a_destination_taken_meanwhile() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let path = part_file(scratch.path(), "Part.stl", b"mine");
    let taken = |_: &Path, to: &Path| -> io::Result<()> {
        std::fs::write(to, b"theirs")?;
        Err(io::Error::from(io::ErrorKind::CrossesDevices))
    };
    let ops = FileOps {
        rename: &taken,
        ..SYSTEM
    };

    assert!(move_into_directory_with(&path, &trash, &ops).is_err());
    assert_eq!(std::fs::read(trash.join("Part.stl")).unwrap(), b"theirs");
    assert_eq!(std::fs::read(&path).unwrap(), b"mine");
}

/// An item that is itself a symbolic link is refused before anything is made, so a destination
/// that appeared meanwhile is left alone.
#[cfg(unix)]
#[test]
fn a_symbolic_link_refused_across_volumes_leaves_a_taken_destination_alone() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let target = part_file(scratch.path(), "Part.stl", b"p");
    let link = scratch.path().join("Link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let taken = |_: &Path, to: &Path| -> io::Result<()> {
        their_folder(to)?;
        Err(io::Error::from(io::ErrorKind::CrossesDevices))
    };
    let ops = FileOps {
        rename: &taken,
        ..SYSTEM
    };

    let error = move_into_directory_with(&link, &trash, &ops).unwrap_err();
    assert!(error.contains("symbolic link"), "{error}");
    assert_eq!(
        tree(&trash.join("Link")),
        vec![
            (String::new(), None),
            ("Theirs".to_string(), None),
            ("Theirs/t.stl".to_string(), Some(b"theirs".to_vec())),
        ],
        "the folder that was there is untouched"
    );
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
}

/// A folder holding a socket (a special file, as a named pipe or a device is) is refused across
/// volumes before the socket is opened: the folder stays as it was and the trash holds nothing.
/// The scratch folder sits right under the temp folder with a short name, since a socket's path
/// is limited to about 100 bytes.
#[cfg(unix)]
#[test]
fn a_folder_holding_a_socket_stays_put_across_volumes() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let scratch = Scratch(std::env::temp_dir().join(format!("xv{}-{n}", std::process::id())));
    let _ = std::fs::remove_dir_all(scratch.path());
    let folder = scratch.path().join("S");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("a.stl"), b"a").unwrap();
    let socket = folder.join("s");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let trash = scratch.path().join("Trash");
    let ops = FileOps {
        rename: &cross_device,
        ..SYSTEM
    };

    let error = move_into_directory_with(&folder, &trash, &ops).unwrap_err();
    assert!(
        error.contains("a socket can't be moved to another volume"),
        "{error}"
    );
    assert_eq!(std::fs::read(folder.join("a.stl")).unwrap(), b"a");
    use std::os::unix::fs::FileTypeExt;
    assert!(std::fs::symlink_metadata(&socket)
        .unwrap()
        .file_type()
        .is_socket());
    assert_eq!(tree(&trash), vec![(String::new(), None)]);
    drop(listener);
}

/// A file whose original is gone by the time the copy is whole (another program removed it) keeps
/// its copy, the only one left, and the error says where it is.
#[test]
fn a_file_whose_original_vanishes_after_the_copy_keeps_the_copy_and_names_it() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let path = part_file(scratch.path(), "Part.stl", b"p");
    let copy_then_vanish = |from: &Path, to: &Path| {
        let n = std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
        Ok(n)
    };
    let ops = FileOps {
        rename: &cross_device,
        copy_file: &copy_then_vanish,
    };

    let error = move_into_directory_with(&path, &trash, &ops).unwrap_err();
    let copy = trash.join("Part.stl");
    assert!(error.contains(&copy.display().to_string()), "{error}");
    assert_eq!(std::fs::read(&copy).unwrap(), b"p");
}

/// Makes `folder` read-only or writable again (so the scratch folder can be removed).
#[cfg(unix)]
fn set_read_only(folder: &Path, read_only: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if read_only { 0o555 } else { 0o755 };
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A file whose original can't be removed once the copy is whole (its folder turned read-only)
/// has its copy removed again: the file stays where it was and the trash holds nothing.
#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_removed_after_the_copy_has_its_copy_removed() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let parts = scratch.path().join("Parts");
    std::fs::create_dir_all(&parts).unwrap();
    let path = part_file(&parts, "Part.stl", b"p");
    let copy_then_lock = |from: &Path, to: &Path| {
        let n = std::fs::copy(from, to)?;
        set_read_only(from.parent().unwrap(), true);
        Ok(n)
    };
    let ops = FileOps {
        rename: &cross_device,
        copy_file: &copy_then_lock,
    };

    let result = move_into_directory_with(&path, &trash, &ops);
    set_read_only(&parts, false);
    assert!(result.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"p");
    assert_eq!(tree(&trash), vec![(String::new(), None)]);
}

/// A folder whose original can't be wholly removed once the copy is whole (a folder in it turned
/// read-only) keeps its whole copy, and the error says where it is.
#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_removed_after_the_copy_keeps_the_copy_and_names_it() {
    let scratch = Scratch::new();
    let trash = scratch.path().join("Trash");
    let folder = brackets(scratch.path());
    let before = tree(&folder);
    let deeper = folder.join("Sub").join("Deeper");
    let copy_then_lock = |from: &Path, to: &Path| {
        let n = std::fs::copy(from, to)?;
        set_read_only(&deeper, true);
        Ok(n)
    };
    let ops = FileOps {
        rename: &cross_device,
        copy_file: &copy_then_lock,
    };

    let result = move_into_directory_with(&folder, &trash, &ops);
    set_read_only(&deeper, false);
    let error = result.unwrap_err();
    let copy = trash.join("Brackets");
    assert!(error.contains(&copy.display().to_string()), "{error}");
    assert_eq!(tree(&copy), before, "the whole copy is kept");
}
