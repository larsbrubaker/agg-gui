//! `BrowserState` on a `MemoryFolder` tree: opening folders and walking back up the breadcrumbs,
//! the selection gestures, the listing following the showing folder's content, and the status
//! line a browser without the app's report shows its errors on.

use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_file_browser::{
    Activation, BrowserConfig, BrowserEntry, BrowserState, DoubleClickTarget, ItemEvent,
};
use agg_gui_files::{Folder, FolderLink, Item, MemoryFolder};

/// Home holding the folders Projects (holding Brackets) and Notes, and the files b.txt, a.txt.
fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("Projects is free");
    projects.add_folder("Brackets").expect("Brackets is free");
    home.add_folder("Notes").expect("Notes is free");
    home.add_file("b.txt", b"b".to_vec());
    home.add_file("a.txt", b"a".to_vec());
    home
}

fn names(state: &BrowserState) -> Vec<String> {
    state
        .entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect()
}

fn selected(state: &BrowserState) -> Vec<String> {
    state
        .selected_entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect()
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

#[test]
fn folders_list_first_then_items_each_sorted_by_name() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    assert_eq!(
        names(&state.borrow()),
        ["Notes", "Projects", "a.txt", "b.txt"]
    );
}

#[test]
fn activating_a_folder_row_opens_it_and_a_breadcrumb_goes_back_up() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    BrowserState::activate(&state, 1, Activation::DoubleClick);
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(names(&state.borrow()), ["Brackets"]);
    BrowserState::activate(&state, 0, Activation::DoubleClick);
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
    assert_eq!(state.borrow().depth(), 3);
    let navigation = state.borrow().navigation();
    state.borrow_mut().go_to(0);
    let s = state.borrow();
    assert_eq!(s.path_names(), ["Home"]);
    assert_eq!(
        s.navigation(),
        navigation + 1,
        "walking back up is a navigation"
    );
    assert_eq!(names(&s), ["Notes", "Projects", "a.txt", "b.txt"]);
}

#[test]
fn going_to_the_folder_showing_changes_nothing() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let generation = state.borrow().generation();
    state.borrow_mut().go_to(0);
    assert_eq!(state.borrow().generation(), generation);
}

#[test]
fn a_plain_click_selects_only_its_row_and_ctrl_toggles() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    BrowserState::click(&state, 2, Modifiers::default());
    assert_eq!(selected(&state.borrow()), ["a.txt"]);
    BrowserState::click(&state, 3, ctrl());
    assert_eq!(selected(&state.borrow()), ["a.txt", "b.txt"]);
    BrowserState::click(&state, 2, ctrl());
    assert_eq!(
        selected(&state.borrow()),
        ["b.txt"],
        "Ctrl on a selected row takes it out"
    );
    BrowserState::click(&state, 0, Modifiers::default());
    assert_eq!(selected(&state.borrow()), ["Notes"]);
    assert!(state.borrow().is_selected(0));
    assert!(!state.borrow().is_selected(3));
}

#[test]
fn shift_selects_the_range_from_the_anchor() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    BrowserState::click(&state, 3, Modifiers::default());
    BrowserState::click(&state, 1, shift());
    assert_eq!(selected(&state.borrow()), ["Projects", "a.txt", "b.txt"]);
}

#[test]
fn a_press_on_one_of_several_selected_rows_keeps_them_until_its_release() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    state.borrow_mut().select_all();
    BrowserState::press(&state, 2, Modifiers::default());
    assert_eq!(
        selected(&state.borrow()).len(),
        4,
        "a drag carries the whole selection"
    );
    state.borrow_mut().release_press(true);
    assert_eq!(selected(&state.borrow()).len(), 4, "a drag keeps it");
    BrowserState::press(&state, 2, Modifiers::default());
    state.borrow_mut().release_press(false);
    assert_eq!(selected(&state.borrow()), ["a.txt"], "a click collapses it");
}

#[test]
fn a_rubber_band_selects_the_rows_it_covers_and_escape_clears() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let mut s = state.borrow_mut();
    s.begin_band(agg_gui::Point::new(0.0, 0.0), false);
    assert!(
        s.drag_band(agg_gui::Point::new(1.0, 1.0)).is_none(),
        "a wobble draws no band"
    );
    assert!(s.drag_band(agg_gui::Point::new(50.0, 50.0)).is_some());
    s.select_band(&[1, 2]);
    assert!(s.end_band(true), "a drawn band keeps what it selected");
    assert_eq!(selected(&s), ["Projects", "a.txt"]);
    assert!(s.clear_selection());
    assert!(selected(&s).is_empty());
}

