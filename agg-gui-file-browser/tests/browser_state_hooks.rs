//! `BrowserState`'s hooks on a `MemoryFolder` tree: view and sort picks reaching the app's
//! `BrowserMemory` and coming back when a folder opens, the open path handed to
//! `remember_location` from the root with the state not borrowed, an item's activation reaching
//! `BrowserActions::activate` with a live handle the hook may borrow, and the app slot.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui_file_browser::{
    Activation, BrowserAction, BrowserActions, BrowserConfig, BrowserMemory, BrowserRef,
    BrowserState, Builtins, Entry, OpenFolder, ViewMode, ViewState,
};
use agg_gui_files::{MemoryFolder, SortBehavior, SortKey};

fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("Projects is free");
    projects.add_file("bracket.txt", b"b".to_vec());
    home.add_file("a.txt", b"a".to_vec());
    home
}

/// A memory keeping views by path and the locations it was handed, and a handle to the browser,
/// so it can check the browser is not borrowed while it runs.
#[derive(Default)]
struct Memory {
    views: RefCell<Vec<(Vec<String>, ViewState)>>,
    locations: RefCell<Vec<(String, Vec<String>, bool)>>,
    browser: RefCell<Option<std::rc::Weak<RefCell<BrowserState>>>>,
}

impl BrowserMemory for Memory {
    fn view(&self, path_names: &[String]) -> Option<ViewState> {
        let views = self.views.borrow();
        views
            .iter()
            .rev()
            .find(|(p, _)| p == path_names)
            .map(|(_, v)| *v)
    }

    fn set_view(&self, path_names: &[String], view: &ViewState) {
        self.views.borrow_mut().push((path_names.to_vec(), *view));
    }

    fn remember_location(&self, key: &str, path: &[OpenFolder]) {
        let names = path.iter().map(|f| f.borrow().name().to_string()).collect();
        let free = self
            .browser
            .borrow()
            .as_ref()
            .and_then(|b| b.upgrade())
            .is_some_and(|b| b.try_borrow_mut().is_ok());
        self.locations
            .borrow_mut()
            .push((key.to_string(), names, free));
    }
}

