//! The Move dialog's controller over an app's rules and reporting: a folder an app's
//! `PlaceRules::folder_inside` places inside a moved folder can't take it, even when the dialog
//! reached it another way than through the moved folder; and a showing folder still held
//! elsewhere at accept moves nothing and says so through the app's own report.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui_file_browser::{
    dialog_options, BrowserConfig, BrowserEntry, BrowserState, FolderKey, MoveAnswers,
    MoveController, MovePick, MoveRequest, MoveSource, MoveVerdict, PlaceRules,
};
use agg_gui_files::{DynamicFolder, DynamicFolderLink, Folder, FolderLink, MemoryFolder};

/// The top: Designs and Drafts, both writable.
fn root() -> Box<dyn Folder> {
    let mut root = DynamicFolder::new("Top");
    root.data.child_containers = ["Designs", "Drafts"]
        .into_iter()
        .map(|name| {
            let folder = MemoryFolder::new(name);
            folder.add_file("gear.stl", b"g".to_vec());
            Rc::new(DynamicFolderLink::new(
                name,
                Rc::new(move || Box::new(folder.clone()) as Box<dyn Folder>),
            )) as Rc<dyn FolderLink>
        })
        .collect();
    Box::new(root)
}

/// Folders told apart by name, with Drafts inside Designs (as a folder on disk can be inside
/// another while the dialog reaches it from a root of its own).
struct DraftsInsideDesigns;

impl PlaceRules for DraftsInsideDesigns {
    fn folder_key(&self, link: &dyn FolderLink) -> Option<FolderKey> {
        Some(FolderKey::new(None, link.name()))
    }
    fn open_folder_key(&self, folder: &dyn Folder) -> Option<FolderKey> {
        Some(FolderKey::new(None, folder.name()))
    }
    fn folder_inside(&self, inner: &FolderKey, outer: &FolderKey) -> bool {
        inner == outer || (inner.location() == "Drafts" && outer.location() == "Designs")
    }
}

fn browser(report: Option<Rc<RefCell<Vec<String>>>>) -> Rc<RefCell<BrowserState>> {
    let mut config = BrowserConfig::builder().options(dialog_options());
    if let Some(heard) = report {
        config = config.report(Rc::new(move |m: &str| heard.borrow_mut().push(m.into())));
    }
    BrowserState::new(root(), config.build())
}

fn link(state: &Rc<RefCell<BrowserState>>, name: &str) -> Rc<dyn FolderLink> {
    state
        .borrow()
        .entries()
        .iter()
        .find_map(|e| match e {
            BrowserEntry::Container(l) if l.name() == name => Some(Rc::clone(l)),
            _ => None,
        })
        .expect("the folder row")
}

fn answers(picked: &Rc<RefCell<Option<MovePick>>>) -> MoveAnswers {
    let picked = Rc::clone(picked);
    MoveAnswers {
        accept: Rc::new(move |pick| *picked.borrow_mut() = Some(pick)),
        cancel: Rc::new(|| {}),
    }
}

#[test]
fn a_folder_the_rules_place_inside_a_moved_folder_cant_take_it() {
    let state = browser(None);
    let designs = state.borrow().entries()[0].item();
    let request = |rules: Rc<dyn PlaceRules>| {
        MoveRequest::new(vec![Rc::clone(&designs)], None)
            .with_source("Top", None, Rc::new(|_| false))
            .with_places(rules)
    };
    let drafts = link(&state, "Drafts");
    state.borrow_mut().open(drafts.as_ref());
    let picked = Rc::default();
    let c = MoveController::new(
        &state,
        request(Rc::new(DraftsInsideDesigns)),
        answers(&picked),
    );
    assert_eq!(
        c.verdict(),
        MoveVerdict::Refused("Designs can't go inside itself. Choose another folder.".into())
    );
    // By default a folder is inside only itself.
    let plain = Rc::new(agg_gui_file_browser::DefaultPlaces);
    let c = MoveController::new(&state, request(plain), answers(&picked));
    assert_eq!(
        c.verdict(),
        MoveVerdict::Move("Move Designs to Drafts.".into())
    );
}

