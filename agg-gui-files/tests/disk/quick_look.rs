//! Whether a folder has folders, for a folder tree's expand arrow (`has_child_containers`). A
//! folder on disk answers from its names alone: sub-folders count, files do not, and a missing
//! folder has none, matching what `load` lists. A row the app marks unknown and a network share
//! answer `None` without reading anything.

use agg_gui_files::disk::{DiskFolder, DiskFolderLink};
use agg_gui_files::{Folder, FolderLink};

use super::temp::{stl_only, TempDir};

/// What `load` lists, to check the quick answer against.
fn loaded_has_folders(link: &DiskFolderLink) -> bool {
    let mut folder = link.get_container().unwrap();
    folder.load();
    !folder.child_containers().is_empty()
}

#[test]
fn a_folder_of_files_has_no_folders_and_a_sub_folder_makes_one() {
    let dir = TempDir::new();
    dir.file("part.stl", b"solid x");
    dir.file("notes.txt", b"x");
    let link = DiskFolderLink::new(dir.path(), stl_only());
    assert_eq!(link.has_child_containers(), Some(false));
    assert!(!loaded_has_folders(&link));

    dir.folder("Brackets");
    assert_eq!(link.has_child_containers(), Some(true));
    assert!(loaded_has_folders(&link));
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    assert_eq!(folder.has_child_containers(), Some(true));
}

#[test]
fn a_missing_folder_has_no_folders() {
    let dir = TempDir::new();
    let link = DiskFolderLink::new(dir.path().join("gone"), stl_only());
    assert_eq!(link.has_child_containers(), Some(false));
    assert!(!loaded_has_folders(&link));
}

#[test]
fn a_folder_the_user_added_is_not_read_to_decide_the_arrow() {
    let dir = TempDir::new();
    dir.folder("Brackets");
    let added = DiskFolderLink::new(dir.path(), stl_only()).with_child_containers_unknown();
    assert_eq!(added.has_child_containers(), None);
}

#[cfg(windows)]
#[test]
fn a_network_share_is_not_read_to_decide_the_arrow() {
    use agg_gui_files::disk::folder_has_child_containers;
    let share = std::path::Path::new(r"\\no-such-server.invalid\share\Parts");
    assert_eq!(folder_has_child_containers(share), None);
}
