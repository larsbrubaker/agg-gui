//! The changes a folder on disk takes, checked on the disk itself: rename (a file keeps its
//! extension, a taken name is numbered), Create Folder, add (content copied in under a free
//! name), move between two folders on disk (renamed across, both listings fresh, the moves
//! answered for undo), move from a folder elsewhere (copied, the originals kept), and remove (to
//! the trash, from where it can be put back).

use std::cell::Cell;
use std::rc::Rc;

use agg_gui_files::disk::{restore_from_trash, DiskFile, DiskFolder, MovedPath};
use agg_gui_files::{CreateFolderItem, Folder, Item, MemoryFolder, WritableFolder};

use super::temp::{stl_only, TempDir};

fn item_named(folder: &dyn Folder, name: &str) -> Rc<dyn Item> {
    folder
        .items()
        .into_iter()
        .find(|i| i.name() == name)
        .or_else(|| {
            folder
                .child_containers()
                .into_iter()
                .find(|l| l.name() == name)
                .map(|l| l as Rc<dyn Item>)
        })
        .unwrap_or_else(|| panic!("{name} is listed"))
}

fn listed(folder: &dyn Folder) -> Vec<String> {
    let mut names: Vec<String> = folder
        .child_containers()
        .iter()
        .map(|l| l.name().to_string())
        .chain(folder.items().iter().map(|i| i.name().to_string()))
        .collect();
    names.sort();
    names
}

#[test]
fn renaming_a_file_keeps_its_extension_and_numbers_a_taken_name() {
    let dir = TempDir::new();
    dir.file("part.stl", b"p");
    dir.file("Bracket.stl", b"b");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    let part = item_named(&folder, "part.stl");
    let (moved, result) = folder.rename_on_disk(&part, "Bracket");
    result.expect("the rename lands");
    assert_eq!(
        moved,
        Some(MovedPath {
            from: dir.path().join("part.stl"),
            to: dir.path().join("Bracket (1).stl"),
        })
    );
    assert_eq!(listed(&folder), ["Bracket (1).stl", "Bracket.stl"]);
}

#[test]
fn a_file_rename_is_told_to_whoever_keeps_files_by_path() {
    let dir = TempDir::new();
    let path = dir.file("old.stl", b"o");
    let heard = Rc::new(Cell::new(false));
    let expected_to = dir.path().join("new.stl");
    let heard_in = heard.clone();
    agg_gui_files::disk::subscribe_file_renamed(Rc::new(move |from, to| {
        if from == path && to == expected_to {
            heard_in.set(true);
        }
    }));
    let mut file = DiskFile::new(dir.path().join("old.stl"));
    file.set_name("new.stl").expect("the rename lands");
    assert!(heard.get());
    assert_eq!(file.name(), "new.stl");
}

#[test]
fn renaming_a_folder_numbers_a_taken_name() {
    let dir = TempDir::new();
    dir.folder("Old");
    dir.folder("Taken");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    let old = item_named(&folder, "Old");
    WritableFolder::rename(&mut folder, &old, "Taken").expect("the rename lands");
    assert!(dir.path().join("Taken (1)").is_dir());
    assert_eq!(listed(&folder), ["Taken", "Taken (1)"]);
}

#[test]
fn create_folder_makes_the_folder_and_lists_it() {
    let dir = TempDir::new();
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    let changed = Rc::new(Cell::new(0));
    let changed_in = changed.clone();
    folder.subscribe_content_changed(Rc::new(move || changed_in.set(changed_in.get() + 1)));
    let new_folder: Rc<dyn Item> = Rc::new(CreateFolderItem::new("Brackets"));
    WritableFolder::add(&mut folder, &[new_folder]).expect("the folder is made");
    assert!(dir.path().join("Brackets").is_dir());
    assert_eq!(listed(&folder), ["Brackets"]);
    assert_eq!(
        changed.get(),
        1,
        "a write that changed the folder raises content-changed"
    );
}

