//! A folder that lists later, as a server's does: its `load` only starts the listing, it says
//! its load is pending until the test lands the listing (or a failure), and landing raises
//! content-changed, as the folder trait asks of such a folder. `LateLink` is a row opening it.
//! Shared by the `folder_listing` and `disk` test binaries.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui_files::{
    ContentChangedEvent, ContentChangedListener, Folder, FolderLink, Item, ListingFailure,
};

/// What the test drives: the listing to land, and what the folder was asked.
#[derive(Default)]
pub struct LateState {
    landed: Cell<bool>,
    failure: Cell<Option<ListingFailure>>,
    items: RefCell<Vec<Rc<dyn Item>>>,
    event: ContentChangedEvent,
    pub loads: Cell<usize>,
    pub listeners: Cell<isize>,
}

impl LateState {
    /// The listing lands with `items`, and content-changed is raised.
    pub fn land(&self, items: Vec<Rc<dyn Item>>) {
        *self.items.borrow_mut() = items;
        self.landed.set(true);
        self.event.raise();
    }

    /// The listing fails with `failure`, and content-changed is raised.
    pub fn fail(&self, failure: ListingFailure) {
        self.failure.set(Some(failure));
        self.landed.set(true);
        self.event.raise();
    }
}

/// The folder, sharing its state with the test.
#[derive(Clone)]
pub struct LateFolder {
    name: String,
    pub state: Rc<LateState>,
}

impl LateFolder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            state: Rc::new(LateState::default()),
        }
    }
}

impl Folder for LateFolder {
    fn id(&self) -> &str {
        &self.name
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.state.items.borrow().clone()
    }
    fn load(&mut self) {
        self.state.loads.set(self.state.loads.get() + 1);
    }
    fn is_load_pending(&self) -> bool {
        !self.state.landed.get()
    }
    fn last_load_failed(&self) -> bool {
        self.state.failure.get().is_some()
    }
    fn listing_failure(&self) -> Option<ListingFailure> {
        self.state.failure.get()
    }
    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        self.state.listeners.set(self.state.listeners.get() + 1);
        self.state.event.subscribe(listener);
    }
    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        self.state.listeners.set(self.state.listeners.get() - 1);
        self.state.event.unsubscribe(listener);
    }
}

/// A row opening `folder`.
pub struct LateLink(pub LateFolder);

impl Item for LateLink {
    fn id(&self) -> &str {
        &self.0.name
    }
    fn name(&self) -> &str {
        &self.0.name
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl FolderLink for LateLink {
    fn get_container(&self) -> Result<Box<dyn Folder>, String> {
        Ok(Box::new(self.0.clone()))
    }
    fn is_read_only(&self) -> bool {
        true
    }
}
