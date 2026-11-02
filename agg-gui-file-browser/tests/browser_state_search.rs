//! `BrowserState` showing a search's results over a `MemoryFolder` tree: the results stand below
//! the folder searched, list in sections with their headers, keep their selection when shown
//! again, carry a rolled-up section's selected results until it opens, and leave the path when
//! the user walks back up.

use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserState, ListSlot, LocalSpawner,
    SearchSectionCollapseState,
};
use agg_gui_files::{Folder, MemoryFolder, SearchResultsFolder, SortKey};

fn home() -> MemoryFolder {
    let home = MemoryFolder::new("Home");
    home.add_file("gear b.stl", b"b".to_vec());
    home.add_file("gear a.stl", b"a".to_vec());
    home.add_file("bracket.stl", b"c".to_vec());
    home
}

/// `root`'s search for `query`, run to its end on a local spawner.
fn finished_search(root: &dyn Folder, query: &str) -> Rc<SearchResultsFolder> {
    let spawner = Rc::new(LocalSpawner::new(Rc::new(|m: &str| panic!("fault: {m}"))));
    let mut results = SearchResultsFolder::new(root, query, spawner);
    results.start_search();
    for _ in 0..1000 {
        if results.is_finished() {
            break;
        }
        run_ready_local_tasks();
    }
    assert!(results.is_finished(), "the search ends");
    Rc::new(results)
}

fn names(state: &BrowserState) -> Vec<String> {
    state
        .entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect()
}

fn selected(state: &BrowserState) -> Vec<String> {
    let entries = state.selected_entries();
    entries.iter().map(|e| e.name().to_string()).collect()
}

fn section_names(state: &BrowserState) -> Vec<(String, bool, Option<String>)> {
    state
        .list_slots()
        .into_iter()
        .filter_map(|slot| match slot {
            ListSlot::Section(h) => Some((h.name, h.collapsed, h.status)),
            _ => None,
        })
        .collect()
}

#[test]
fn results_show_in_sections_below_the_folder_searched_and_leave_with_it() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    let results = finished_search(&root, "gear");
    state.borrow_mut().push_search_results(Rc::clone(&results));
    {
        let s = state.borrow();
        assert_eq!(s.depth(), 2, "the results stand below the folder searched");
        assert!(s.showing_search_results().is_some());
        assert_eq!(names(&s), ["gear a.stl", "gear b.stl"]);
        let sections = section_names(&s);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].2.as_deref(), Some("(2)"));
    }
    state.borrow_mut().go_to(0);
    let s = state.borrow();
    assert!(s.showing_search_results().is_none());
    assert_eq!(names(&s), ["bracket.stl", "gear a.stl", "gear b.stl"]);
}

#[test]
fn the_same_results_shown_again_keep_their_selection() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    state
        .borrow_mut()
        .push_search_results(finished_search(&root, "gear"));
    BrowserState::click(&state, 1, Modifiers::default());
    assert_eq!(selected(&state.borrow()), ["gear b.stl"]);
    let resets = state.borrow().list_resets();
    state.borrow_mut().set_user_sort_ascending(false);
    let s = state.borrow();
    assert_eq!(names(&s), ["gear b.stl", "gear a.stl"]);
    assert_eq!(
        selected(&s),
        ["gear b.stl"],
        "a re-sort keeps the selection"
    );
    assert_eq!(s.list_resets(), resets, "and the list's place");
}

#[test]
fn a_rolled_up_section_carries_its_selected_results_until_it_opens() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    state
        .borrow_mut()
        .push_search_results(finished_search(&root, "gear"));
    BrowserState::click(&state, 0, Modifiers::default());
    let section = section_names(&state.borrow())[0].0.clone();
    state.borrow_mut().toggle_search_section(&section);
    {
        let s = state.borrow();
        assert!(names(&s).is_empty(), "a rolled-up section hides its rows");
        assert!(section_names(&s)[0].1);
        assert!(selected(&s).is_empty());
    }
    state.borrow_mut().toggle_search_section(&section);
    assert_eq!(selected(&state.borrow()), ["gear a.stl"]);
    assert!(!SearchSectionCollapseState::session(
        |s| s.is_collapsed(section.as_str())
    ));
}

#[test]
fn a_sort_key_picked_while_searching_sorts_the_results() {
    let root = home();
    let state = BrowserState::new(Box::new(root.clone()), BrowserConfig::default());
    state
        .borrow_mut()
        .push_search_results(finished_search(&root, "gear"));
    state.borrow_mut().set_user_sort_key(SortKey::Name);
    assert_eq!(names(&state.borrow()), ["gear a.stl", "gear b.stl"]);
}
