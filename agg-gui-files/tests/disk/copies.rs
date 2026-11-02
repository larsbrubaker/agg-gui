//! Copies onto the disk that keep the originals: `copy_in_async` awaits each item's content and
//! copies down a folder that is not on disk with its structure (folders first, then files),
//! listing a folder that lists later through the rules' `list_for_copy`; `add_async` awaits
//! content as `add` takes it; a folder is never copied into itself; a read-only folder copies
//! nothing.

use std::path::Path;
use std::rc::Rc;

use agg_gui_files::disk::{
    gather, read_dir_node, CopyNode, DiskFolder, DiskFolderLink, DiskRules, ListedFolder,
};
use agg_gui_files::{Folder, Item, MemoryFolder};

use super::temp::{block_on, stl_only, stl_only_read_only, TempDir};

fn file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A folder elsewhere: "Kit" holding "Bolts" (with "m3.stl") and "plate.stl".
fn kit() -> (MemoryFolder, Rc<dyn Item>) {
    let root = MemoryFolder::new("Elsewhere");
    let kit = root.add_folder("Kit").expect("made");
    let bolts = kit.add_folder("Bolts").expect("made");
    bolts.add_file("m3.stl", b"m3".to_vec());
    kit.add_file("plate.stl", b"plate".to_vec());
    let link: Rc<dyn Item> = kit.link();
    (root, link)
}

#[test]
fn a_folder_elsewhere_is_copied_down_whole() {
    let dir = TempDir::new();
    let (_root, kit) = kit();
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    let (made, result) = block_on(folder.copy_in_async(&[kit]));
    result.expect("the copy lands");
    assert_eq!(made, [dir.path().join("Kit")]);
    assert_eq!(file(&dir.path().join("Kit").join("plate.stl")), "plate");
    assert_eq!(
        file(&dir.path().join("Kit").join("Bolts").join("m3.stl")),
        "m3"
    );
    let names: Vec<String> = folder
        .child_containers()
        .iter()
        .map(|l| l.name().to_string())
        .collect();
    assert_eq!(names, ["Kit"], "the folder lists itself again");
}

#[test]
fn a_taken_name_is_numbered_and_files_copy_with_their_content() {
    let dir = TempDir::new();
    dir.folder("Kit");
    dir.file("gear.stl", b"old");
    let (_root, kit) = kit();
    let elsewhere = MemoryFolder::new("More");
    let gear = elsewhere.add_file("gear.stl", b"new".to_vec());
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    let (made, result) = block_on(folder.copy_in_async(&[kit, gear]));
    result.expect("the copy lands");
    assert_eq!(
        made,
        [dir.path().join("Kit (1)"), dir.path().join("gear (1).stl")]
    );
    assert_eq!(file(&dir.path().join("gear (1).stl")), "new");
}

#[test]
fn a_folder_is_never_copied_into_itself() {
    let dir = TempDir::new();
    let inner = dir.folder("Inner");
    let link: Rc<dyn Item> = Rc::new(DiskFolderLink::new(dir.path(), stl_only()));
    let mut folder = DiskFolder::new(&inner, stl_only());
    let (made, result) = block_on(folder.copy_in_async(&[link]));
    assert!(made.is_empty());
    assert!(result.is_err(), "the item is named as not copied");
}

#[test]
fn a_read_only_folder_copies_nothing() {
    let dir = TempDir::new();
    let (_root, kit) = kit();
    let mut folder = DiskFolder::new(dir.path(), stl_only_read_only());
    let (made, result) = block_on(folder.copy_in_async(&[kit]));
    assert!(made.is_empty());
    assert!(result.is_err());
    assert!(!dir.path().join("Kit").exists());
}

#[test]
fn add_async_awaits_content_as_add_takes_it() {
    let dir = TempDir::new();
    let elsewhere = MemoryFolder::new("Elsewhere");
    let gear = elsewhere.add_file("gear.stl", b"gear".to_vec());
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    block_on(folder.add_async(&[gear])).expect("the add lands");
    assert_eq!(file(&dir.path().join("gear.stl")), "gear");
    assert_eq!(folder.items().len(), 1);
}

/// Rules that list the copied folder named "Late" themselves, as an app does for a folder whose
/// `load` only starts the listing; the folder it hands back is a memory folder with one file.
struct ListsLateFolders;

impl DiskRules for ListsLateFolders {
    fn lists_file(&self, _name: &str) -> bool {
        true
    }
    fn list_for_copy(&self, link: &dyn Item) -> Option<ListedFolder> {
        if link.name() != "Late" {
            return None;
        }
        Some(Box::pin(async {
            let folder = MemoryFolder::new("Late");
            folder.add_file("listed late.stl", b"late".to_vec());
            Some(Box::new(folder) as Box<dyn Folder>)
        }))
    }
}

#[test]
fn a_folder_that_lists_later_is_listed_by_the_rules() {
    let dir = TempDir::new();
    let root = MemoryFolder::new("Elsewhere");
    let late: Rc<dyn Item> = root.add_folder("Late").expect("made").link();
    let mut folder = DiskFolder::new(dir.path(), Rc::new(ListsLateFolders));
    let (_, result) = block_on(folder.copy_in_async(&[late]));
    result.expect("the copy lands");
    assert_eq!(
        file(&dir.path().join("Late").join("listed late.stl")),
        "late"
    );
}

#[test]
fn a_folder_on_disk_reads_into_a_tree_in_name_order() {
    let dir = TempDir::new();
    let kit = dir.folder("Kit");
    std::fs::write(kit.join("b.stl"), b"b").unwrap();
    std::fs::write(kit.join("a.stl"), b"a").unwrap();
    std::fs::write(kit.join(".DS_Store"), b"x").unwrap();
    let node = read_dir_node(&kit).expect("readable");
    let CopyNode::Folder { name, children } = node else {
        panic!("a folder reads as a folder");
    };
    assert_eq!(name, "Kit");
    let names: Vec<&str> = children
        .iter()
        .map(|c| match c {
            CopyNode::File { file_name, .. } => file_name.as_str(),
            CopyNode::Folder { name, .. } => name.as_str(),
        })
        .collect();
    assert_eq!(names, ["a.stl", "b.stl"]);
    let link: Rc<dyn Item> = Rc::new(DiskFolderLink::new(&kit, stl_only()));
    let (nodes, failures) = block_on(gather(&[link], stl_only().as_ref()));
    assert!(failures.is_empty());
    assert_eq!(nodes.len(), 1);
}
