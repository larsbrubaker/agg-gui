//! What a folder on disk lists: its sub-folders by name and the files its rules accept by path,
//! its `index.md` as the header, a message row saying why a folder it cannot read is empty, and
//! the same rules carried down to the folders its rows open.

use std::rc::Rc;

use agg_gui_files::disk::{AllFiles, DiskFile, DiskFolder};
use agg_gui_files::{Folder, Item, MessageItem};

use super::temp::{stl_only, TempDir};

fn names_of_items(folder: &DiskFolder) -> Vec<String> {
    folder
        .items()
        .iter()
        .map(|i| i.name().to_string())
        .collect()
}

fn names_of_folders(folder: &DiskFolder) -> Vec<String> {
    folder
        .child_containers()
        .iter()
        .map(|l| l.name().to_string())
        .collect()
}

#[test]
fn a_folder_lists_its_folders_and_the_files_its_rules_accept() {
    let dir = TempDir::new();
    dir.file("b.stl", b"b");
    dir.file("A.STL", b"a");
    dir.file("notes.txt", b"not listed");
    dir.file("index.md", b"# Brackets");
    dir.folder("zeta");
    dir.folder("Alpha");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    assert_eq!(names_of_items(&folder), ["A.STL", "b.stl"]);
    assert_eq!(names_of_folders(&folder), ["Alpha", "zeta"]);
    assert_eq!(folder.header_markdown(), "# Brackets");
    let first = folder.items()[0].clone();
    let file = first
        .as_any()
        .and_then(|a| a.downcast_ref::<DiskFile>())
        .expect("a listed file is a DiskFile");
    assert_eq!(file.file_path(), dir.path().join("A.STL"));
    assert_eq!(file.file_size(), 1);
    let stream = first.as_asset_stream().expect("a file streams");
    assert_eq!(stream.get_stream().expect("readable"), Some(b"a".to_vec()));
    assert_eq!(
        first.as_asset().expect("a file is content").content_type(),
        "stl"
    );
}

#[test]
fn every_file_lists_under_rules_that_accept_all() {
    let dir = TempDir::new();
    dir.file("notes.txt", b"t");
    dir.file("part.stl", b"s");
    let mut folder = DiskFolder::new(dir.path(), Rc::new(AllFiles));
    folder.load();
    assert_eq!(names_of_items(&folder), ["notes.txt", "part.stl"]);
}

#[test]
fn a_sub_folder_opens_with_the_same_rules() {
    let dir = TempDir::new();
    let inner = dir.folder("Inner");
    std::fs::write(inner.join("deep.stl"), b"d").expect("written");
    std::fs::write(inner.join("deep.txt"), b"t").expect("written");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    let link = folder.child_containers()[0].clone();
    let mut opened = link.get_container().expect("a folder row opens");
    opened.load();
    assert_eq!(opened.name(), "Inner");
    let names: Vec<String> = opened
        .items()
        .iter()
        .map(|i| i.name().to_string())
        .collect();
    assert_eq!(names, ["deep.stl"]);
}

#[test]
fn a_missing_folder_lists_a_row_saying_so() {
    let dir = TempDir::new();
    let mut folder = DiskFolder::new(dir.path().join("Gone"), stl_only());
    folder.load();
    assert!(folder.child_containers().is_empty());
    let items = folder.items();
    assert_eq!(items.len(), 1);
    assert!(items[0]
        .as_any()
        .is_some_and(|a| a.downcast_ref::<MessageItem>().is_some()));
    assert_eq!(items[0].name(), "Missing directory: Gone");
}

#[test]
fn extra_items_list_after_the_folders_own_files() {
    let dir = TempDir::new();
    dir.file("part.stl", b"s");
    let extra: Rc<dyn Item> = Rc::new(MessageItem::new("Built-in tool"));
    let mut folder = DiskFolder::new(dir.path(), stl_only()).with_extra_items(vec![extra]);
    folder.load();
    assert_eq!(names_of_items(&folder), ["part.stl", "Built-in tool"]);
}

#[test]
fn a_removed_index_takes_its_header_with_it() {
    let dir = TempDir::new();
    let index = dir.file("index.md", b"About");
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    assert_eq!(folder.header_markdown(), "About");
    std::fs::remove_file(index).expect("removed");
    folder.load();
    assert_eq!(folder.header_markdown(), "");
}
