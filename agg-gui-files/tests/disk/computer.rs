//! The Computer folder: the user's Downloads, Desktop and Documents (each only when it exists)
//! then the drives, every row opening a folder on disk with the app's rules and its own sort; a
//! search covers the user's folders, never the drives; a drive row has folders without being
//! read.

use std::rc::Rc;

use agg_gui_files::disk::{drive_roots, ComputerFolder, ComputerFolders, DiskFolder};
use agg_gui_files::{Folder, FolderLink, SortBehavior, SortKey};

use super::temp::{stl_only, TempDir};

fn names(links: &[Rc<dyn FolderLink>]) -> Vec<String> {
    links.iter().map(|l| l.name().to_string()).collect()
}

#[test]
fn computer_lists_the_folders_it_is_given_then_the_drives() {
    let dir = TempDir::new();
    let folders = ComputerFolders {
        downloads: dir.folder("Downloads"),
        desktop: dir.folder("Desktop"),
        documents: dir.folder("Documents"),
        drives: vec![
            ("C:\\".into(), dir.folder("C")),
            ("D:\\".into(), dir.folder("D")),
        ],
    };
    let mut computer = ComputerFolder::new(folders, stl_only());
    computer.load();
    assert_eq!(computer.name(), "Computer");
    assert_eq!(
        names(&computer.child_containers()),
        ["Downloads", "Desktop", "Documents", "C:\\", "D:\\"]
    );
    assert_eq!(
        names(
            &computer
                .search_sections()
                .expect("Computer searches its user folders")
        ),
        ["Downloads", "Desktop", "Documents"]
    );
    let drive = computer.child_containers()[3].clone();
    assert_eq!(
        drive.has_child_containers(),
        Some(true),
        "a drive is not read"
    );
}

#[test]
fn a_user_folder_that_does_not_exist_is_left_out() {
    let dir = TempDir::new();
    let folders = ComputerFolders {
        downloads: dir.path().join("No Downloads"),
        desktop: dir.folder("Desktop"),
        documents: dir.path().join("No Documents"),
        drives: Vec::new(),
    };
    let computer = ComputerFolder::new(folders, stl_only());
    assert_eq!(names(&computer.child_containers()), ["Desktop"]);
}

#[test]
fn each_row_opens_a_folder_on_disk_with_the_rules_and_its_sort() {
    let dir = TempDir::new();
    let downloads = dir.folder("Downloads");
    std::fs::write(downloads.join("part.stl"), b"p").unwrap();
    std::fs::write(downloads.join("notes.txt"), b"t").unwrap();
    let folders = ComputerFolders {
        downloads,
        desktop: dir.folder("Desktop"),
        documents: dir.path().join("none"),
        drives: Vec::new(),
    };
    let computer = ComputerFolder::new(folders, stl_only());
    let rows = computer.child_containers();
    let mut opened = rows[0].get_container().expect("Downloads opens");
    opened.load();
    assert!(opened
        .as_any()
        .is_some_and(|a| a.downcast_ref::<DiskFolder>().is_some()));
    let listed: Vec<String> = opened
        .items()
        .iter()
        .map(|i| i.name().to_string())
        .collect();
    assert_eq!(listed, ["part.stl"], "the app's rules list the files");
    assert_eq!(
        opened.default_sort(),
        Some(SortBehavior::new(SortKey::ModifiedDate, false)),
        "Downloads lists newest first"
    );
    let desktop = rows[1].get_container().expect("Desktop opens");
    assert_eq!(
        desktop.default_sort(),
        Some(SortBehavior::new(SortKey::Name, true))
    );
}

#[test]
fn this_computer_has_a_drive_and_its_rows_have_folders_unread() {
    let dir = TempDir::new();
    let folders = ComputerFolders::with_this_computers_drives(
        dir.path().join("a"),
        dir.path().join("b"),
        dir.path().join("c"),
    );
    assert!(!drive_roots().is_empty(), "the computer has a drive");
    let computer = ComputerFolder::new(folders, stl_only());
    let rows = computer.child_containers();
    assert_eq!(rows.len(), drive_roots().len());
    for row in rows {
        assert_eq!(row.has_child_containers(), Some(true), "{}", row.name());
    }
}
