//! Recoverable deletes for folders on disk (`agg_gui_files::disk`'s trash): a remove sends files
//! and folders to a trash and records where they went, and a restore puts them back at their old
//! paths with their content. Every test but the last uses this binary's temporary trash
//! (`temp`); the freedesktop step and the Recycle Bin record reader are pinned on every host;
//! the last round-trips one temporary file through the host's own trash and back, since that is
//! the only way to check the system's call, and leaves nothing behind.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use agg_gui_files::disk::{
    move_to_system_trash, move_to_trash, newest_recycle_bin_match, parse_recycle_bin_record,
    restore_from_trash, DiskFolder, TrashedItem,
};
use agg_gui_files::{Folder, Item};

use super::temp::{stl_only, TempDir};

fn names(container: &DiskFolder) -> Vec<String> {
    let mut names: Vec<String> = container
        .child_containers()
        .iter()
        .map(|c| c.name().to_string())
        .chain(container.items().iter().map(|i| i.name().to_string()))
        .collect();
    names.sort();
    names
}

#[test]
fn removed_items_go_to_the_trash_and_restore_with_their_content() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.path().join("Brackets")).unwrap();
    std::fs::write(
        dir.path().join("Brackets").join("inner.stl"),
        b"solid a\nendsolid a\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("Part.stl"), b"solid p\nendsolid p\n").unwrap();
    let mut container = DiskFolder::new(dir.path(), stl_only());
    container.load();
    let mut items: Vec<Rc<dyn Item>> = container.items();
    items.extend(
        container
            .child_containers()
            .into_iter()
            .map(|c| c as Rc<dyn Item>),
    );

    let (trashed, result) = container.remove_to_trash(&items);
    result.unwrap();
    assert_eq!(trashed.len(), 2);
    assert!(names(&container).is_empty());
    for item in &trashed {
        assert!(!item.original.exists());
        assert!(
            item.in_trash.exists(),
            "{:?} is in the trash",
            item.in_trash
        );
    }

    for item in &trashed {
        restore_from_trash(item).unwrap();
    }
    container.load();
    assert_eq!(names(&container), vec!["Brackets", "Part.stl"]);
    assert_eq!(
        std::fs::read(dir.path().join("Brackets").join("inner.stl")).unwrap(),
        b"solid a\nendsolid a\n"
    );
}

#[test]
fn two_items_with_one_name_keep_separate_places_in_the_trash() {
    let (a, b) = (TempDir::new(), TempDir::new());
    for (dir, text) in [(&a, "one"), (&b, "two")] {
        std::fs::write(dir.path().join("Same.stl"), text).unwrap();
    }
    let first = move_to_trash_in(&a.path().join("Same.stl"));
    let second = move_to_trash_in(&b.path().join("Same.stl"));
    assert_ne!(first.in_trash, second.in_trash);
    restore_from_trash(&second).unwrap();
    restore_from_trash(&first).unwrap();
    assert_eq!(
        std::fs::read_to_string(a.path().join("Same.stl")).unwrap(),
        "one"
    );
    assert_eq!(
        std::fs::read_to_string(b.path().join("Same.stl")).unwrap(),
        "two"
    );
}

fn move_to_trash_in(path: &Path) -> TrashedItem {
    move_to_trash(path).unwrap()
}

#[test]
fn restore_never_overwrites_a_new_item_with_the_old_name() {
    let dir = TempDir::new();
    let path = dir.path().join("Part.stl");
    std::fs::write(&path, "old").unwrap();
    let trashed = move_to_trash_in(&path);
    std::fs::write(&path, "new").unwrap();
    assert!(restore_from_trash(&trashed).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    assert!(trashed.in_trash.exists(), "the old item stays in the trash");
}

#[cfg(unix)]
#[test]
fn freedesktop_trash_writes_the_record_file_managers_read() {
    use agg_gui_files::disk::move_to_freedesktop_trash;
    let dir = TempDir::new();
    let root = dir.path().join("Trash");
    let folder = dir.path().join("My Parts");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Gear 50%.stl"), "g").unwrap();
    let first = move_to_freedesktop_trash(&folder.join("Gear 50%.stl"), &root).unwrap();
    assert_eq!(first.in_trash, root.join("files").join("Gear 50%.stl"));
    let info = first.info_file.clone().unwrap();
    assert_eq!(info, root.join("info").join("Gear 50%.stl.trashinfo"));
    let text = std::fs::read_to_string(&info).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "[Trash Info]");
    let encoded = format!(
        "Path={}/My%20Parts/Gear%2050%25.stl",
        std::path::absolute(dir.path())
            .unwrap()
            .to_string_lossy()
            .replace(' ', "%20")
    );
    assert_eq!(lines[1], encoded);
    let date = lines[2].strip_prefix("DeletionDate=").unwrap();
    assert_eq!(date.len(), 19);
    assert_eq!(&date[4..5], "-");
    assert_eq!(&date[10..11], "T");

    // A second item of the same name gets its own name and record.
    std::fs::write(folder.join("Gear 50%.stl"), "h").unwrap();
    let second = move_to_freedesktop_trash(&folder.join("Gear 50%.stl"), &root).unwrap();
    assert_eq!(second.in_trash, root.join("files").join("Gear 50% 2.stl"));

    restore_from_trash(&second).unwrap();
    assert!(
        !second.info_file.unwrap().exists(),
        "the record goes with the item"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("Gear 50%.stl")).unwrap(),
        "h"
    );
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A Windows 10+ `$I` record: version 2, the item's size, its deletion time, then the path's
/// length (null included) and characters.
fn v2_record(path: &str, deleted: u64) -> Vec<u8> {
    let mut v2 = Vec::new();
    v2.extend(2u64.to_le_bytes());
    v2.extend(1234u64.to_le_bytes());
    v2.extend(deleted.to_le_bytes());
    v2.extend((path.encode_utf16().count() as u32 + 1).to_le_bytes());
    v2.extend(utf16(path));
    v2.extend([0, 0]);
    v2
}

