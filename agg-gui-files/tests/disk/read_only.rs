//! Read-only rules (an app's folder of autosaved designs, its shipped samples): the folder allows
//! no change, each write fails before touching the disk, its rows say the folders they open are
//! read-only, and the folders inside it are read-only too. The refusal says only that the folder
//! can't be changed: the caller names the folder the user dropped on, which is not always the
//! folder's name on disk.

use std::rc::Rc;

use agg_gui_files::disk::{DiskFolder, DiskFolderLink};
use agg_gui_files::{
    ContainerActions, CreateFolderItem, Folder, FolderLink, Item, MemoryFolder, WritableFolder,
};

use super::temp::{stl_only, stl_only_read_only, TempDir};

#[test]
fn a_read_only_folder_allows_no_change() {
    let dir = TempDir::new();
    let folder = DiskFolder::new(dir.path(), stl_only_read_only());
    for action in [
        ContainerActions::AddItems,
        ContainerActions::AddContainers,
        ContainerActions::RemoveItems,
        ContainerActions::RenameItems,
    ] {
        assert!(!folder.allow_action(action), "{action:?} is refused");
    }
    let writable = DiskFolder::new(dir.path(), stl_only());
    assert!(writable.allow_action(ContainerActions::AddItems));
}

#[test]
fn every_write_to_a_read_only_folder_fails_and_leaves_the_disk_alone() {
    let dir = TempDir::new();
    let part = dir.file("part.stl", b"p");
    let mut folder = DiskFolder::new(dir.path(), stl_only_read_only());
    folder.load();
    let item: Rc<dyn Item> = folder.items()[0].clone();

    let new_folder: Rc<dyn Item> = Rc::new(CreateFolderItem::new("New"));
    assert!(WritableFolder::add(&mut folder, &[new_folder]).is_err());
    assert!(!dir.path().join("New").exists());

    assert!(WritableFolder::rename(&mut folder, &item, "renamed").is_err());
    assert!(part.is_file());

    assert!(WritableFolder::remove(&mut folder, std::slice::from_ref(&item)).is_err());
    assert!(part.is_file());

    let mut elsewhere = MemoryFolder::new("Elsewhere");
    let incoming = elsewhere.add_file("gear.stl", b"g".to_vec());
    assert!(folder.move_items(&[incoming], &mut elsewhere).is_err());
    assert!(!dir.path().join("gear.stl").exists());

    let out_dir = TempDir::new();
    let mut out = DiskFolder::new(out_dir.path(), stl_only());
    let (moved, result) = out.move_from_disk(&[item], &mut folder);
    assert!(result.is_err(), "nothing moves out of a read-only folder");
    assert!(moved.is_empty());
    assert!(part.is_file());
}

#[test]
fn the_folders_inside_a_read_only_folder_are_read_only() {
    let dir = TempDir::new();
    dir.folder("Inner");
    let mut folder = DiskFolder::new(dir.path(), stl_only_read_only());
    folder.load();
    let link = folder.child_containers()[0].clone();
    assert!(link.is_read_only());
    let opened = link.get_container().expect("a folder row opens");
    let writable = opened
        .as_writable()
        .expect("a folder on disk is writable in kind");
    assert!(!writable.allow_action(ContainerActions::AddItems));

    let open_row = DiskFolderLink::new(dir.path().join("Inner"), stl_only());
    assert!(!open_row.is_read_only());
}

#[test]
fn a_refused_write_says_the_folder_cant_be_changed_without_its_name_on_disk() {
    let dir = TempDir::new();
    let mut folder = DiskFolder::new(dir.path(), stl_only_read_only());
    let new_folder: Rc<dyn Item> = Rc::new(CreateFolderItem::new("New"));
    assert_eq!(
        WritableFolder::add(&mut folder, &[new_folder]),
        Err("This folder can't be changed".to_string())
    );
    let mut elsewhere = MemoryFolder::new("Elsewhere");
    let incoming = elsewhere.add_file("gear.stl", b"g".to_vec());
    assert_eq!(
        folder.move_items(&[incoming], &mut elsewhere),
        Err("This folder can't be changed".to_string())
    );
}
