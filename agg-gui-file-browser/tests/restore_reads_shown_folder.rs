//! A restored location reads only the folder it shows: the folders above it go on the path for
//! the breadcrumbs without being listed or activated, so reopening a dialog deep in a tree whose
//! upper folders are huge (a temp folder, a full Downloads) costs one listing, not one per level.
//! Going back up to such a folder reads it then, once, and shows its rows.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui_file_browser::{
    run_ready_local_tasks, BrowserConfig, BrowserMemory, BrowserState, LocationFuture,
};
use agg_gui_files::{Folder, FolderLink, Item, MemoryFolder};

/// How often a folder was listed and activated.
#[derive(Default)]
struct Reads {
    loads: Cell<u32>,
    activations: Cell<u32>,
}

/// A folder that counts its listings and activations, listing what `inner` holds.
struct Counted {
    inner: MemoryFolder,
    reads: Rc<Reads>,
}

impl Counted {
    fn new(inner: MemoryFolder) -> (Self, Rc<Reads>) {
        let reads = Rc::new(Reads::default());
        (
            Self {
                inner,
                reads: Rc::clone(&reads),
            },
            reads,
        )
    }
}

impl Folder for Counted {
    fn id(&self) -> &str {
        self.inner.id()
    }
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn is_protected(&self) -> bool {
        self.inner.is_protected()
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.inner.child_containers()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.inner.items()
    }
    fn load(&mut self) {
        self.reads.loads.set(self.reads.loads.get() + 1);
        self.inner.load();
    }
    fn activate(&mut self) {
        self.reads.activations.set(self.reads.activations.get() + 1);
    }
}

/// A memory that finds Home > Projects > Brackets at once, as a folder on disk is found.
struct Found(RefCell<Option<Vec<Box<dyn Folder>>>>);

impl BrowserMemory for Found {
    fn last_location(&self, _key: &str) -> Option<LocationFuture> {
        let folders = self.0.borrow_mut().take();
        Some(Box::pin(std::future::ready(folders)))
    }
}

/// A browser on Home restored into Projects > Brackets, with Projects' and Brackets' counts.
fn restored() -> (Rc<RefCell<BrowserState>>, Rc<Reads>, Rc<Reads>) {
    let home = MemoryFolder::new("Home");
    let projects = home.add_folder("Projects").expect("free");
    projects.add_file("gear.stl", b"g".to_vec());
    let brackets = projects.add_folder("Brackets").expect("free");
    brackets.add_file("bracket.stl", b"b".to_vec());
    let (projects, projects_reads) = Counted::new(projects);
    let (brackets, brackets_reads) = Counted::new(brackets);
    let memory = Rc::new(Found(RefCell::new(Some(vec![
        Box::new(projects) as Box<dyn Folder>,
        Box::new(brackets),
    ]))));
    let config = BrowserConfig::builder()
        .memory(memory as Rc<dyn BrowserMemory>)
        .build();
    let state = BrowserState::new(Box::new(home), config);
    BrowserState::restore_location(&state, "dialog");
    run_ready_local_tasks();
    (state, projects_reads, brackets_reads)
}

fn names(state: &Rc<RefCell<BrowserState>>) -> Vec<String> {
    state
        .borrow()
        .entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect()
}

#[test]
fn a_restored_location_reads_only_the_folder_it_shows() {
    let (state, projects, brackets) = restored();
    assert_eq!(
        state.borrow().path_names(),
        ["Home", "Projects", "Brackets"],
        "the breadcrumbs name every folder down to the remembered one"
    );
    assert_eq!(names(&state), ["bracket.stl"], "Brackets' rows show");
    assert_eq!(brackets.loads.get(), 1, "the folder shown is listed once");
    assert_eq!(
        brackets.activations.get(),
        1,
        "the folder shown is activated"
    );
    assert_eq!(projects.loads.get(), 0, "the folder above it is not listed");
    assert_eq!(projects.activations.get(), 0, "nor activated");
}

#[test]
fn going_back_up_reads_the_folder_then_and_once() {
    let (state, projects, _brackets) = restored();
    state.borrow_mut().go_to(1);
    assert_eq!(projects.loads.get(), 1, "Projects is listed as it shows");
    assert_eq!(projects.activations.get(), 1, "and activated");
    assert_eq!(
        names(&state),
        ["Brackets", "gear.stl"],
        "Projects' rows show"
    );
    // Down into Brackets and back up: Projects was read, so it shows as it stands.
    let link = state
        .borrow()
        .active_container()
        .expect("Projects shows")
        .child_containers()
        .into_iter()
        .next()
        .expect("Brackets' row");
    state.borrow_mut().open(link.as_ref());
    state.borrow_mut().go_to(1);
    assert_eq!(
        projects.loads.get(),
        1,
        "a folder already read is not listed again"
    );
    assert_eq!(
        projects.activations.get(),
        2,
        "it is activated as it shows again"
    );
}