#[test]
fn adding_a_file_copies_its_content_under_a_free_name() {
    let source = TempDir::new();
    let dir = TempDir::new();
    dir.file("part.stl", b"already here");
    let original = source.file("part.stl", b"new content");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    let item: Rc<dyn Item> = Rc::new(DiskFile::new(&original));
    WritableFolder::add(&mut folder, &[item]).expect("the add lands");
    assert_eq!(
        std::fs::read(dir.path().join("part (1).stl")).expect("the copy is there"),
        b"new content"
    );
    assert!(original.is_file(), "an add copies, the original stays");
}

#[test]
fn moving_between_folders_on_disk_renames_across_and_answers_each_move() {
    let from_dir = TempDir::new();
    let to_dir = TempDir::new();
    from_dir.file("part.stl", b"p");
    from_dir.folder("Sub");
    to_dir.file("part.stl", b"taken");
    let mut source = DiskFolder::new(from_dir.path(), stl_only());
    source.load();
    let mut target = DiskFolder::new(to_dir.path(), stl_only());
    target.load();
    let items = vec![item_named(&source, "part.stl"), item_named(&source, "Sub")];
    let (moved, result) = target.move_from_disk(&items, &mut source);
    result.expect("the move lands");
    assert_eq!(
        moved,
        [
            MovedPath {
                from: from_dir.path().join("part.stl"),
                to: to_dir.path().join("part (1).stl"),
            },
            MovedPath {
                from: from_dir.path().join("Sub"),
                to: to_dir.path().join("Sub"),
            },
        ]
    );
    assert!(listed(&source).is_empty(), "the source lists itself again");
    assert_eq!(listed(&target), ["Sub", "part (1).stl", "part.stl"]);
}

#[test]
fn move_items_from_disk_goes_through_the_writable_folder() {
    let from_dir = TempDir::new();
    let to_dir = TempDir::new();
    from_dir.file("part.stl", b"p");
    let mut source = DiskFolder::new(from_dir.path(), stl_only());
    source.load();
    let mut target = DiskFolder::new(to_dir.path(), stl_only());
    let items = vec![item_named(&source, "part.stl")];
    target
        .move_items(&items, &mut source)
        .expect("the move lands");
    assert!(!from_dir.path().join("part.stl").exists());
    assert!(to_dir.path().join("part.stl").is_file());
}

#[test]
fn moving_in_from_elsewhere_copies_and_keeps_the_originals() {
    let to_dir = TempDir::new();
    let mut elsewhere = MemoryFolder::new("Elsewhere");
    let item = elsewhere.add_file("gear.stl", b"gear".to_vec());
    let mut target = DiskFolder::new(to_dir.path(), stl_only());
    target
        .move_items(&[item], &mut elsewhere)
        .expect("the copy lands");
    assert_eq!(
        std::fs::read(to_dir.path().join("gear.stl")).expect("copied"),
        b"gear"
    );
    elsewhere.load();
    assert_eq!(
        elsewhere.items().len(),
        1,
        "the original stays where it was"
    );
}

#[test]
fn removing_sends_to_the_trash_and_restoring_puts_it_back() {
    let dir = TempDir::new();
    dir.file("part.stl", b"p");
    dir.folder("Sub");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    let items = vec![item_named(&folder, "part.stl"), item_named(&folder, "Sub")];
    let (trashed, result) = folder.remove_to_trash(&items);
    result.expect("the remove lands");
    assert_eq!(trashed.len(), 2);
    assert!(listed(&folder).is_empty());
    for item in &trashed {
        assert!(
            item.in_trash.exists(),
            "{} is in the trash",
            item.in_trash.display()
        );
    }
    for item in &trashed {
        restore_from_trash(item).expect("put back");
    }
    folder.load();
    assert_eq!(listed(&folder), ["Sub", "part.stl"]);
}

#[test]
fn removing_an_item_already_gone_is_no_failure() {
    let dir = TempDir::new();
    let path = dir.file("part.stl", b"p");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    let item = item_named(&folder, "part.stl");
    std::fs::remove_file(path).expect("removed outside the app");
    WritableFolder::remove(&mut folder, &[item]).expect("nothing left to trash is fine");
}
