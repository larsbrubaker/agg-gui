//! The plain providers: a message row (`MessageItem`), a folder filled when it loads
//! (`DynamicFolder` over `FolderData`) and a row whose folder is made when opened
//! (`DynamicFolderLink`): its unavailable action, which only a user's open runs, never a
//! search's walk; its defaults; its place, asked of the folder it makes on each call; its cheap
//! child-folder answer.

use std::cell::Cell;
use std::rc::Rc;

use agg_gui_files::{
    DynamicFolder, DynamicFolderLink, Folder, FolderData, FolderLink, Item, MemoryFolder,
    MessageItem, PlaceId, NO_FOLDER,
};

#[test]
fn a_message_row_has_no_id_and_says_whether_it_is_a_missing_file() {
    let loading = MessageItem::loading();
    assert_eq!(loading.name(), "Loading...");
    assert_eq!(
        loading.id(),
        "",
        "a message row is no item a browser can select by ID"
    );
    assert!(!loading.is_missing_file());
    assert_eq!(loading.thumbnail_icon(), None);

    let missing = MessageItem::missing_file("parts.zip");
    assert_eq!(missing.name(), "Missing-parts.zip");
    assert!(missing.is_missing_file());
    // The missing-file row asks for the transparent part icon, as a key the app resolves.
    assert_eq!(
        missing.thumbnail_icon(),
        Some("part_icon_transparent_40x40.png")
    );
    // Both are the one type, so a check for a message row sees either.
    let as_item: Rc<dyn Item> = Rc::new(missing);
    assert!(as_item.as_any().is_some_and(|a| a.is::<MessageItem>()));
}

#[test]
fn a_walk_never_runs_the_unavailable_action() {
    let prompted = Rc::new(Cell::new(0));
    let seen = Rc::clone(&prompted);
    let link = DynamicFolderLink::new_optional("Cloud Library", Rc::new(|| None))
        .with_when_unavailable(move || seen.set(seen.get() + 1));

    // A search's walk.
    assert!(link.get_container().is_err());
    assert_eq!(prompted.get(), 0);

    // The user's open.
    assert!(link.open_unavailable());
    assert_eq!(prompted.get(), 1);
}

#[test]
fn a_link_without_an_action_reports_the_failure() {
    let link = DynamicFolderLink::new_optional("Nothing", Rc::new(|| None));
    assert_eq!(
        link.get_container().err().as_deref(),
        Some("Could not be opened")
    );
    assert_eq!(NO_FOLDER, "Could not be opened");
    assert!(!link.open_unavailable());
}

#[test]
fn a_link_is_protected_writable_and_visible_unless_told_otherwise() {
    let link = DynamicFolderLink::new("Parts", Rc::new(|| Box::new(MemoryFolder::new("Parts"))));
    assert!(link.is_protected());
    assert!(
        !link.is_read_only(),
        "a link is writable unless told otherwise"
    );
    assert!(link.is_visible());
    assert_eq!(link.id(), "");
    assert!(!link.holds_destinations());
    assert_eq!(link.micro_icon(), None);
    assert_eq!(
        link.has_child_containers(),
        None,
        "unknown unless the maker says how"
    );

    let shown = Rc::new(Cell::new(false));
    let showing = Rc::clone(&shown);
    let told = DynamicFolderLink::new("Parts", Rc::new(|| Box::new(MemoryFolder::new("Parts"))))
        .with_id("parts")
        .with_read_only(true)
        .with_micro_icon("computer.svg")
        .with_holds_destinations(true)
        .with_visibility(move || showing.get());
    assert_eq!(told.id(), "parts");
    assert!(told.is_read_only());
    assert_eq!(told.micro_icon(), Some("computer.svg"));
    assert!(told.holds_destinations());
    assert!(!told.is_visible());
    shown.set(true);
    assert!(told.is_visible(), "visibility is asked each time");
}

#[test]
fn a_root_row_answers_its_folders_place_making_the_folder_each_time_and_no_key() {
    let made = Rc::new(Cell::new(0));
    let counter = Rc::clone(&made);
    let link = DynamicFolderLink::new(
        "Disk",
        Rc::new(move || {
            counter.set(counter.get() + 1);
            Box::new(MemoryFolder::new("Disk").with_place(PlaceId::COMPUTER))
        }),
    );
    assert_eq!(link.place(), Some(PlaceId::COMPUTER));
    assert_eq!(link.place(), Some(PlaceId::COMPUTER));
    assert_eq!(made.get(), 2, "each answer makes the folder once");
    // The tree knows a top-level row by its name.
    assert_eq!(link.location_key(), None);

    let nowhere = DynamicFolderLink::new_optional("Nothing", Rc::new(|| None));
    assert_eq!(nowhere.place(), None);
}

#[test]
fn a_local_listing_answers_whether_the_folder_has_folders() {
    let empty = DynamicFolderLink::new("Empty", Rc::new(|| Box::new(MemoryFolder::new("Empty"))))
        .with_local_listing();
    assert_eq!(empty.has_child_containers(), Some(false));

    let parent = MemoryFolder::new("Parent");
    parent.add_folder("Child").expect("a folder");
    let with_child = DynamicFolderLink::new("Parent", Rc::new(move || Box::new(parent.clone())))
        .with_local_listing();
    assert_eq!(with_child.has_child_containers(), Some(true));

    let told = DynamicFolderLink::new("Told", Rc::new(|| Box::new(MemoryFolder::new("Told"))))
        .with_child_containers(|| Some(true));
    assert_eq!(told.has_child_containers(), Some(true));
}

#[test]
fn a_dynamic_folder_is_filled_by_its_initializer_on_each_load() {
    let loads = Rc::new(Cell::new(0));
    let counter = Rc::clone(&loads);
    let mut folder = DynamicFolder::new("Samples").with_header_markdown("# Samples");
    folder.initializer = Some(Rc::new(move |data: &mut FolderData| {
        counter.set(counter.get() + 1);
        data.items = vec![Rc::new(MessageItem::new("one")) as Rc<dyn Item>];
        data.child_containers = vec![MemoryFolder::new("Inner").link()];
    }));
    assert!(folder.items().is_empty(), "nothing is listed before load");
    folder.load();
    assert_eq!(loads.get(), 1);
    assert_eq!(folder.items().len(), 1);
    assert_eq!(folder.child_containers()[0].name(), "Inner");
    assert_eq!(folder.header_markdown(), "# Samples");
    assert_eq!(folder.name(), "Samples");
    assert!(
        folder.is_protected(),
        "a plain folder is protected unless it says otherwise"
    );
    assert_eq!(folder.id(), "");
}

#[test]
fn folder_data_starts_empty_and_protected() {
    let data = FolderData::new("Home");
    assert_eq!(data.name, "Home");
    assert_eq!(data.id, "");
    assert!(data.is_protected);
    assert!(data.items.is_empty() && data.child_containers.is_empty());
}
