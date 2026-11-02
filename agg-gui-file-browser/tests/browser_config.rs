//! A browser config with no app hooks is a working one, and each default is honest: entries draw
//! the look's icons, the menu is the built-ins in their usual groups, activation is left to the
//! browser, nothing is remembered, no dialog is offered (and a call that reaches one changes
//! nothing), and drags stay inside. An app's own hooks replace only what it sets, and the view
//! modes keep the numbers stored view states hold.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::Point;
use agg_gui_file_browser::{
    Activation, BrowserAction, BrowserActions, BrowserCommand, BrowserConfig, BrowserLayout,
    BrowserMemory, BrowserOptions, BrowserRef, BuiltinAction, Builtins, Confirmation, DialogKind,
    DoubleClickTarget, Entry, NameRequest, OutsideDrops, OutsideOver, OverlayFrame, PictureCx,
    PictureState, ViewMode, ViewState,
};
use agg_gui_files::{Item, MemoryFolder, PlaceId, SortBehavior, SortKey, Spawner};

fn shown(menu: &[BrowserAction]) -> Vec<String> {
    menu.iter()
        .map(|a| match a {
            BrowserAction::Builtin(b) => format!("{b:?}"),
            BrowserAction::Command(c) => c.title().to_string(),
            BrowserAction::Separator => "-".to_string(),
        })
        .collect()
}

fn sample() -> (MemoryFolder, Entry, Entry) {
    let home = MemoryFolder::new("Home").with_place(PlaceId::COMPUTER);
    let file = home.add_file("bracket.stl", b"solid".to_vec());
    let parts = home.add_folder("Parts").expect("a new folder");
    let path: Rc<[String]> = Rc::from(vec!["Home".to_string()]);
    let item_entry = Entry::new(file, Rc::clone(&path));
    let folder_entry = Entry::folder(parts.link(), path);
    (home, item_entry, folder_entry)
}

#[test]
fn entries_say_what_they_are_and_where_they_are_listed() {
    let (_home, item, folder) = sample();
    assert_eq!(item.name(), "bracket.stl");
    assert!(!item.is_folder() && item.link().is_none());
    assert_eq!(folder.name(), "Parts");
    assert!(folder.is_folder());
    let opened = folder
        .link()
        .expect("a folder row")
        .get_container()
        .expect("opens");
    assert_eq!(opened.name(), "Parts");
    assert_eq!(item.folder_path(), ["Home".to_string()]);
    let snapshot = BrowserRef::new(vec!["Home".to_string()], vec![item.clone()])
        .with_folder_place(Some(PlaceId::COMPUTER))
        .with_folder_writable(true);
    assert_eq!(snapshot.selection().len(), 1);
    assert_eq!(snapshot.selection()[0].name(), "bracket.stl");
    assert_eq!(snapshot.folder_place(), Some(&PlaceId::COMPUTER));
    assert!(snapshot.folder_writable());
}

#[test]
fn the_default_config_composes_every_default() {
    let config = BrowserConfig::default();
    let (_home, item, folder) = sample();
    let state = BrowserRef::new(vec!["Home".to_string()], vec![item.clone()]);

    // Pictures: the look's icons.
    let arrived: Rc<dyn Fn()> = Rc::new(|| {});
    let cx = PictureCx::new(&config.spawner, &arrived);
    assert!(matches!(
        config.pictures.picture(&cx, &item, 100),
        PictureState::Default
    ));
    assert!(matches!(
        config.pictures.picture(&cx, &folder, 100),
        PictureState::Default
    ));
    assert!(config
        .pictures
        .tile_overlay(&item, OverlayFrame { inset: 0.0 })
        .is_none());

    // Actions: every built-in in its groups; activation left to the browser.
    let menu = config.actions.menu(&state, &Builtins::all());
    assert_eq!(
        shown(&menu),
        [
            "Add",
            "CreateFolder",
            "-",
            "OpenFileLocation",
            "-",
            "Rename",
            "Move",
            "Remove"
        ]
    );
    assert!(!config
        .actions
        .activate(&state, &item, Activation::DoubleClick));
    assert!(!config.actions.activate(&state, &folder, Activation::Tap));

    // No outside drops; the default places rule.
    assert!(config.drops.is_none());
    let home = MemoryFolder::new("A").with_place(PlaceId::COMPUTER);
    assert!(!config
        .places
        .drop_copies(Some(&home), Some(PlaceId::COMPUTER), false));
    assert!(config
        .places
        .drop_copies(Some(&home), Some(PlaceId::new("cloud")), false));

    // Nothing remembered.
    let path = vec!["Home".to_string()];
    config.memory.set_view(&path, &ViewState::default());
    assert_eq!(config.memory.view(&path), None);
    config.memory.set_tree_width(240.0);
    assert_eq!(config.memory.tree_width(), None);
    assert!(config.memory.last_location("main").is_none());

    // The options a browser opens with.
    let options = &config.options;
    assert_eq!(options.layout, BrowserLayout::TreeAndList);
    assert_eq!(options.double_click, DoubleClickTarget::Actions);
    assert!(options.show_folders && options.folder_filter.is_none());
    assert!(!options.tap_activates && !options.finger_tap_opens_folders);
    assert_eq!(options.thumb_size, 100);
}

