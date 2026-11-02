//! The folder tree's rows as data (`TreeModel`): a folder that cannot answer cheaply keeps its
//! arrow until opened, loses it when opening finds no folders and stays without it across a
//! relist, and is never made just to ask; an opened folder whose listing is on its way shows a
//! "Loading..." row, then fills in or folds without its arrow once the listing lands (also when
//! the browser's own copy lands first); folding meanwhile stops listening; a failed listing folds
//! the row but keeps its arrow; a landing elsewhere doesn't reopen what the user folded, while
//! one on the way to the browser's folder lets the walk continue; a folder already waiting for
//! its listing never starts a second one; and two same-named folders in different places stay
//! two rows, the one that was open reopening after a relist.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui_file_browser::{BrowserConfig, BrowserState, TreeModel};
use agg_gui_files::{
    ContentChangedEvent, ContentChangedListener, DynamicFolder, DynamicFolderLink, Folder,
    FolderLink, Item, MemoryFolder, MessageItem,
};

/// A folder with no folders whose container counts how often it is made; it does not say
/// whether it has folders unless `with_local_listing` is added.
fn empty_folder(name: &str, made: &Rc<Cell<u32>>) -> DynamicFolderLink {
    let made = Rc::clone(made);
    let name_owned = name.to_string();
    DynamicFolderLink::new(
        name,
        Rc::new(move || {
            made.set(made.get() + 1);
            Box::new(DynamicFolder::new(name_owned.clone())) as Box<dyn Folder>
        }),
    )
}

/// A listing the test lands when it says so (`None` while on its way).
type Listing = Rc<RefCell<Option<Vec<Rc<dyn FolderLink>>>>>;

/// A container whose listing lands when the test says so.
struct SlowContainer {
    listing: Listing,
    failed: Rc<Cell<bool>>,
    changed: ContentChangedEvent,
}

impl Folder for SlowContainer {
    fn id(&self) -> &str {
        ""
    }
    fn name(&self) -> &str {
        "Slow"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.listing.borrow().clone().unwrap_or_default()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        Vec::new()
    }
    fn load(&mut self) {}
    fn is_load_pending(&self) -> bool {
        self.listing.borrow().is_none()
    }
    fn last_load_failed(&self) -> bool {
        self.failed.get()
    }
    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        self.changed.subscribe(listener);
    }
    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        self.changed.unsubscribe(listener);
    }
}

/// What lands a "Slow" folder's listing: the listing, whether it failed, and the event it raises.
struct Landing {
    listing: Listing,
    failed: Rc<Cell<bool>>,
    changed: ContentChangedEvent,
}

impl Landing {
    fn land(&self, links: Vec<Rc<dyn FolderLink>>) {
        *self.listing.borrow_mut() = Some(links);
        self.changed.raise();
    }
}

/// A remote "Slow" folder whose every container shares one listing, as a cloud folder's do.
fn slow_folder() -> (DynamicFolderLink, Landing) {
    let landing = Landing {
        listing: Rc::new(RefCell::new(None)),
        failed: Rc::new(Cell::new(false)),
        changed: ContentChangedEvent::default(),
    };
    let (l, f, c) = (
        Rc::clone(&landing.listing),
        Rc::clone(&landing.failed),
        landing.changed.clone(),
    );
    let link = DynamicFolderLink::new(
        "Slow",
        Rc::new(move || {
            Box::new(SlowContainer {
                listing: Rc::clone(&l),
                failed: Rc::clone(&f),
                changed: c.clone(),
            }) as Box<dyn Folder>
        }),
    );
    (link, landing)
}

fn tree(links: Vec<Rc<dyn FolderLink>>) -> (Rc<RefCell<BrowserState>>, TreeModel) {
    let mut root = DynamicFolder::new("Home");
    root.data.child_containers = links;
    let state = BrowserState::new(Box::new(root), BrowserConfig::default());
    let model = TreeModel::for_state(&state);
    let model = Rc::try_unwrap(model).ok().expect("one owner").into_inner();
    (state, model)
}

fn row(model: &TreeModel, name: &str) -> usize {
    model
        .rows()
        .iter()
        .position(|n| n.name() == name)
        .unwrap_or_else(|| panic!("{name} is a row"))
}

/// The "Loading..." row one level in.
fn loading() -> String {
    format!("  {}", MessageItem::loading().name())
}

#[test]
fn a_remote_folder_keeps_its_arrow_until_opened_and_loses_it_for_good_when_empty() {
    let (local_made, remote_made) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (state, mut model) = tree(vec![
        Rc::new(empty_folder("Local", &local_made).with_local_listing()),
        Rc::new(empty_folder("Remote", &remote_made)),
    ]);
    // The local folder is asked; the remote one is not even made.
    assert!(!model.rows()[row(&model, "Local")].has_arrow());
    assert!(local_made.get() > 0);
    assert!(model.rows()[row(&model, "Remote")].has_arrow());
    assert_eq!(remote_made.get(), 0);

    // Opening finds nothing: no open arrow over nothing, and no arrow at all.
    let remote = row(&model, "Remote");
    model.toggle(remote, true);
    assert!(!model.rows()[remote].is_expanded());
    assert!(!model.rows()[remote].has_arrow());
    assert_eq!(model.rows().len(), 2);

    // A relist (after a File-menu write) does not bring the arrow back.
    model.relist(&state.borrow());
    assert!(!model.rows()[row(&model, "Remote")].has_arrow());
}

