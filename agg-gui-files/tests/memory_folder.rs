//! `MemoryFolder`, the in-memory writable folder agg-gui's own tests and demo browse: it lists
//! files and sub-folders, takes adds (files copied, folders copied whole, a create-folder item
//! making a folder), removes and renames, raises content-changed after each change like a real
//! folder, shares its contents between every handle on it, and answers a place and a location
//! key.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui_files::{
    names_of_items_gone_from_source, CreateFolderItem, Folder, Item, MemoryFile, MemoryFolder,
    PlaceId, WritableFolder,
};

fn counting(folder: &dyn Folder) -> Rc<Cell<i32>> {
    let runs = Rc::new(Cell::new(0));
    let counter = Rc::clone(&runs);
    folder.subscribe_content_changed(Rc::new(move || counter.set(counter.get() + 1)));
    runs
}

fn names(items: &[Rc<dyn Item>]) -> Vec<String> {
    items.iter().map(|i| i.name().to_string()).collect()
}

fn folder_names(folder: &dyn Folder) -> Vec<String> {
    folder
        .child_containers()
        .iter()
        .map(|l| l.name().to_string())
        .collect()
}

fn bytes_of(item: &Rc<dyn Item>) -> Vec<u8> {
    item.as_asset_stream()
        .expect("a file")
        .get_stream()
        .expect("readable")
        .expect("content")
}

#[test]
fn files_and_folders_are_listed_in_the_order_they_were_added() {
    let home = MemoryFolder::new("Samples");
    home.add_file("gear.stl", b"solid gear".to_vec());
    home.add_file("notes.txt", Vec::new());
    home.add_folder("Brackets").expect("a folder");
    assert_eq!(names(&home.items()), ["gear.stl", "notes.txt"]);
    assert_eq!(folder_names(&home), ["Brackets"]);
    let gear = &home.items()[0];
    assert_eq!(bytes_of(gear), b"solid gear");
    let asset = gear.as_asset().expect("content");
    assert_eq!(asset.content_type(), "stl");
    assert_eq!(asset.file_name(), "gear.stl");
    assert!(
        home.is_protected(),
        "a top folder cannot be renamed or removed"
    );
    assert!(!home.child_containers()[0].is_protected());
}

#[test]
fn every_change_raises_content_changed_on_every_view_of_the_folder() {
    let home = MemoryFolder::new("Samples");
    let mut view = home.link().get_container().expect("opens");
    view.load();
    let raised = counting(view.as_ref());

    home.add_file("gear.stl", Vec::new());
    assert_eq!(
        raised.get(),
        1,
        "an add through another handle shows in this view"
    );
    assert_eq!(names(&view.items()), ["gear.stl"]);

    let gear = view.items()[0].clone();
    let writable = view.as_writable_mut().expect("writable");
    writable.rename(&gear, "cog.stl").expect("renamed");
    assert_eq!(raised.get(), 2);
    let cog = view.items()[0].clone();
    let writable = view.as_writable_mut().expect("writable");
    writable.remove(&[cog]).expect("removed");
    assert_eq!(raised.get(), 3);
    assert!(home.items().is_empty());
}

#[test]
fn loading_raises_nothing() {
    let mut home = MemoryFolder::new("Samples");
    home.add_file("gear.stl", Vec::new());
    let raised = counting(&home);
    home.load();
    assert_eq!(
        raised.get(),
        0,
        "a listing that is already there is not a change"
    );
}

#[test]
fn a_rename_keeps_the_id_and_refuses_a_name_already_here() {
    let mut home = MemoryFolder::new("Samples");
    let gear = home.add_file("gear.stl", b"g".to_vec());
    home.add_file("cog.stl", Vec::new());
    let id = gear.id().to_string();

    let taken = home.rename(&gear, "cog.stl");
    assert_eq!(taken, Err("cog.stl is already in this folder".to_string()));
    home.rename(&gear, "wheel.stl").expect("a free name");
    let renamed = &home.items()[0];
    assert_eq!(renamed.name(), "wheel.stl");
    assert_eq!(renamed.id(), id, "the same file under its new name");
    assert_eq!(bytes_of(renamed), b"g");

    let sub = home.add_folder("Old").expect("a folder");
    let sub_link: Rc<dyn Item> = home.child_containers()[0].clone();
    home.rename(&sub_link, "New").expect("a folder renames");
    assert_eq!(folder_names(&home), ["New"]);
    assert_eq!(home.child_containers()[0].id(), sub.id());
}