#[test]
fn the_default_dialogs_offer_nothing_and_change_nothing() {
    let config = BrowserConfig::default();
    for kind in [
        DialogKind::Confirm,
        DialogKind::AskName,
        DialogKind::PickFiles,
    ] {
        assert!(!config.dialogs.offers(kind), "{kind:?} is not offered");
    }
    let confirmed = Rc::new(Cell::new(None));
    let heard = Rc::clone(&confirmed);
    config.dialogs.confirm(
        Confirmation::new("Remove", "Remove 2 items?", "Remove"),
        Box::new(move |yes| heard.set(Some(yes))),
    );
    assert_eq!(
        confirmed.get(),
        Some(false),
        "no dialog means no confirmation"
    );
    let named = Rc::new(RefCell::new(Some(Some(String::new()))));
    let heard = Rc::clone(&named);
    config.dialogs.ask_name(
        NameRequest::new("Create Folder", "Folder Name", "New Folder", "Create"),
        Box::new(move |name| *heard.borrow_mut() = Some(name)),
    );
    assert_eq!(*named.borrow(), Some(None), "no dialog means no name");
    let picked = Rc::new(Cell::new(None));
    let heard = Rc::clone(&picked);
    config
        .dialogs
        .pick_files(Box::new(move |files: Vec<Rc<dyn Item>>| {
            heard.set(Some(files.len()))
        }));
    assert_eq!(picked.get(), Some(0));
}

#[test]
fn the_default_menu_keeps_only_whole_groups_apart() {
    let only = |offered: &[BuiltinAction]| shown(&Builtins::new(offered).default_menu());
    assert_eq!(only(&[]), Vec::<String>::new());
    // A read-only folder: no adding, so no leading separator.
    assert_eq!(
        only(&[BuiltinAction::OpenFileLocation]),
        ["OpenFileLocation"]
    );
    // Given out of order and twice, listed in order and once.
    assert_eq!(
        only(&[
            BuiltinAction::Remove,
            BuiltinAction::Add,
            BuiltinAction::Remove
        ]),
        ["Add", "-", "Remove"]
    );
    let builtins = Builtins::new(&[BuiltinAction::Rename]);
    assert!(builtins.offers(BuiltinAction::Rename) && !builtins.offers(BuiltinAction::Move));
}

struct Share(Rc<Cell<u32>>);

impl BrowserActions for Share {
    fn menu(&self, state: &BrowserRef, builtins: &Builtins) -> Vec<BrowserAction> {
        let runs = Rc::clone(&self.0);
        let mut menu = builtins.default_menu();
        menu.push(BrowserAction::Separator);
        menu.push(BrowserAction::Command(
            BrowserCommand::new("Share...", move || runs.set(runs.get() + 1))
                .enabled(!state.selection().is_empty()),
        ));
        menu
    }
}

struct Bed(RefCell<Vec<String>>);

impl OutsideDrops for Bed {
    fn over(&self, _items: &[Rc<dyn Item>], at: Point) -> OutsideOver {
        if at.x > 100.0 {
            OutsideOver::Target {
                hides_picture: true,
            }
        } else {
            OutsideOver::NoTarget {
                hides_picture: false,
            }
        }
    }
    fn release(&self, items: &[Rc<dyn Item>], at: Point) -> bool {
        let takes = matches!(self.over(items, at), OutsideOver::Target { .. });
        if takes {
            self.0
                .borrow_mut()
                .extend(items.iter().map(|i| i.name().to_string()));
        }
        takes
    }
    fn cancel(&self) {}
}

struct Remembers(RefCell<Vec<(Vec<String>, ViewState)>>);

impl BrowserMemory for Remembers {
    fn view(&self, path: &[String]) -> Option<ViewState> {
        self.0
            .borrow()
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, v)| *v)
    }
    fn set_view(&self, path: &[String], view: &ViewState) {
        self.0.borrow_mut().push((path.to_vec(), *view));
    }
}