#[test]
fn a_pending_listing_shows_loading_then_fills_in_its_folders_when_it_lands() {
    let (slow, landing) = slow_folder();
    let made = Rc::new(Cell::new(0));
    let (state, mut model) = tree(vec![Rc::new(slow)]);
    let index = row(&model, "Slow");
    assert!(model.rows()[index].has_arrow());

    model.toggle(index, true);
    assert!(model.rows()[index].is_expanded());
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow".to_string(), loading()]);
    assert!(!model.rows()[index + 1].has_arrow());

    landing.land(vec![Rc::new(
        empty_folder("Inner", &made).with_local_listing(),
    )]);
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow", "  Inner"]);
    assert!(model.rows()[index].has_arrow());
    assert!(!model.rows()[row(&model, "Inner")].has_arrow());
}

#[test]
fn a_pending_listing_with_no_folders_folds_the_row_and_takes_its_arrow() {
    let (slow, landing) = slow_folder();
    let (state, mut model) = tree(vec![Rc::new(slow)]);
    let index = row(&model, "Slow");
    model.toggle(index, true);
    assert!(model.rows()[index].is_expanded());

    landing.land(Vec::new());
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow"]);
    assert!(!model.rows()[index].is_expanded());
    assert!(!model.rows()[index].has_arrow());

    model.relist(&state.borrow());
    assert!(!model.rows()[row(&model, "Slow")].has_arrow());
}

#[test]
fn folding_a_folder_while_its_listing_is_on_its_way_stops_waiting_for_it() {
    let (slow, landing) = slow_folder();
    let (state, mut model) = tree(vec![Rc::new(slow)]);
    let index = row(&model, "Slow");
    model.toggle(index, true);
    assert_eq!(model.waiting_listings(), 1);
    model.toggle(index, true);
    assert!(model.waiting_listings() == 0);
    assert_eq!(model.indented_names(), vec!["Slow"]);

    // The folded row's listener is gone: a landing changes nothing, and the arrow stays.
    landing.land(Vec::new());
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow"]);
    assert!(!model.rows()[index].is_expanded());
    assert!(model.rows()[index].has_arrow());
}

