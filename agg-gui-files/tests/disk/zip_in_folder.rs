//! With both `disk` and `zip`: a zip file in a folder on disk lists as a read-only folder with
//! the zip picture, never as a file; the tree's quick look counts it as a folder; a search finds
//! a matching one as that folder row, never as a file.

use std::io::Write;
use std::rc::Rc;

use agg_gui_files::disk::{AllFiles, DiskFolder, DiskFolderLink};
use agg_gui_files::zip_folder::ZipFolderLink;
use agg_gui_files::{Folder, FolderLink, Item, SearchCancel};

use super::temp::{block_on, TempDir};

fn kit_zip() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("part.stl", zip::write::SimpleFileOptions::default())
        .expect("an entry");
    zip.write_all(b"solid\nendsolid\n").expect("its text");
    zip.finish().expect("the zip").into_inner()
}

#[test]
fn a_zip_file_shows_as_a_read_only_folder_with_the_zip_picture() {
    let dir = TempDir::new();
    dir.file("Kit.zip", &kit_zip());
    dir.folder("Folder");
    let mut folder = DiskFolder::new(dir.path(), Rc::new(AllFiles));
    folder.load();
    let links = folder.child_containers();
    let names: Vec<&str> = links.iter().map(|l| l.name()).collect();
    assert_eq!(names, ["Folder", "Kit.zip"]);
    assert!(folder.items().is_empty(), "the zip is a folder, not a file");
    let zip = &links[1];
    assert!(zip.is_read_only());
    assert_eq!(zip.tile_icon(), Some("zip_folder.png"));
    assert!(zip
        .as_any()
        .is_some_and(|a| a.downcast_ref::<ZipFolderLink>().is_some()));
    let mut opened = zip.get_container().unwrap();
    opened.load();
    assert_eq!(opened.name(), "Kit");
    assert_eq!(opened.items()[0].name(), "part.stl");
}

#[test]
fn the_quick_look_counts_a_zip_file_as_a_folder() {
    let dir = TempDir::new();
    dir.file("notes.txt", b"x");
    let link = DiskFolderLink::new(dir.path(), Rc::new(AllFiles));
    assert_eq!(link.has_child_containers(), Some(false));
    dir.file("parts.ZIP", b"");
    assert_eq!(link.has_child_containers(), Some(true));
}

/// A zip file whose name matches is found as the folder row the listing shows for it, before the
/// files beside it, never as a file; one whose name does not match is not found.
#[test]
fn a_search_finds_a_matching_zip_file_as_its_folder_row() {
    let dir = TempDir::new();
    dir.file("gear kit.zip", &kit_zip());
    dir.file("bracket kit.zip", &kit_zip());
    dir.file("a gear.stl", b"g");
    let mut folder = DiskFolder::new(dir.path(), Rc::new(AllFiles));
    folder.load();
    let listed = folder
        .child_containers()
        .into_iter()
        .find(|link| link.name().starts_with("gear kit"))
        .expect("the zip lists as a folder");
    let found = Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = Rc::clone(&found);
    block_on(folder.search_provider().unwrap().search_async(
        "gear",
        Rc::new(move |batch: Vec<Rc<dyn Item>>| sink.borrow_mut().extend(batch)),
        SearchCancel::new(),
    ))
    .unwrap();
    let found = found.borrow();
    let names: Vec<&str> = found.iter().map(|i| i.name()).collect();
    assert_eq!(names, [listed.name(), "a gear.stl"]);
    let row = found[0].as_container_link().expect("a folder row");
    assert!(row.as_any().is_some_and(|a| a.is::<ZipFolderLink>()));
    assert!(Rc::clone(&found[0]).into_container_link().is_some());
}
