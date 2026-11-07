//! A folder on disk searching itself: every folder under it is walked for the files its rules
//! list whose names (less the extension) hold every term of the query, and the folders whose
//! names hold them, in a stable order (a folder's matching sub-folders, its files, then each
//! sub-folder, depth first); hidden folders are neither found nor walked, a blank
//! query finds nothing, and the folder's own listing is left alone.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui_files::disk::{file_name_contains_filter, DiskFolder};
use agg_gui_files::{Folder, Item, SearchCancel};

use super::temp::{block_on, stl_only, TempDir};

fn search(folder: &DiskFolder, query: &str) -> Vec<String> {
    let found = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&found);
    let provider = folder
        .search_provider()
        .expect("a folder on disk searches itself");
    block_on(provider.search_async(
        query,
        Rc::new(move |batch: Vec<Rc<dyn Item>>| {
            sink.borrow_mut()
                .extend(batch.iter().map(|i| i.name().to_string()))
        }),
        SearchCancel::new(),
    ))
    .expect("the search finishes");
    let names = found.borrow().clone();
    names
}

#[test]
fn a_search_finds_listed_files_below_the_folder_in_walk_order() {
    let dir = TempDir::new();
    dir.file("gear big.stl", b"g");
    dir.file("gear notes.txt", b"t");
    dir.file("bracket.stl", b"b");
    let sub = dir.folder("Sub");
    std::fs::write(sub.join("small gear.stl"), b"g").unwrap();
    let hidden = dir.folder(".hidden");
    std::fs::write(hidden.join("gear hidden.stl"), b"g").unwrap();
    let mut folder = DiskFolder::new(dir.path(), stl_only());
    folder.load();
    assert_eq!(search(&folder, "gear"), ["gear big.stl", "small gear.stl"]);
    assert_eq!(search(&folder, "  "), Vec::<String>::new());
    assert_eq!(
        folder.items().len(),
        2,
        "the folder's own listing is untouched"
    );
}

/// A folder named for the query is a result, a row that opens it, reported before the files
/// beside it; a hidden folder is not, and the walk still goes into a matching folder.
#[test]
fn a_search_finds_folders_whose_names_match() {
    let dir = TempDir::new();
    dir.file("card.stl", b"c");
    let holder = dir.folder("Card Holder");
    std::fs::write(holder.join("holder card.stl"), b"c").unwrap();
    let other = dir.folder("Other");
    std::fs::create_dir(other.join("Card Box")).unwrap();
    dir.folder(".card hidden");
    let folder = DiskFolder::new(dir.path(), stl_only());
    assert_eq!(
        search(&folder, "card"),
        ["Card Holder", "card.stl", "holder card.stl", "Card Box"]
    );

    let found = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&found);
    block_on(folder.search_provider().unwrap().search_async(
        "holder",
        Rc::new(move |batch: Vec<Rc<dyn Item>>| sink.borrow_mut().extend(batch)),
        SearchCancel::new(),
    ))
    .unwrap();
    let found = found.borrow();
    let row = found
        .iter()
        .find(|i| i.name() == "Card Holder")
        .expect("the folder");
    let opened = row
        .as_container_link()
        .expect("a folder row")
        .get_container()
        .expect("it opens");
    assert_eq!(opened.name(), "Card Holder");
}

#[test]
fn the_extension_and_the_path_do_not_match() {
    assert!(!file_name_contains_filter("gear.stl", "stl"));
    assert!(!file_name_contains_filter("/parts/gear/plate.stl", "gear"));
    assert!(file_name_contains_filter("big gear box.stl", "box gear"));
}

#[test]
fn a_cancelled_search_reports_nothing() {
    let dir = TempDir::new();
    dir.file("gear.stl", b"g");
    let folder = DiskFolder::new(dir.path(), stl_only());
    let cancel = SearchCancel::new();
    cancel.cancel();
    let found = Rc::new(RefCell::new(0));
    let sink = Rc::clone(&found);
    block_on(folder.search_provider().unwrap().search_async(
        "gear",
        Rc::new(move |batch: Vec<Rc<dyn Item>>| *sink.borrow_mut() += batch.len()),
        cancel,
    ))
    .unwrap();
    assert_eq!(*found.borrow(), 0);
}