/// A Vista to 8.1 `$I` record: version 1, size, deletion time, and a 260-character path field.
fn v1_record(path: &str, deleted: u64) -> Vec<u8> {
    let mut v1 = Vec::new();
    v1.extend(1u64.to_le_bytes());
    v1.extend(1234u64.to_le_bytes());
    v1.extend(deleted.to_le_bytes());
    let mut field = utf16(path);
    field.resize(520, 0);
    v1.extend(field);
    v1
}

#[test]
fn recycle_bin_records_name_the_original_path_and_time() {
    let path = r"C:\Users\Lars\Designs\Bracket.mcx";
    let v2 = v2_record(path, 99);
    assert_eq!(parse_recycle_bin_record(&v2), Some((99, path.to_string())));

    let v1 = v1_record(path, 7);
    assert_eq!(parse_recycle_bin_record(&v1), Some((7, path.to_string())));

    assert_eq!(parse_recycle_bin_record(&v2[..20]), None, "a cut record");
    let mut unknown = v2.clone();
    unknown[0] = 9;
    assert_eq!(
        parse_recycle_bin_record(&unknown),
        None,
        "an unknown version"
    );
}

/// A path held in its short 8.3 spelling (GitHub's Windows runners set `%TEMP%` to
/// `C:\Users\RUNNER~1\...`) is found by the long spelling the Recycle Bin records, in either
/// record version, without case; the newest recycle of the path wins, and the `$R` item beside
/// its record is the one handed back for a restore.
#[test]
fn recycle_bin_finds_the_newest_record_of_a_path_by_any_spelling() {
    let short = r"C:\Users\RUNNER~1\AppData\Local\Temp\Parts\Gear.stl";
    let long = r"C:\Users\runneradmin\AppData\Local\Temp\Parts\Gear.stl";
    let bin = PathBuf::from("bin").join("S-1-5-21-1");
    let other = PathBuf::from("bin").join("S-1-5-21-2");
    let records = vec![
        (bin.join("$IAAAAAA.stl"), v1_record(long, 10)),
        // The newest recycle of this path, recorded in another case.
        (
            other.join("$IBBBBBB.stl"),
            v2_record(&long.to_uppercase(), 30),
        ),
        (bin.join("$ICCCCCC.stl"), v2_record(long, 20)),
        // Newer, but another file.
        (
            bin.join("$IDDDDDD.stl"),
            v2_record(r"C:\Elsewhere\Gear.stl", 90),
        ),
        // A recycled item's content is never taken for a record, even when it parses as one.
        (bin.join("$REEEEEE.stl"), v2_record(long, 99)),
    ];
    let wanted = [short.to_string(), long.to_string()];
    assert_eq!(
        newest_recycle_bin_match(records.clone(), &wanted),
        Some((other.join("$RBBBBBB.stl"), other.join("$IBBBBBB.stl")))
    );

    // Matching only the spelling the caller held finds nothing: the record has the long one.
    assert_eq!(
        newest_recycle_bin_match(records.clone(), &[short.to_string()]),
        None
    );
    let v1_only = vec![(bin.join("$IAAAAAA"), v1_record(long, 10))];
    assert_eq!(
        newest_recycle_bin_match(v1_only, &wanted),
        Some((bin.join("$RAAAAAA"), bin.join("$IAAAAAA"))),
        "a version 1 record of a folder (no extension)"
    );
}

/// The host's real trash: the file goes in and comes back. Leaves nothing behind.
#[test]
fn system_trash_round_trip() {
    let dir = TempDir::new();
    let path = dir.path().join(format!(
        "agg-gui-files trash test {}.stl",
        std::process::id()
    ));
    std::fs::write(&path, "solid t\nendsolid t\n").unwrap();
    let trashed = move_to_system_trash(&path).unwrap();
    assert!(!path.exists());
    assert!(trashed.in_trash.exists(), "{:?}", trashed.in_trash);
    restore_from_trash(&trashed).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "solid t\nendsolid t\n"
    );
}
