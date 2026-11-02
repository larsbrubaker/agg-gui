//! Disk item IDs keep the exact text they have always had: an app's thumbnail cache (MatterCAD's)
//! is keyed by them, so a changed ID would throw every cached picture away and render each tile
//! again. The expected strings were computed by MatterCAD's own code
//! (`mattercad-model`'s `file_system_item::file_system_id` and `round_trip_utc`, FNV-1a from
//! `vector_math::long_hash`), compiled unchanged with these inputs.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agg_gui_files::disk::{disk_item_id_at, round_trip_utc, AllFiles, DiskFile, DiskFolderLink};
use agg_gui_files::Item;

use super::temp::TempDir;

/// (path, seconds and nanoseconds since 1970 UTC, round-trip text, ID) as MatterCAD computes them.
const GOLDEN: &[(&str, u64, u32, &str, &str)] = &[
    (
        "/Users/ana/Designs/bracket.stl",
        1_709_622_489,
        123_456_700,
        "2024-03-05T07:08:09.1234567Z",
        "8307865409876948153",
    ),
    (
        "C:\\Users\\Ana\\Documents\\Café Ünïcode 🚀.mcx",
        1_000_000_000,
        0,
        "2001-09-09T01:46:40.0000000Z",
        "2576378744409901786",
    ),
    (
        "/home/ana/Library/Brackets",
        0,
        0,
        "1970-01-01T00:00:00.0000000Z",
        "3120864160485598855",
    ),
    (
        "",
        4_102_444_799,
        999_999_999,
        "2099-12-31T23:59:59.9999999Z",
        "5511172581038008006",
    ),
];

#[test]
fn ids_match_the_ids_mattercad_has_always_given() {
    for (path, secs, nanos, time_text, id) in GOLDEN {
        let since_epoch = Duration::new(*secs, *nanos);
        assert_eq!(
            round_trip_utc(since_epoch),
            *time_text,
            "time text of {path:?}"
        );
        assert_eq!(
            disk_item_id_at(Path::new(path), since_epoch),
            *id,
            "ID of {path:?}"
        );
    }
}

#[test]
fn a_file_and_a_folder_take_their_id_from_path_and_write_time() {
    let dir = TempDir::new();
    let written = UNIX_EPOCH + Duration::new(1_709_622_489, 123_456_700);
    let file_path = dir.file("bracket.stl", b"solid");
    let file = std::fs::File::options()
        .write(true)
        .open(&file_path)
        .expect("the file opens");
    file.set_modified(written).expect("the write time is set");
    drop(file);
    let since_epoch = written.duration_since(UNIX_EPOCH).expect("after 1970");
    assert_eq!(
        DiskFile::new(&file_path).id(),
        disk_item_id_at(&file_path, since_epoch)
    );

    let folder_path = dir.folder("Brackets");
    let modified = std::fs::metadata(&folder_path)
        .and_then(|m| m.modified())
        .expect("the folder has a write time");
    let link = DiskFolderLink::new(&folder_path, std::rc::Rc::new(AllFiles));
    assert_eq!(
        link.id(),
        disk_item_id_at(
            &folder_path,
            modified.duration_since(UNIX_EPOCH).expect("after 1970")
        )
    );
}

#[test]
fn writing_a_file_changes_its_id() {
    let dir = TempDir::new();
    let path = dir.file("part.stl", b"one");
    let set = |when: SystemTime| {
        let file = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("the file opens");
        file.set_modified(when).expect("the write time is set");
    };
    set(UNIX_EPOCH + Duration::from_secs(1_000_000_000));
    let before = DiskFile::new(&path).id().to_string();
    set(UNIX_EPOCH + Duration::from_secs(1_000_000_001));
    assert_ne!(DiskFile::new(&path).id(), before);
}