#[test]
fn a_view_and_sort_picked_are_remembered_for_the_folder_and_restored_on_opening_it() {
    let memory = Rc::new(Memory::default());
    let config = BrowserConfig::builder().memory(memory.clone()).build();
    let state = BrowserState::new(Box::new(home()), config.clone());
    state.borrow_mut().set_view_mode(ViewMode::Rows);
    state.borrow_mut().set_user_sort_key(SortKey::ModifiedDate);
    state.borrow_mut().set_user_sort_ascending(false);
    let last = memory.views.borrow().last().cloned();
    assert_eq!(
        last,
        Some((
            vec!["Home".to_string()],
            ViewState {
                view_mode: ViewMode::Rows,
                sort: SortBehavior::new(SortKey::ModifiedDate, false),
            }
        ))
    );
    assert_eq!(state.borrow().thumb_size(), ViewMode::Rows.thumb_size());

    // Another browser opening Home reads it back; Projects remembers nothing.
    let again = BrowserState::new(Box::new(home()), config);
    assert_eq!(again.borrow().view_mode(), ViewMode::Rows);
    assert_eq!(
        again.borrow().sort(),
        SortBehavior::new(SortKey::ModifiedDate, false)
    );
    BrowserState::activate(&again, 0, Activation::DoubleClick);
    assert_eq!(again.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(
        again.borrow().view_mode(),
        ViewMode::Rows,
        "an unremembered folder keeps the view"
    );
}

#[test]
fn remember_location_is_handed_the_path_from_the_root_with_the_state_free() {
    let memory = Rc::new(Memory::default());
    let config = BrowserConfig::builder().memory(memory.clone()).build();
    let state = BrowserState::new(Box::new(home()), config);
    *memory.browser.borrow_mut() = Some(Rc::downgrade(&state));
    BrowserState::activate(&state, 0, Activation::DoubleClick);
    BrowserState::remember_location(&state, "main");
    assert_eq!(
        *memory.locations.borrow(),
        vec![(
            "main".to_string(),
            vec!["Home".to_string(), "Projects".to_string()],
            true
        )]
    );
}

/// The app's data for one browser.
#[derive(Debug, Default, PartialEq)]
struct TapCount(u32);

/// Actions recording each activation, counting it in the app slot through the live handle.
#[derive(Default)]
struct Recording(RefCell<Vec<(String, Vec<String>, Activation)>>);

impl BrowserActions for Recording {
    fn menu(&self, _state: &BrowserRef, builtins: &Builtins) -> Vec<BrowserAction> {
        builtins.default_menu()
    }

    fn activate(&self, state: &BrowserRef, entry: &Entry, how: Activation) -> bool {
        let selection = state
            .selection()
            .iter()
            .map(|e| e.name().to_string())
            .collect();
        self.0
            .borrow_mut()
            .push((entry.name().to_string(), selection, how));
        state.with_app_slot::<TapCount, _>(|count| count.0 += 1);
        // The browser is free: the hook may change it.
        if let Some(live) = state.live() {
            live.borrow_mut().go_to(0);
        }
        true
    }
}

#[test]
fn an_items_activation_reaches_the_apps_actions_with_the_browser_free() {
    let actions = Rc::new(Recording::default());
    let config = BrowserConfig::builder().actions(actions.clone()).build();
    let state = BrowserState::with_config(Box::new(home()), config, TapCount::default());
    BrowserState::click(&state, 1, Default::default());
    BrowserState::activate(&state, 1, Activation::DoubleClick);
    assert_eq!(
        *actions.0.borrow(),
        vec![(
            "a.txt".to_string(),
            vec!["a.txt".to_string()],
            Activation::DoubleClick
        )]
    );
    assert_eq!(state.borrow().app_slot::<TapCount>(), Some(&TapCount(1)));
}

#[test]
fn a_folders_activation_opens_it_without_the_apps_actions() {
    let actions = Rc::new(Recording::default());
    let config = BrowserConfig::builder().actions(actions.clone()).build();
    let state = BrowserState::new(Box::new(home()), config);
    BrowserState::activate(&state, 0, Activation::Tap);
    assert!(actions.0.borrow().is_empty());
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
}

#[test]
fn a_tap_activates_on_its_release_only_where_taps_activate() {
    let actions = Rc::new(Recording::default());
    let options = agg_gui_file_browser::BrowserOptions {
        tap_activates: true,
        ..Default::default()
    };
    let config = BrowserConfig::builder()
        .actions(actions.clone())
        .options(options)
        .build();
    let state = BrowserState::new(Box::new(home()), config);
    BrowserState::press(&state, 1, Default::default());
    assert!(
        actions.0.borrow().is_empty(),
        "the press waits for its release"
    );
    assert!(state.borrow().tap_pending());
    BrowserState::finish_tap(&state, 1, true);
    assert_eq!(actions.0.borrow().len(), 1);
    assert_eq!(actions.0.borrow()[0].2, Activation::Tap);
}

#[test]
fn the_app_slot_holds_one_value_of_the_apps_own_type() {
    let state = BrowserState::with_config(Box::new(home()), BrowserConfig::default(), TapCount(3));
    let mut s = state.borrow_mut();
    assert_eq!(s.app_slot::<TapCount>(), Some(&TapCount(3)));
    assert_eq!(s.app_slot::<String>(), None, "another type finds nothing");
    if let Some(count) = s.app_slot_mut::<TapCount>() {
        count.0 = 4;
    }
    assert_eq!(s.app_slot::<TapCount>(), Some(&TapCount(4)));
    s.set_app_slot(String::from("replaced"));
    assert_eq!(s.app_slot::<TapCount>(), None);
    assert_eq!(s.app_slot::<String>().map(String::as_str), Some("replaced"));
}

#[test]
fn the_app_slot_gives_its_value_or_takes_a_new_one_of_the_type_asked_for() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let mut s = state.borrow_mut();
    assert_eq!(
        s.app_slot::<TapCount>(),
        None,
        "a browser made without app data"
    );
    s.app_slot_or_insert_with(TapCount::default).0 += 1;
    assert_eq!(s.app_slot::<TapCount>(), Some(&TapCount(1)));
    s.app_slot_or_insert_with(|| TapCount(9)).0 += 1;
    assert_eq!(
        s.app_slot::<TapCount>(),
        Some(&TapCount(2)),
        "the value already there is kept, not made again"
    );
    assert_eq!(s.app_slot_or_insert_with(|| String::from("other")), "other");
    assert_eq!(s.app_slot::<TapCount>(), None, "another type replaces it");
}

#[test]
fn a_snapshot_of_a_closed_browser_has_no_live_handle() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let snapshot = state.borrow().snapshot();
    assert!(snapshot.live().is_some());
    drop(state);
    assert!(snapshot.live().is_none());
    assert_eq!(snapshot.with_app_slot::<TapCount, _>(|c| c.0), None);
}
