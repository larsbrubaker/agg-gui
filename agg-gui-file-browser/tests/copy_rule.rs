//! Move or copy: items dragged within one place move; they are copied, keeping the originals,
//! between two places, out of a folder that keeps its items or from no known folder, and when the
//! copy modifier is held, which never turns a copy into a move. A folder in no place never makes
//! a drop cross. The rule is read through `DefaultPlaces`, as a browser's config holds it.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_file_browser::{
    copy_modifier_held, default_crosses_places, BrowserHistory, CopyCx, CopyTakeover,
    DefaultPlaces, PlaceRules,
};
use agg_gui_files::{
    ContainerActions, Folder, FolderLink, Item, MemoryFolder, PlaceId, WritableFolder,
};

fn cloud() -> PlaceId {
    PlaceId::new("cloud")
}

fn on_computer(name: &str) -> MemoryFolder {
    MemoryFolder::new(name).with_place(PlaceId::COMPUTER)
}

fn copies(source: Option<&dyn Folder>, target: Option<PlaceId>, modifier: bool) -> bool {
    let rules: Rc<dyn PlaceRules> = Rc::new(DefaultPlaces);
    rules.drop_copies(source, target, modifier)
}

/// A folder in a place that lists nothing and takes no changes (a catalog the app ships).
struct KeepsItsItems(Option<PlaceId>);

impl Folder for KeepsItsItems {
    fn id(&self) -> &str {
        "keeps"
    }
    fn name(&self) -> &str {
        "Keeps"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        Vec::new()
    }
    fn load(&mut self) {}
    fn place(&self) -> Option<PlaceId> {
        self.0.clone()
    }
}

/// A writable folder that takes items but never gives them up.
struct AddOnly(MemoryFolder);

impl Folder for AddOnly {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn name(&self) -> &str {
        self.0.name()
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.0.child_containers()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.0.items()
    }
    fn load(&mut self) {}
    fn as_writable(&self) -> Option<&dyn WritableFolder> {
        Some(self)
    }
    fn place(&self) -> Option<PlaceId> {
        Some(PlaceId::COMPUTER)
    }
}

impl WritableFolder for AddOnly {
    fn add(&mut self, _items: &[Rc<dyn Item>]) -> Result<(), String> {
        Ok(())
    }
    fn remove(&mut self, _items: &[Rc<dyn Item>]) -> Result<(), String> {
        Err("never".into())
    }
    fn rename(&mut self, _item: &Rc<dyn Item>, _new_name: &str) -> Result<(), String> {
        Err("never".into())
    }
    fn allow_action(&self, action: ContainerActions) -> bool {
        action != ContainerActions::RemoveItems
    }
}

#[test]
fn a_drag_within_one_place_moves() {
    let home = on_computer("Home");
    assert!(!copies(Some(&home), Some(PlaceId::COMPUTER), false));
    let shelf = MemoryFolder::new("Shelf").with_place(cloud());
    assert!(!copies(Some(&shelf), Some(cloud()), false));
}

#[test]
fn a_drag_between_two_places_copies() {
    let home = on_computer("Home");
    assert!(copies(Some(&home), Some(cloud()), false));
    let shelf = MemoryFolder::new("Shelf").with_place(cloud());
    assert!(copies(Some(&shelf), Some(PlaceId::COMPUTER), false));
}

#[test]
fn the_copy_modifier_turns_a_move_into_a_copy_and_never_back() {
    let home = on_computer("Home");
    assert!(copies(Some(&home), Some(PlaceId::COMPUTER), true));
    assert!(copies(Some(&home), Some(cloud()), true));
}

#[test]
fn items_from_a_folder_that_keeps_them_are_copied() {
    let catalog = KeepsItsItems(Some(PlaceId::COMPUTER));
    assert!(copies(Some(&catalog), Some(PlaceId::COMPUTER), false));
    let add_only = AddOnly(MemoryFolder::new("Inbox"));
    assert!(copies(Some(&add_only), Some(PlaceId::COMPUTER), false));
}

