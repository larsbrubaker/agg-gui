//! An archive listed one directory level at a time: files in archive order, folders in the
//! order first seen, each folder opening the next level; a sibling folder sharing a name's
//! prefix is not listed inside; a missing archive lists a missing-file row and an unreadable one
//! an error row, rather than failing the listing. Entry IDs are MatterCAD's text for text (its
//! thumbnail cache is keyed by them): the expected strings were computed by MatterCAD's own
//! `zip_memory_item::long_hash_code`, compiled unchanged.

use std::path::Path;

use agg_gui_files::zip_folder::{ZipEntry, ZipFolder};
use agg_gui_files::{Folder, Item, MessageItem};

use super::fixture::{zip_of, TempDir};

fn names(folder: &dyn Folder) -> (Vec<String>, Vec<String>) {
    (
        folder
            .child_containers()
            .iter()
            .map(|c| c.name().to_string())
            .collect(),
        folder
            .items()
            .iter()
            .map(|i| i.name().to_string())
            .collect(),
    )
}

#[test]
fn a_missing_zip_lists_a_missing_file_row() {
    let dir = TempDir::new();
    let missing = dir.path().join("missing.zip");
    let mut folder = ZipFolder::new(missing.to_string_lossy(), "");
    folder.load();
    assert_eq!(folder.name(), "missing");
    let items = folder.items();
    assert_eq!(items.len(), 1);
    assert!(items[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<MessageItem>())
        .is_some_and(MessageItem::is_missing_file));
}

#[test]
fn a_zip_lists_one_directory_level_and_opens_its_folders() {
    let dir = TempDir::new();
    let path = dir.path().join("Parts.zip");
    std::fs::write(
        &path,
        zip_of(&[
            ("cube.stl", "solid a\nendsolid a\n"),
            ("Gears/", ""),
            ("Gears/spur.stl", "solid b\nendsolid b\n"),
            ("Gears/Small/pinion.stl", "solid c\nendsolid c\n"),
            ("readme.txt", "hello"),
        ]),
    )
    .unwrap();

    let mut top = ZipFolder::new(path.to_string_lossy(), "");
    top.load();
    assert_eq!(top.name(), "Parts");
    assert!(top.is_protected());
    assert_eq!(
        names(&top),
        (
            vec!["Gears".to_string()],
            vec!["cube.stl".to_string(), "readme.txt".to_string()]
        )
    );

    let gears_link = top.child_containers()[0].clone();
    assert!(gears_link.is_read_only());
    let mut gears = gears_link.get_container().unwrap();
    gears.load();
    assert_eq!(
        names(gears.as_ref()),
        (vec!["Small".to_string()], vec!["spur.stl".to_string()])
    );
    let spur = gears.items()[0].clone();
    let stream = spur.as_asset_stream().unwrap();
    assert_eq!(
        stream.get_stream().unwrap().unwrap(),
        b"solid b\nendsolid b\n"
    );
    assert_eq!(stream.content_type(), "stl");

    let mut small = gears.child_containers()[0].get_container().unwrap();
    small.load();
    assert_eq!(names(small.as_ref()).1, vec!["pinion.stl".to_string()]);
}

#[test]
fn a_sibling_folder_sharing_a_prefix_is_not_listed_inside() {
    let dir = TempDir::new();
    let path = dir.path().join("Kit.zip");
    std::fs::write(
        &path,
        zip_of(&[
            ("Parts/a.stl", "solid\nendsolid\n"),
            ("Parts2/b.stl", "solid\nendsolid\n"),
        ]),
    )
    .unwrap();
    let mut parts = ZipFolder::new(path.to_string_lossy(), "Parts");
    parts.load();
    assert_eq!(names(&parts), (Vec::new(), vec!["a.stl".to_string()]));
}

#[test]
fn an_unreadable_zip_lists_an_error_row() {
    let dir = TempDir::new();
    let path = dir.path().join("Broken.zip");
    std::fs::write(&path, b"not a zip at all").unwrap();
    let mut folder = ZipFolder::new(path.to_string_lossy(), "");
    folder.load();
    let items = folder.items();
    assert_eq!(items.len(), 1);
    assert!(items[0].name().starts_with("Error loading container - "));
    assert!(folder.child_containers().is_empty());
}

#[test]
fn entry_ids_match_the_ids_mattercad_has_always_given() {
    let edge = ZipEntry::new(Path::new("/designs/Knife.zip"), "Blades/edge.stl", 3);
    assert_eq!(edge.id(), "4849624854640651701");
    assert_eq!(edge.name(), "edge.stl");
    let part = ZipEntry::new(Path::new("C:\\Parts\\Kit Ünï.zip"), "part.stl", 0);
    assert_eq!(part.id(), "5033561993003570691");
}

#[test]
fn an_entry_gone_from_the_archive_gives_an_empty_stream() {
    let dir = TempDir::new();
    let path = dir.path().join("Kit.zip");
    std::fs::write(&path, zip_of(&[("a.stl", "a")])).unwrap();
    let gone = ZipEntry::new(&path, "b.stl", 1);
    let stream = gone.as_asset_stream().unwrap();
    assert_eq!(stream.get_stream().unwrap(), Some(Vec::new()));
}