#[test]
fn a_change_in_the_showing_folder_lists_it_again_from_the_top() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    BrowserState::click(&state, 2, Modifiers::default());
    let (resets, generation) = {
        let s = state.borrow();
        (s.list_resets(), s.generation())
    };
    root.add_file("c.txt", b"c".to_vec());
    state.borrow_mut().sync_content();
    let s = state.borrow();
    assert_eq!(names(&s), ["Notes", "Projects", "a.txt", "b.txt", "c.txt"]);
    assert!(s.generation() > generation, "the view rebuilds");
    assert_eq!(s.list_resets(), resets + 1, "the list goes back to the top");
    assert!(selected(&s).is_empty(), "new content drops the selection");
}

#[test]
fn a_folder_left_behind_no_longer_refreshes_the_list() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    BrowserState::activate(&state, 1, Activation::DoubleClick);
    let changes = state.borrow().content_changes();
    root.add_file("c.txt", b"c".to_vec());
    assert_eq!(
        state.borrow().content_changes(),
        changes,
        "only the showing folder is heard"
    );
}

#[test]
fn rows_waiting_to_be_selected_are_selected_once_listed() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    let copy = root.add_file("copy.txt", b"c".to_vec());
    state
        .borrow_mut()
        .select_when_listed(vec![agg_gui_file_browser::item_identity_key(copy.as_ref())]);
    let mut s = state.borrow_mut();
    assert_eq!(selected(&s), ["copy.txt"]);
    assert!(s.take_reveal_selection());
    assert!(!s.take_reveal_selection(), "answering clears it");
}

#[test]
fn hiding_folders_and_a_folder_filter_leave_their_rows_out() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    state.borrow_mut().set_show_containers(false);
    assert_eq!(names(&state.borrow()), ["a.txt", "b.txt"]);
    let options = agg_gui_file_browser::BrowserOptions {
        folder_filter: Some(Rc::new(|link: &dyn FolderLink| link.name() != "Notes")),
        ..Default::default()
    };
    let config = BrowserConfig::builder().options(options).build();
    let state = BrowserState::new(Box::new(home()), config);
    assert_eq!(names(&state.borrow()), ["Projects", "a.txt", "b.txt"]);
}

#[test]
fn a_host_hears_clicks_and_double_clicks_on_items_but_not_folders() {
    let options = agg_gui_file_browser::BrowserOptions {
        double_click: DoubleClickTarget::Host,
        ..Default::default()
    };
    let config = BrowserConfig::builder().options(options).build();
    let state = BrowserState::new(Box::new(home()), config);
    BrowserState::click(&state, 0, Modifiers::default());
    BrowserState::click(&state, 2, Modifiers::default());
    BrowserState::activate(&state, 2, Activation::DoubleClick);
    let events = state.borrow_mut().take_item_events();
    let seen: Vec<String> = events
        .iter()
        .map(|e| match e {
            ItemEvent::Clicked(entry) => format!("click {}", entry.name()),
            ItemEvent::Activated(entry) => format!("activate {}", entry.name()),
        })
        .collect();
    assert_eq!(seen, ["click a.txt", "activate a.txt"]);
    assert!(state.borrow_mut().take_item_events().is_empty());
}

#[test]
fn without_the_apps_report_an_error_shows_on_the_status_line() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let report = state.borrow().report();
    let held = state.borrow();
    report("Couldn't move b.txt");
    assert_eq!(
        held.status(),
        "Couldn't move b.txt",
        "reported while the state was borrowed"
    );
}

#[test]
fn with_the_apps_report_the_status_line_stays_empty() {
    let heard = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let to = Rc::clone(&heard);
    let config = BrowserConfig::builder()
        .report(Rc::new(move |m: &str| to.borrow_mut().push(m.into())))
        .build();
    let state = BrowserState::new(Box::new(home()), config);
    (state.borrow().report())("Couldn't rename a.txt");
    assert_eq!(*heard.borrow(), ["Couldn't rename a.txt"]);
    assert_eq!(state.borrow().status(), "");
}

