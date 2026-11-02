//! `BrowserState` with the options a picking dialog sets: an item filter listing only some files
//! (its own folder's, a search's results with their counts, and never hiding a folder's
//! message), one row selectable at a time, and a folder listed again on the user's ask.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{Modifiers, Point};
use agg_gui_file_browser::{
    extension_filter, run_ready_local_tasks, BrowserConfig, BrowserOptions, BrowserState, ListSlot,
    LocalSpawner,
};
use agg_gui_files::{
    DynamicFolder, Folder, FolderLink, Item, MemoryFolder, MessageItem, SearchResultsFolder,
};

/// Home holding the folder Parts, and the files a.stl, b.txt and c.STL.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    home.add_folder("Parts").expect("Parts is free");
    home.add_file("b.txt", b"b".to_vec());
    home.add_file("a.stl", b"a".to_vec());
    home.add_file("c.STL", b"c".to_vec());
    home
}

fn state_with(root: Box<dyn Folder>, options: BrowserOptions) -> Rc<RefCell<BrowserState>> {
    BrowserState::new(root, BrowserConfig::builder().options(options).build())
}

fn stl_only() -> BrowserOptions {
    BrowserOptions {
        item_filter: Some(extension_filter(&[".stl"])),
        ..Default::default()
    }
}

fn names(state: &BrowserState) -> Vec<String> {
    let entries = state.entries();
    entries.iter().map(|e| e.name().to_string()).collect()
}

fn selected(state: &BrowserState) -> Vec<String> {
    let entries = state.selected_entries();
    entries.iter().map(|e| e.name().to_string()).collect()
}

fn keys(shift: bool, ctrl: bool) -> Modifiers {
    Modifiers {
        shift,
        ctrl,
        ..Modifiers::default()
    }
}

#[test]
fn an_item_filter_lists_only_the_files_it_keeps_and_every_folder() {
    let state = state_with(Box::new(home()), stl_only());
    assert_eq!(names(&state.borrow()), ["Parts", "a.stl", "c.STL"]);
}

#[test]
fn an_item_filter_never_hides_a_folders_message() {
    let mut loading = DynamicFolder::new("Cloud");
    loading.data.items = vec![Rc::new(MessageItem::loading()) as Rc<dyn Item>];
    let state = state_with(Box::new(loading), stl_only());
    let message = state.borrow().listing_message().expect("the message shows");
    assert!(message.loading, "{message:?}");
}

#[test]
fn an_item_filter_keeps_only_its_files_among_search_results_and_counts_them() {
    let root = MemoryFolder::new("Home");
    for name in ["gear a.stl", "gear b.txt", "gear c.STL", "bracket.stl"] {
        root.add_file(name, b"g".to_vec());
    }
    let state = state_with(Box::new(root.clone()), stl_only());
    let spawner = Rc::new(LocalSpawner::new(Rc::new(|m: &str| panic!("fault: {m}"))));
    let mut results = SearchResultsFolder::new(&root, "gear", spawner);
    results.start_search();
    for _ in 0..1000 {
        if results.is_finished() {
            break;
        }
        run_ready_local_tasks();
    }
    assert!(results.is_finished(), "the search ends");
    state.borrow_mut().push_search_results(Rc::new(results));
    let s = state.borrow();
    let mut found = names(&s);
    found.sort();
    assert_eq!(found, ["gear a.stl", "gear c.STL"]);
    let counts: Vec<Option<String>> = s
        .list_slots()
        .into_iter()
        .filter_map(|slot| match slot {
            ListSlot::Section(header) => Some(header.status),
            _ => None,
        })
        .collect();
    assert_eq!(counts, [Some("(2)".to_string())]);
}

#[test]
fn without_multi_select_a_press_with_any_keys_selects_only_its_row() {
    let options = BrowserOptions {
        multi_select: false,
        ..Default::default()
    };
    let state = state_with(Box::new(home()), options);
    state.borrow_mut().mark_listed();
    // Parts, a.stl, b.txt, c.STL.
    BrowserState::click(&state, 1, Modifiers::default());
    BrowserState::click(&state, 3, keys(false, true));
    assert_eq!(selected(&state.borrow()), ["c.STL"], "Control doesn't add");
    BrowserState::click(&state, 1, keys(true, false));
    assert_eq!(selected(&state.borrow()), ["a.stl"], "Shift doesn't extend");
    let mut s = state.borrow_mut();
    s.select_all();
    assert_eq!(selected(&s), ["a.stl"], "select all does nothing");
    s.move_selection_to(2, true);
    assert_eq!(selected(&s), ["b.txt"], "Shift with an arrow moves alone");
    s.ensure_selected(3, true);
    assert_eq!(selected(&s), ["c.STL"], "a Control right press doesn't add");
    s.begin_band(Point::new(0.0, 0.0), false);
    assert!(s.drag_band(Point::new(200.0, 200.0)).is_none(), "no band");
    s.end_band(true);
    assert!(
        selected(&s).is_empty(),
        "a press on empty space still clears"
    );
}

#[test]
fn with_multi_select_the_same_presses_select_several() {
    let state = state_with(Box::new(home()), BrowserOptions::default());
    state.borrow_mut().mark_listed();
    BrowserState::click(&state, 1, Modifiers::default());
    BrowserState::click(&state, 3, keys(false, true));
    assert_eq!(selected(&state.borrow()), ["a.stl", "c.STL"]);
}

/// A folder listing a snapshot of `source`, taken at each load, that says nothing when `source`
/// changes: a folder changed behind the browser's back.
struct Snapshot {
    source: MemoryFolder,
    items: Vec<Rc<dyn Item>>,
}

impl Folder for Snapshot {
    fn id(&self) -> &str {
        "snapshot"
    }
    fn name(&self) -> &str {
        "Snapshot"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.items.clone()
    }
    fn load(&mut self) {
        self.items = self.source.items();
    }
}

#[test]
fn relist_loads_the_showing_folder_again_and_shows_its_new_rows() {
    let source = MemoryFolder::new("Source");
    source.add_file("old.txt", b"o".to_vec());
    let folder = Snapshot {
        source: source.clone(),
        items: Vec::new(),
    };
    let state = state_with(Box::new(folder), BrowserOptions::default());
    source.add_file("new.txt", b"n".to_vec());
    state.borrow_mut().sync_content();
    assert_eq!(names(&state.borrow()), ["old.txt"], "nothing said");
    let generation = state.borrow().generation();
    state.borrow_mut().relist();
    assert_eq!(names(&state.borrow()), ["new.txt", "old.txt"]);
    assert!(
        state.borrow().generation() > generation,
        "the list shows anew"
    );
}