#[test]
fn items_from_no_known_folder_are_copied() {
    assert!(copies(None, Some(PlaceId::COMPUTER), false));
    assert!(copies(None, None, false));
}

#[test]
fn a_folder_in_no_place_never_makes_a_drop_cross() {
    let home = on_computer("Home");
    // A row whose folder is not made yet answers no place: it counts as the source's place.
    assert!(!copies(Some(&home), None, false));
    let loose = MemoryFolder::new("Loose");
    assert!(!copies(Some(&loose), Some(cloud()), false));
}

#[test]
fn opened_folders_cross_only_between_two_known_places() {
    let rules = DefaultPlaces;
    let home = on_computer("Home");
    let other = on_computer("Other");
    let shelf = MemoryFolder::new("Shelf").with_place(cloud());
    let loose = MemoryFolder::new("Loose");
    assert!(!rules.crosses_places(&home, &other));
    assert!(rules.crosses_places(&home, &shelf));
    assert!(rules.crosses_places(&shelf, &home));
    assert!(!rules.crosses_places(&home, &loose));
    assert_eq!(
        rules.crosses_places(&home, &shelf),
        default_crosses_places(&home, &shelf)
    );
}

#[test]
fn the_default_rules_hand_every_copy_back_whole() {
    let home = on_computer("Home");
    let item = home.add_file("bracket.stl", b"solid".to_vec());
    let target = on_computer("Brackets");
    let cx = CopyCx {
        items: vec![Rc::clone(&item)],
        target: Box::new(target),
        target_name: "Brackets".into(),
        source: None,
        recorder: Rc::new(BrowserHistory::new()).recorder(),
        report: Rc::new(|_: &str| {}),
    };
    match DefaultPlaces.take_over_copy(cx) {
        CopyTakeover::Taken | CopyTakeover::Refused => panic!("the default runs no copy itself"),
        CopyTakeover::Declined(back) => {
            assert_eq!(back.items.len(), 1);
            assert!(Rc::ptr_eq(&back.items[0], &item));
            assert_eq!(back.target.name(), "Brackets");
            assert_eq!(back.target_name, "Brackets");
        }
    }
}

#[test]
fn an_app_can_take_copies_over() {
    struct Takes(RefCell<Vec<String>>);
    impl PlaceRules for Takes {
        fn take_over_copy(&self, cx: CopyCx) -> CopyTakeover {
            if cx.target.place() == Some(PlaceId::new("cloud")) {
                self.0.borrow_mut().push(cx.target_name);
                CopyTakeover::Taken
            } else {
                CopyTakeover::Declined(cx)
            }
        }
    }
    let rules = Takes(RefCell::new(Vec::new()));
    let to = |folder: MemoryFolder, name: &str| CopyCx {
        items: Vec::new(),
        target: Box::new(folder),
        target_name: name.into(),
        source: None,
        recorder: Rc::new(BrowserHistory::new()).recorder(),
        report: Rc::new(|_: &str| {}),
    };
    let shelf = MemoryFolder::new("Shelf").with_place(cloud());
    assert!(matches!(
        rules.take_over_copy(to(shelf, "Shelf")),
        CopyTakeover::Taken
    ));
    assert!(matches!(
        rules.take_over_copy(to(on_computer("Home"), "Home")),
        CopyTakeover::Declined(_)
    ));
    assert_eq!(*rules.0.borrow(), vec!["Shelf".to_string()]);
    // What it does not override keeps the default rule.
    assert!(rules.drop_copies(Some(&on_computer("A")), Some(cloud()), false));
}

#[test]
fn the_copy_modifier_is_option_on_the_mac_and_ctrl_elsewhere() {
    let alt = Modifiers {
        alt: true,
        ..Modifiers::default()
    };
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    assert_eq!(copy_modifier_held(alt), cfg!(target_os = "macos"));
    assert_eq!(copy_modifier_held(ctrl), !cfg!(target_os = "macos"));
    assert!(!copy_modifier_held(Modifiers::default()));
}