#[test]
fn a_showing_folder_still_held_is_kept_and_the_user_told_why_it_did_not_change() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    BrowserState::activate(&state, 1, Activation::DoubleClick);
    let held = state.borrow().open_path();
    let replaced = std::cell::Cell::new(false);
    state.borrow_mut().replace_active_container(|folder| {
        replaced.set(true);
        folder
    });
    assert!(!replaced.get(), "a folder still held is not replaced");
    let s = state.borrow();
    assert_eq!(s.path_names(), ["Home", "Projects"], "it stays showing");
    assert_eq!(
        s.status(),
        "This folder is busy. Open it again to see the change."
    );
    drop(held);
}

#[test]
fn a_location_that_cannot_be_found_is_said_on_the_status_line() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let item: Rc<dyn Item> = state.borrow().entries()[2].item();
    state.borrow_mut().show_location(item.as_ref(), 0, None);
    assert_eq!(
        state.borrow().status(),
        "Couldn't find the location of \"a.txt\""
    );
}

#[test]
fn a_location_found_is_shown_below_its_base() {
    let root = home();
    let projects = root.add_folder("Projects").expect("Projects is there");
    let brackets = projects.add_folder("Brackets").expect("Brackets is there");
    let state = BrowserState::new(Box::new(root), BrowserConfig::default());
    let item: Rc<dyn Item> = state.borrow().entries()[2].item();
    let located: Vec<Box<dyn Folder>> = vec![Box::new(projects), Box::new(brackets)];
    state
        .borrow_mut()
        .show_location(item.as_ref(), 0, Some(located));
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"]
    );
}

#[test]
fn a_folder_row_entry_is_a_container() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let entries = state.borrow().entries();
    assert!(matches!(entries[0], BrowserEntry::Container(_)));
    assert!(!entries[2].is_container());
}

#[test]
fn rows_can_be_selected_before_any_view_has_listed_them() {
    // A memory folder hands out new link objects per listing, so a row index has to mean the
    // listing the state made, not a fresh one.
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    BrowserState::click(&state, 0, Modifiers::default());
    assert!(
        state.borrow().is_selected(0),
        "the folder row clicked is selected"
    );
    assert_eq!(selected(&state.borrow()), ["Notes"]);
    BrowserState::activate(&state, 1, Activation::DoubleClick);
    BrowserState::click(&state, 0, Modifiers::default());
    assert!(
        state.borrow().is_selected(0),
        "the opened folder's rows are listed too"
    );
    assert_eq!(selected(&state.borrow()), ["Brackets"]);
}

#[test]
fn without_a_view_a_content_change_lists_the_new_rows() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    root.add_file("c.txt", b"c".to_vec());
    state.borrow_mut().sync_content();
    let row = state.borrow().listed_entry(4).map(|e| e.name().to_string());
    assert_eq!(row.as_deref(), Some("c.txt"));
}

#[test]
fn once_a_view_lists_the_rows_an_index_means_what_it_last_built() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    state.borrow_mut().mark_listed();
    root.add_file("c.txt", b"c".to_vec());
    state.borrow_mut().sync_content();
    assert!(
        state.borrow().listed_entry(4).is_none(),
        "the list on screen has not shown c.txt yet"
    );
    state.borrow_mut().mark_listed();
    let row = state.borrow().listed_entry(4).map(|e| e.name().to_string());
    assert_eq!(row.as_deref(), Some("c.txt"));
}

#[test]
fn a_taken_folder_put_back_shows_again_with_its_listing() {
    let state = BrowserState::new(Box::new(home()), BrowserConfig::default());
    let projects = state.borrow().root_child_containers()[0].clone();
    state.borrow_mut().open(projects.as_ref());
    let taken = state
        .borrow_mut()
        .take_active_container()
        .expect("below the root");
    assert_eq!(state.borrow().path_names(), ["Home"]);
    let held = Rc::clone(&taken);
    state.borrow_mut().put_back_active_container(taken);
    assert_eq!(state.borrow().path_names(), ["Home", "Projects"]);
    assert_eq!(names(&state.borrow()), ["Brackets"]);
    drop(held);
    assert!(state.borrow().can_hand_over_active_container());
}
