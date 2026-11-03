//! Where a folder on disk is: on the computer, and known by its path. A folder and its row give
//! the same place and the same location key however the path was spelled, so a folder tree finds
//! the row of the folder it is showing; two folders never share a key.

use std::rc::Rc;

use agg_gui_files::disk::{disk_location_key, AllFiles, DiskFolder, DiskFolderLink};
use agg_gui_files::{Folder, FolderLink, PlaceId};

use super::temp::TempDir;

#[test]
fn a_folder_and_its_row_are_on_the_computer() {
    let dir = TempDir::new();
    let inner = dir.folder("Inner");
    let folder = DiskFolder::new(&inner, Rc::new(AllFiles));
    let link = DiskFolderLink::new(&inner, Rc::new(AllFiles));
    assert_eq!(folder.place(), Some(PlaceId::COMPUTER));
    assert_eq!(link.place(), Some(PlaceId::COMPUTER));
}

#[test]
fn a_folder_and_its_row_share_a_key_however_the_path_is_spelled() {
    let dir = TempDir::new();
    let inner = dir.folder("Inner");
    let spelled_oddly = dir.path().join(".").join("Inner").join("");
    let folder = DiskFolder::new(&spelled_oddly, Rc::new(AllFiles));
    let link = DiskFolderLink::new(&inner, Rc::new(AllFiles));
    let key = folder.location_key().expect("a folder on disk has a key");
    assert_eq!(link.location_key().as_deref(), Some(key.as_str()));
    assert_eq!(key, disk_location_key(&inner));
    assert!(!key.contains('\0'), "a folder's key never holds a NUL");
}

#[test]
fn two_folders_never_share_a_key() {
    let dir = TempDir::new();
    let a = DiskFolder::new(dir.folder("A"), Rc::new(AllFiles));
    let b = DiskFolder::new(dir.folder("B"), Rc::new(AllFiles));
    assert_ne!(a.location_key(), b.location_key());
}

#[test]
fn a_listed_row_answers_its_folders_key() {
    let dir = TempDir::new();
    let inner = dir.folder("Inner");
    let mut folder = DiskFolder::new(dir.path(), Rc::new(AllFiles));
    folder.load();
    let link = folder.child_containers()[0].clone();
    let opened = link.get_container().expect("a folder row opens");
    assert_eq!(link.location_key(), opened.location_key());
    assert_eq!(opened.location_key(), Some(disk_location_key(&inner)));
}

#[test]
fn a_folder_key_is_within_its_own_and_every_folder_above_it_and_no_other() {
    use agg_gui_files::disk::disk_key_within;
    let key = |p: &str| disk_location_key(std::path::Path::new(p));
    let root = std::path::MAIN_SEPARATOR_STR;
    let path = |parts: &[&str]| {
        let mut p = std::path::PathBuf::from(root);
        p.extend(parts);
        disk_location_key(&p)
    };
    let designs = path(&["Designs"]);
    let parts = path(&["Designs", "Parts"]);
    assert!(disk_key_within(&designs, &designs));
    assert!(disk_key_within(&parts, &designs));
    assert!(disk_key_within(
        &path(&["Designs", "Parts", "Gears"]),
        &designs
    ));
    assert!(
        disk_key_within(&parts, &key(root)),
        "everything is within the root"
    );
    assert!(
        !disk_key_within(&designs, &parts),
        "a folder is not within one below it"
    );
    assert!(
        !disk_key_within(&path(&["Designs2"]), &designs),
        "a name that only starts the same is beside it"
    );
    assert!(
        !disk_key_within(&path(&["Designs\"", "Parts"]), &designs),
        "a quote in a name is part of the name"
    );
    assert!(disk_key_within(
        &path(&["Designs\"", "Parts"]),
        &path(&["Designs\""])
    ));
}