#[test]
fn removing_an_item_the_folder_does_not_list_says_so() {
    let mut home = MemoryFolder::new("Samples");
    let other = MemoryFolder::new("Other");
    let stranger = other.add_file("gear.stl", Vec::new());
    assert_eq!(
        home.remove(&[stranger]),
        Err("gear.stl is not in this folder".to_string())
    );
}

#[test]
fn a_name_already_here_gets_a_number_before_its_extension() {
    let mut home = MemoryFolder::new("Samples");
    home.add_file("gear.stl", Vec::new());
    home.add_file("gear.stl", Vec::new());
    let other = MemoryFolder::new("Other");
    let copy = other.add_file("gear.stl", Vec::new());
    home.add(&[copy]).expect("copied");
    assert_eq!(
        names(&home.items()),
        ["gear.stl", "gear (2).stl", "gear (3).stl"]
    );
}

#[test]
fn a_create_folder_item_makes_a_folder_and_keeps_one_already_there() {
    let mut home = MemoryFolder::new("Samples");
    let make: Rc<dyn Item> = Rc::new(CreateFolderItem::new("Brackets"));
    home.add(std::slice::from_ref(&make)).expect("made");
    let first_id = home.child_containers()[0].id().to_string();
    home.add(&[make]).expect("kept");
    assert_eq!(folder_names(&home), ["Brackets"]);
    assert_eq!(home.child_containers()[0].id(), first_id);
    // The recorded default goes through the same add.
    assert!(home.create_folder_recorded("Hinges").result.is_ok());
    assert_eq!(folder_names(&home), ["Brackets", "Hinges"]);
}

#[test]
fn adding_a_folder_copies_it_whole_and_never_into_itself() {
    let home = MemoryFolder::new("Samples");
    let brackets = home.add_folder("Brackets").expect("a folder");
    brackets.add_file("l bracket.stl", b"l".to_vec());
    brackets
        .add_folder("Small")
        .expect("a folder")
        .add_file("tiny.stl", b"t".to_vec());

    let mut other = MemoryFolder::new("Other");
    other
        .add(&[brackets.link() as Rc<dyn Item>])
        .expect("copied");
    let copy = other.child_containers()[0].get_container().expect("opens");
    assert_eq!(copy.name(), "Brackets");
    assert_eq!(names(&copy.items()), ["l bracket.stl"]);
    assert_ne!(copy.id(), brackets.id(), "a copy is a folder of its own");
    let small = copy.child_containers()[0].get_container().expect("opens");
    assert_eq!(bytes_of(&small.items()[0]), b"t");

    let mut inner = brackets.add_folder("Small").expect("a folder");
    let refused = inner.add(&[brackets.link() as Rc<dyn Item>]);
    assert_eq!(
        refused,
        Err("A folder cannot be copied into itself".to_string())
    );
}

#[test]
fn a_move_adds_then_removes_and_names_what_moved() {
    let mut from = MemoryFolder::new("From");
    let gear = from.add_file("gear.stl", b"g".to_vec());
    from.add_folder("Brackets").expect("a folder");
    let brackets: Rc<dyn Item> = from.child_containers()[0].clone();
    let mut to = MemoryFolder::new("To");

    let moving = vec![Rc::clone(&gear), brackets];
    to.move_items(&moving, &mut from).expect("moved");
    assert!(from.items().is_empty() && from.child_containers().is_empty());
    assert_eq!(names(&to.items()), ["gear.stl"]);
    assert_eq!(folder_names(&to), ["Brackets"]);
    assert_eq!(
        names_of_items_gone_from_source(&moving, &from),
        ["gear.stl", "Brackets"]
    );
}

#[test]
fn a_folder_its_link_and_its_sub_folders_share_a_place_and_a_location_key() {
    let home = MemoryFolder::new("Samples").with_place(PlaceId::new("memory"));
    let sub = home.add_folder("Brackets").expect("a folder");
    assert_eq!(home.place(), Some(PlaceId::new("memory")));
    assert_eq!(
        sub.place(),
        Some(PlaceId::new("memory")),
        "sub-folders share the place"
    );
    assert_eq!(home.link().place(), home.place());

    let link = home.child_containers()[0].clone();
    assert_eq!(
        link.location_key(),
        sub.location_key(),
        "row and folder agree"
    );
    assert_ne!(home.location_key(), sub.location_key());
    assert_eq!(MemoryFolder::new("Elsewhere").place(), None);
}