#[test]
fn the_browsers_listing_landing_first_answers_the_trees_wait_too() {
    let (slow, landing) = slow_folder();
    let made = Rc::new(Cell::new(0));
    let link: Rc<dyn FolderLink> = Rc::new(slow);
    let (state, mut model) = tree(vec![Rc::clone(&link)]);
    state.borrow_mut().open(link.as_ref());
    model.sync_to(&state.borrow());
    let index = row(&model, "Slow");
    model.toggle(index, true);
    assert_eq!(model.indented_names(), vec!["Slow".to_string(), loading()]);

    landing.land(vec![Rc::new(
        empty_folder("Inner", &made).with_local_listing(),
    )]);
    model.content_changed(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow", "  Inner"]);
    assert!(model.waiting_listings() == 0);
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow", "  Inner"]);
}

#[test]
fn a_failed_listing_folds_the_row_and_keeps_its_arrow() {
    let (slow, landing) = slow_folder();
    let (state, mut model) = tree(vec![Rc::new(slow)]);
    let index = row(&model, "Slow");
    model.toggle(index, true);

    landing.failed.set(true);
    landing.land(Vec::new());
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow"]);
    assert!(!model.rows()[index].is_expanded());
    assert!(model.rows()[index].has_arrow());

    model.relist(&state.borrow());
    assert!(model.rows()[row(&model, "Slow")].has_arrow());
}

/// A folder listing `children`, which answers from memory whether it has folders.
fn folder_with(name: &str, children: Vec<Rc<dyn FolderLink>>) -> Rc<dyn FolderLink> {
    let name_owned = name.to_string();
    Rc::new(
        DynamicFolderLink::new(
            name,
            Rc::new(move || {
                let mut container = DynamicFolder::new(name_owned.clone());
                container.data.child_containers = children.clone();
                Box::new(container) as Box<dyn Folder>
            }),
        )
        .with_local_listing(),
    )
}

#[test]
fn a_landing_elsewhere_does_not_reopen_a_folder_the_user_folded() {
    let made = Rc::new(Cell::new(0));
    let leaf: Rc<dyn FolderLink> = Rc::new(empty_folder("Leaf", &made).with_local_listing());
    let local = folder_with("Local", vec![Rc::clone(&leaf)]);
    let (slow, landing) = slow_folder();
    let (state, mut model) = tree(vec![Rc::clone(&local), Rc::new(slow)]);
    state.borrow_mut().open(local.as_ref());
    state.borrow_mut().open(leaf.as_ref());
    model.sync_to(&state.borrow());
    let local_row = row(&model, "Local");
    assert!(
        model.rows()[local_row].is_expanded(),
        "opened down to the browser's folder"
    );

    model.toggle(local_row, true);
    model.toggle(row(&model, "Slow"), true);
    landing.land(Vec::new());
    model.sync_to(&state.borrow());
    assert!(!model.rows()[row(&model, "Local")].is_expanded());
    assert_eq!(model.indented_names(), vec!["Local", "Slow"]);
}

#[test]
fn a_landing_on_the_way_to_the_browsers_folder_lets_the_walk_continue() {
    let made = Rc::new(Cell::new(0));
    let deep: Rc<dyn FolderLink> = Rc::new(empty_folder("Deep", &made).with_local_listing());
    let inner = folder_with("Inner", vec![Rc::clone(&deep)]);
    let (slow, landing) = slow_folder();
    let slow: Rc<dyn FolderLink> = Rc::new(slow);
    let (state, mut model) = tree(vec![Rc::clone(&slow)]);
    for link in [&slow, &inner, &deep] {
        state.borrow_mut().open(link.as_ref());
    }
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow".to_string(), loading()]);

    landing.land(vec![Rc::clone(&inner)]);
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Slow", "  Inner", "    Deep"]);
}

/// A root row "Parts" listing two folders named "Twin", each in its own place, the second
/// holding "Inside Second" and the first "Inside First".
fn twins() -> Rc<dyn FolderLink> {
    let (first, second) = (MemoryFolder::new("Twin"), MemoryFolder::new("Twin"));
    first.add_folder("Inside First").expect("free");
    second.add_folder("Inside Second").expect("free");
    folder_with("Parts", vec![first.link(), second.link()])
}

#[test]
fn a_relist_reopens_the_folder_that_was_open_not_its_same_named_sibling() {
    let parts = twins();
    let (state, mut model) = tree(vec![Rc::clone(&parts)]);
    model.toggle(row(&model, "Parts"), true);
    assert_eq!(model.indented_names(), vec!["Parts", "  Twin", "  Twin"]);
    model.toggle(2, true);
    assert_eq!(
        model.indented_names(),
        vec!["Parts", "  Twin", "  Twin", "    Inside Second"]
    );

    // "Parts" showing changes: its folders are listed again, and only the second twin reopens.
    state.borrow_mut().open(parts.as_ref());
    model.content_changed(&state.borrow());
    assert_eq!(
        model.indented_names(),
        vec!["Parts", "  Twin", "  Twin", "    Inside Second"]
    );
    // A write lists the whole tree again, with the same result.
    model.relist(&state.borrow());
    assert_eq!(
        model.indented_names(),
        vec!["Parts", "  Twin", "  Twin", "    Inside Second"]
    );
}

#[test]
fn the_row_drawn_selected_is_the_twin_the_list_shows() {
    let parts = twins();
    let (state, mut model) = tree(vec![Rc::clone(&parts)]);
    model.toggle(row(&model, "Parts"), true);
    let second = model.rows()[2].path().last().cloned().expect("a link");
    state.borrow_mut().open(parts.as_ref());
    state.borrow_mut().open(second.as_ref());
    assert_eq!(model.showing_row(&state.borrow().path_levels()), Some(2));
    let first = model.rows()[1].path().last().cloned().expect("a link");
    state.borrow_mut().go_to(1);
    state.borrow_mut().open(first.as_ref());
    assert_eq!(model.showing_row(&state.borrow().path_levels()), Some(1));
}

#[test]
fn a_folder_waiting_for_its_listing_is_not_listed_again_when_its_parent_relists() {
    let (slow, landing) = slow_folder();
    let slow: Rc<dyn FolderLink> = Rc::new(slow);
    let local = folder_with("Local", vec![Rc::clone(&slow)]);
    let (state, mut model) = tree(vec![Rc::clone(&local)]);
    model.toggle(row(&model, "Local"), true);
    model.toggle(row(&model, "Slow"), true);
    assert_eq!(model.waiting_listings(), 1);

    // "Local" showing changes while "Slow" waits: Slow stays open on its "Loading..." row and
    // keeps its one wait.
    state.borrow_mut().open(local.as_ref());
    model.content_changed(&state.borrow());
    assert_eq!(model.waiting_listings(), 1, "no second listing for Slow");
    assert_eq!(
        model.indented_names(),
        vec![
            "Local".to_string(),
            "  Slow".to_string(),
            format!("  {}", loading())
        ]
    );

    let made = Rc::new(Cell::new(0));
    landing.land(vec![Rc::new(
        empty_folder("Inner", &made).with_local_listing(),
    )]);
    model.sync_to(&state.borrow());
    assert_eq!(model.indented_names(), vec!["Local", "  Slow", "    Inner"]);
    assert!(model.waiting_listings() == 0);
}