#[test]
fn a_held_folder_at_accept_moves_nothing_and_says_so_through_the_apps_report() {
    let heard = Rc::new(RefCell::new(Vec::new()));
    let state = browser(Some(Rc::clone(&heard)));
    let designs = link(&state, "Designs");
    state.borrow_mut().open(designs.as_ref());
    let gear = state.borrow().entries()[0].item();
    let request =
        MoveRequest::new(vec![gear], None).with_source("Elsewhere", None, Rc::new(|_| false));
    let picked: Rc<RefCell<Option<MovePick>>> = Rc::default();
    let c = MoveController::new(&state, request, answers(&picked));
    assert!(c.accept_enabled(), "{}", c.summary());
    let held = state.borrow().open_path();
    c.accept();
    assert!(picked.borrow().is_none(), "nothing moved");
    assert_eq!(
        *heard.borrow(),
        ["The folder was busy, so nothing moved. Try again."]
    );
    drop(held);
}

#[test]
fn a_live_source_is_read_at_each_verdict_and_a_gone_one_takes_nothing() {
    use std::cell::Cell;
    let state = browser(None);
    let gear = {
        let designs = link(&state, "Designs");
        let folder = designs.get_container().expect("Designs opens");
        folder.items()[0].clone()
    };
    // The folder the items leave: there or gone, keeping its items or not.
    let (there, keeps) = (Rc::new(Cell::new(true)), Rc::new(Cell::new(false)));
    let (t, k) = (Rc::clone(&there), Rc::clone(&keeps));
    let request = MoveRequest::new(vec![gear], None).with_live_source(Rc::new(move || {
        let keeps = k.get();
        t.get()
            .then(|| MoveSource::new(None, "Elsewhere", None, Rc::new(move |_: Option<&_>| keeps)))
    }));
    let drafts = link(&state, "Drafts");
    state.borrow_mut().open(drafts.as_ref());
    let picked: Rc<RefCell<Option<MovePick>>> = Rc::default();
    let c = MoveController::new(&state, request, answers(&picked));
    assert_eq!(
        c.verdict(),
        MoveVerdict::Move("Move gear.stl to Drafts.".into())
    );
    keeps.set(true);
    assert_eq!(
        c.verdict(),
        MoveVerdict::Copy("Copy gear.stl to Drafts. The original stays in Elsewhere.".into())
    );
    there.set(false);
    assert_eq!(
        c.verdict(),
        MoveVerdict::Refused("Choose a folder you can add to.".into())
    );
    assert!(!c.accept_enabled());
    c.accept();
    assert!(picked.borrow().is_none(), "a gone source moves nothing");
    there.set(true);
    c.accept();
    assert!(
        picked.borrow().as_ref().is_some_and(|p| p.copy),
        "the pick copies as the line said"
    );
}

#[test]
fn a_row_that_cant_open_at_accept_says_so_through_the_apps_report() {
    let heard = Rc::new(RefCell::new(Vec::<String>::new()));
    let gone = DynamicFolderLink::new_optional("Gone", Rc::new(|| None));
    let mut root = DynamicFolder::new("Top");
    root.data.child_containers = vec![Rc::new(gone) as Rc<dyn FolderLink>];
    let heard_too = Rc::clone(&heard);
    let state = BrowserState::new(
        Box::new(root),
        BrowserConfig::builder()
            .options(dialog_options())
            .report(Rc::new(move |m: &str| {
                heard_too.borrow_mut().push(m.into())
            }))
            .build(),
    );
    let item = {
        let folder = MemoryFolder::new("Elsewhere");
        folder.add_file("gear.stl", b"g".to_vec());
        folder.items()[0].clone()
    };
    state.borrow_mut().ensure_selected(0, false);
    let request =
        MoveRequest::new(vec![item], None).with_source("Elsewhere", None, Rc::new(|_| false));
    let picked: Rc<RefCell<Option<MovePick>>> = Rc::default();
    let c = MoveController::new(&state, request, answers(&picked));
    assert!(c.accept_enabled(), "{}", c.summary());
    c.accept();
    assert!(picked.borrow().is_none(), "nothing moved");
    assert_eq!(
        *heard.borrow(),
        ["Couldn't move to \"Gone\": Could not be opened"]
    );
}