#[test]
fn a_sub_folder_keeps_the_place_after_the_top_handle_is_gone() {
    let parts = MemoryFolder::new("Sample")
        .with_place(PlaceId::new("memory"))
        .add_folder("Parts")
        .expect("a folder");
    assert_eq!(parts.place(), Some(PlaceId::new("memory")));
    let deeper = parts.add_folder("Deeper").expect("a folder");
    assert_eq!(deeper.place(), Some(PlaceId::new("memory")));
    // Set from anywhere in the tree, every folder in it answers it.
    let deeper = deeper.with_place(PlaceId::COMPUTER);
    assert_eq!(parts.place(), Some(PlaceId::COMPUTER));
    assert_eq!(deeper.link().place(), Some(PlaceId::COMPUTER));
}

#[test]
fn a_removed_sub_folder_is_in_no_place_and_may_take_its_old_parent() {
    let mut home = MemoryFolder::new("Samples").with_place(PlaceId::new("memory"));
    let parts = home.add_folder("Parts").expect("a folder");
    let inner = parts.add_folder("Inner").expect("a folder");
    let parts_row: Rc<dyn Item> = home.child_containers()[0].clone();
    home.remove(&[parts_row]).expect("removed");
    assert_eq!(
        parts.place(),
        None,
        "a folder taken out of its tree is in no place"
    );
    assert_eq!(inner.place(), None, "and so is everything in it");
    assert_eq!(home.place(), Some(PlaceId::new("memory")));

    // No longer inside Samples, so Samples may be copied into it.
    let mut parts = parts;
    parts.add(&[home.link() as Rc<dyn Item>]).expect("copied");
    assert_eq!(folder_names(&parts), ["Inner", "Samples"]);
}

#[test]
fn copying_a_folder_into_its_own_sub_folder_is_refused_with_only_the_sub_folder_handle_kept() {
    let (mut inner, top_row) = {
        let top = MemoryFolder::new("Top");
        let inner = top
            .add_folder("Middle")
            .expect("a folder")
            .add_folder("Inner")
            .expect("a folder");
        (inner, top.link())
    };
    // Only the row holds Top now; the walk up from Inner still finds it.
    let refused = inner.add(&[top_row as Rc<dyn Item>]);
    assert_eq!(
        refused,
        Err("A folder cannot be copied into itself".to_string())
    );
}

#[test]
fn a_folder_is_not_made_where_a_file_of_its_name_is() {
    let mut home = MemoryFolder::new("Samples");
    home.add_file("Parts", Vec::new());
    assert_eq!(
        home.add_folder("Parts").err(),
        Some("Parts is already in this folder".to_string())
    );
    let make: Rc<dyn Item> = Rc::new(CreateFolderItem::new("Parts"));
    assert_eq!(
        home.add(&[make]),
        Err("Parts is already in this folder".to_string())
    );
    assert!(home.child_containers().is_empty());
    assert_eq!(names(&home.items()), ["Parts"]);
}

#[test]
fn a_rename_to_an_empty_name_is_refused() {
    let mut home = MemoryFolder::new("Samples");
    let gear = home.add_file("gear.stl", Vec::new());
    for empty in ["", "   ", "\t"] {
        assert_eq!(
            home.rename(&gear, empty),
            Err("A name cannot be empty".to_string())
        );
    }
    assert_eq!(names(&home.items()), ["gear.stl"]);
}

#[test]
fn a_link_answers_at_once_whether_the_folder_has_folders() {
    let home = MemoryFolder::new("Samples");
    let sub = home.add_folder("Brackets").expect("a folder");
    assert_eq!(home.link().has_child_containers(), Some(true));
    assert_eq!(sub.link().has_child_containers(), Some(false));
}

#[test]
fn a_file_reaches_its_own_type_through_as_any() {
    let home = MemoryFolder::new("Samples");
    let gear = home.add_file("gear.stl", b"g".to_vec());
    let file = gear
        .as_any()
        .and_then(|a| a.downcast_ref::<MemoryFile>())
        .expect("a memory file");
    assert_eq!(file.bytes(), b"g");
}
