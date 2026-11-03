//! The Move dialog's verdict over folders that have no location key (an app's remote folders,
//! told apart by server IDs): with the default rules it can't tell them apart and moves; with
//! `PlaceRules::folder_key` and `open_folder_key` naming them, "already in" and "can't go inside
//! itself" hold for them as they do for folders on disk.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_file_browser::{
    dialog_options, move_verdict, BrowserConfig, BrowserEntry, BrowserState, DefaultPlaces,
    FolderKey, MoveRequest, MoveVerdict, PlaceRules,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, FolderLink, Item, MemoryFolder};

/// Folders told apart by name, as an app tells its remote folders by their IDs.
struct ByName;

impl PlaceRules for ByName {
    fn folder_key(&self, link: &dyn FolderLink) -> Option<FolderKey> {
        Some(FolderKey::new(None, link.name()))
    }
    fn open_folder_key(&self, folder: &dyn Folder) -> Option<FolderKey> {
        Some(FolderKey::new(None, folder.name()))
    }
}

/// A link to a keyless folder `name` holding folders `children`, its ID unique to this link (as
/// each listing of a remote folder makes its rows anew).
fn keyless(name: &'static str, children: &'static [&'static str]) -> Rc<dyn FolderLink> {
    thread_local!(static NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
    let id = NEXT.with(|n| {
        n.set(n.get() + 1);
        n.get()
    });
    Rc::new(
        DynamicFolderLink::new(
            name,
            Rc::new(move || {
                let mut folder = DynamicFolder::new(name);
                folder.data.child_containers = children.iter().map(|c| keyless(c, &[])).collect();
                Box::new(folder) as Box<dyn Folder>
            }),
        )
        .with_id(format!("{name} {id}")),
    )
}

/// The top: Designs (holding Parts) and Drafts, none with a location key.
fn dialog() -> Rc<RefCell<BrowserState>> {
    agg_gui::set_device_scale(1.0);
    let mut root = DynamicFolder::new("Top");
    root.data.child_containers = vec![keyless("Designs", &["Parts"]), keyless("Drafts", &[])];
    let config = BrowserConfig::builder().options(dialog_options()).build();
    BrowserState::new(Box::new(root), config)
}

/// Selects the folder row `name` in the dialog's list.
fn select(state: &Rc<RefCell<BrowserState>>, name: &str) {
    let index = state
        .borrow()
        .entries()
        .iter()
        .position(|e| matches!(e, BrowserEntry::Container(l) if l.name() == name))
        .unwrap_or_else(|| panic!("{name} is listed"));
    BrowserState::click(state, index, Modifiers::default());
}

/// Opens the folder row `name` in the dialog.
fn open(state: &Rc<RefCell<BrowserState>>, name: &str) {
    let link = state
        .borrow()
        .entries()
        .into_iter()
        .find_map(|e| match e {
            BrowserEntry::Container(l) if l.name() == name => Some(l),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{name} is listed"));
    state.borrow_mut().open(link.as_ref());
}

/// Moving `items` out of the folder `source` under `places`.
fn request(items: Vec<Rc<dyn Item>>, source: &str, places: Rc<dyn PlaceRules>) -> MoveRequest {
    let key = places.open_folder_key(&DynamicFolder::new(source));
    MoveRequest::new(items, None)
        .with_source(source, key, Rc::new(|_| false))
        .with_places(places)
}

fn gear() -> Rc<dyn Item> {
    let folder = MemoryFolder::new("Loose");
    folder.add_file("gear.stl", b"g".to_vec());
    folder.items().into_iter().next().expect("gear.stl")
}

#[test]
fn keyed_folders_say_the_items_are_already_there() {
    let state = dialog();
    select(&state, "Designs");
    let unkeyed = request(vec![gear()], "Designs", Rc::new(DefaultPlaces));
    assert!(
        matches!(
            move_verdict(&unkeyed, &state.borrow()),
            MoveVerdict::Move(_)
        ),
        "without keys the folders can't be told apart"
    );
    let keyed = request(vec![gear()], "Designs", Rc::new(ByName));
    assert_eq!(
        move_verdict(&keyed, &state.borrow()),
        MoveVerdict::Refused("gear.stl is already in Designs.".into())
    );
    select(&state, "Drafts");
    assert_eq!(
        move_verdict(&keyed, &state.borrow()),
        MoveVerdict::Move("Move gear.stl to Drafts.".into())
    );
}

#[test]
fn a_keyed_folder_cant_go_into_a_folder_inside_it() {
    let state = dialog();
    open(&state, "Designs");
    select(&state, "Parts");
    let designs = || vec![keyless("Designs", &["Parts"]) as Rc<dyn Item>];
    let unkeyed = request(designs(), "Top", Rc::new(DefaultPlaces));
    let verdict = move_verdict(&unkeyed, &state.borrow());
    assert!(matches!(verdict, MoveVerdict::Move(_)), "{verdict:?}");
    let keyed = request(designs(), "Top", Rc::new(ByName));
    assert_eq!(
        move_verdict(&keyed, &state.borrow()),
        MoveVerdict::Refused("Designs can't go inside itself. Choose another folder.".into())
    );
}