#[test]
fn an_apps_hooks_replace_only_what_it_sets() {
    let runs = Rc::new(Cell::new(0));
    let bed = Rc::new(Bed(RefCell::new(Vec::new())));
    let config = BrowserConfig::builder()
        .actions(Rc::new(Share(Rc::clone(&runs))))
        .drops(Rc::clone(&bed) as Rc<dyn OutsideDrops>)
        .memory(Rc::new(Remembers(RefCell::new(Vec::new()))))
        .options(BrowserOptions {
            layout: BrowserLayout::ListOnly,
            folder_filter: Some(Rc::new(|link| link.name() != "Trash")),
            ..BrowserOptions::default()
        })
        .build();
    let (_home, item, folder) = sample();

    let state = BrowserRef::new(vec!["Home".to_string()], vec![item.clone()]);
    let menu = config
        .actions
        .menu(&state, &Builtins::new(&[BuiltinAction::Rename]));
    assert_eq!(shown(&menu), ["Rename", "-", "Share..."]);
    let BrowserAction::Command(share) = &menu[2] else {
        panic!("the app's command is last")
    };
    assert!(share.run());
    assert_eq!(runs.get(), 1);
    let nothing_selected = BrowserRef::new(vec!["Home".to_string()], Vec::new());
    let menu = config.actions.menu(&nothing_selected, &Builtins::new(&[]));
    let BrowserAction::Command(share) = &menu[1] else {
        panic!("the app's command")
    };
    assert!(
        !share.is_enabled() && !share.run(),
        "a disabled command does nothing"
    );
    assert_eq!(runs.get(), 1);

    let drops = config.drops.as_ref().expect("the app's drops");
    let items = vec![Rc::clone(item.item())];
    assert_eq!(
        drops.over(&items, Point::new(10.0, 0.0)),
        OutsideOver::NoTarget {
            hides_picture: false,
        }
    );
    assert!(drops.release(&items, Point::new(200.0, 0.0)));
    assert_eq!(*bed.0.borrow(), ["bracket.stl"]);

    let rows = ViewState {
        view_mode: ViewMode::Rows,
        sort: SortBehavior::new(SortKey::ModifiedDate, false),
    };
    let path = vec!["Home".to_string(), "Parts".to_string()];
    config.memory.set_view(&path, &rows);
    assert_eq!(config.memory.view(&path), Some(rows));

    let filter = config
        .options
        .folder_filter
        .as_ref()
        .expect("the app's filter");
    assert!(filter(folder.link().expect("a folder row")));
    assert_eq!(config.options.layout, BrowserLayout::ListOnly);
    // What the app left alone keeps its default.
    assert!(!config.dialogs.offers(DialogKind::Confirm));
    let arrived: Rc<dyn Fn()> = Rc::new(|| {});
    let spawner: &Rc<dyn Spawner> = &config.spawner;
    let cx = PictureCx::new(spawner, &arrived);
    assert!(matches!(
        config.pictures.picture(&cx, &item, 50),
        PictureState::Default
    ));
}

#[test]
fn view_modes_keep_their_stored_numbers_and_sizes() {
    let table = [
        (ViewMode::Rows, 0, 50),
        (ViewMode::Icons, 1, 100),
        (ViewMode::Icons18, 2, 18),
        (ViewMode::Icons70, 3, 70),
        (ViewMode::Icons256, 4, 256),
    ];
    for (mode, number, side) in table {
        assert_eq!(mode.number(), number);
        assert_eq!(ViewMode::from_number(number as i64), Some(mode));
        assert_eq!(mode.thumb_size(), side);
        assert_eq!(mode.is_icons(), mode != ViewMode::Rows);
    }
    assert_eq!(ViewMode::from_number(5), None);
    assert_eq!(ViewMode::for_icon_size(70), ViewMode::Icons70);
    assert_eq!(
        ViewMode::for_icon_size(75),
        ViewMode::Icons,
        "a dialog's 75 reads as icons"
    );
    assert_eq!(ViewMode::for_icon_size(50), ViewMode::Icons);
    let default = ViewState::default();
    assert_eq!(default.view_mode, ViewMode::Icons);
    assert_eq!(default.sort, SortBehavior::new(SortKey::Name, true));
}

#[test]
fn a_picture_context_reaches_the_open_folders_reading_and_says_when_one_arrived() {
    let config = BrowserConfig::default();
    let (_home, item, _folder) = sample();
    let told = Rc::new(Cell::new(0));
    let counter = Rc::clone(&told);
    let arrived: Rc<dyn Fn()> = Rc::new(move || counter.set(counter.get() + 1));
    let asked = RefCell::new(Vec::new());
    let read = |item: &Rc<dyn Item>, w: i32, h: i32| {
        asked.borrow_mut().push((item.name().to_string(), w, h));
        None
    };
    let cx = PictureCx::new(&config.spawner, &arrived);
    assert!(cx.folder_picture(item.item(), 100, 100).is_none());
    assert!(asked.borrow().is_empty(), "no reading of the folder's own");
    let cx = cx.with_folder_picture(&read);
    assert!(cx.folder_picture(item.item(), 100, 80).is_none());
    assert_eq!(*asked.borrow(), [("bracket.stl".to_string(), 100, 80)]);
    cx.picture_arrived();
    (cx.arrived_callback())();
    assert_eq!(told.get(), 2);
}
