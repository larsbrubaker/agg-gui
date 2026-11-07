//! [`DynamicFolderLink`]: a folder row whose folder is made when it is opened, the row an app's
//! top level is built from. Every such row shows the plain folder tile, so only the micro icon is
//! carried (`with_micro_icon`, a key the app's look resolves). The ID is empty unless set, and
//! the link is protected and not read-only by default.
//!
//! A creator may have nothing to give (`new_optional`): opening the link then fails, and a
//! user's open runs the link's `with_when_unavailable` action instead (a sign-in, say), which a
//! search's walk never does, since a search the user typed must not open a dialog they did not
//! ask for. Whether the folder has folders of its own, for a folder tree's arrow, is unknown
//! unless the maker says how to answer it cheaply (`with_child_containers`,
//! `with_local_listing`), so a link listed over the network never has its folder made just to
//! ask.

use std::rc::Rc;

use crate::folder::Folder;
use crate::item::{FolderLink, Item};
use crate::place::PlaceId;
use crate::translate::tr;

/// Makes the folder a link opens.
pub type FolderCreator = Rc<dyn Fn() -> Box<dyn Folder>>;

/// Makes the folder a link opens, or has none to give.
pub type OptionalFolderCreator = Rc<dyn Fn() -> Option<Box<dyn Folder>>>;

/// What `get_container` answers, in English, for a creator with nothing to give (translated
/// when answered).
pub const NO_FOLDER: &str = "Could not be opened";

/// A folder row whose folder is made when it is opened.
#[derive(Clone)]
pub struct DynamicFolderLink {
    id: String,
    name: String,
    creator: OptionalFolderCreator,
    when_unavailable: Option<Rc<dyn Fn()>>,
    visibility: Option<Rc<dyn Fn() -> bool>>,
    is_read_only: bool,
    /// A read-only folder holding folders that can take items (`with_holds_destinations`,
    /// `with_holds_destinations_when`), asked each time.
    holds_destinations: Rc<dyn Fn() -> bool>,
    micro_icon: Option<String>,
    /// How to tell cheaply whether the folder has folders (`with_child_containers`).
    child_containers: Option<Rc<dyn Fn() -> Option<bool>>>,
}

impl DynamicFolderLink {
    /// A link named `name` whose folder `creator` makes.
    pub fn new(name: impl Into<String>, creator: FolderCreator) -> Self {
        Self::new_optional(name, Rc::new(move || Some(creator())))
    }

    /// A link whose creator may have nothing to give.
    pub fn new_optional(name: impl Into<String>, creator: OptionalFolderCreator) -> Self {
        Self {
            id: String::new(),
            name: name.into(),
            creator,
            when_unavailable: None,
            visibility: None,
            is_read_only: false,
            holds_destinations: Rc::new(|| false),
            micro_icon: None,
            child_containers: None,
        }
    }

    /// How `has_child_containers` answers: `answer` must not touch the network, slow storage or
    /// the user (a known answer, or a quick look at a local folder's names).
    pub fn with_child_containers(mut self, answer: impl Fn() -> Option<bool> + 'static) -> Self {
        self.child_containers = Some(Rc::new(answer));
        self
    }

    /// `has_child_containers` makes the folder and asks it: only for a creator with no side
    /// effects whose folder lists in memory or answers from local disk quickly.
    pub fn with_local_listing(self) -> Self {
        let creator = Rc::clone(&self.creator);
        self.with_child_containers(move || creator()?.has_child_containers())
    }

    /// The link with the ID `id`.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// The link, read-only or not.
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.is_read_only = read_only;
        self
    }

    /// The link, holding folders that can take items though it is read-only itself (folders
    /// shared with the user) or not.
    pub fn with_holds_destinations(self, holds: bool) -> Self {
        self.with_holds_destinations_when(move || holds)
    }

    /// The link holds folders that can take items while `holds` answers true (folders the user
    /// may save into only while signed in).
    pub fn with_holds_destinations_when(mut self, holds: impl Fn() -> bool + 'static) -> Self {
        self.holds_destinations = Rc::new(holds);
        self
    }

    /// The icon key of the glyph drawn on the folder tile.
    pub fn with_micro_icon(mut self, icon: impl Into<String>) -> Self {
        self.micro_icon = Some(icon.into());
        self
    }

    /// What a user's open does when the creator had nothing to give (a sign-in).
    pub fn with_when_unavailable(mut self, action: impl Fn() + 'static) -> Self {
        self.when_unavailable = Some(Rc::new(action));
        self
    }

    /// The link shows only while `visible` answers true.
    pub fn with_visibility(mut self, visible: impl Fn() -> bool + 'static) -> Self {
        self.visibility = Some(Rc::new(visible));
        self
    }
}

impl Item for DynamicFolderLink {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_protected(&self) -> bool {
        true
    }

    fn is_visible(&self) -> bool {
        self.visibility.as_ref().is_none_or(|v| v())
    }

    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        Some(self)
    }

    fn into_container_link(self: Rc<Self>) -> Option<Rc<dyn FolderLink>> {
        Some(self)
    }

    /// The concrete link, so a drag can tell a top-level row from other rows and open it to
    /// learn where its folder lives.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

impl FolderLink for DynamicFolderLink {
    fn get_container(&self) -> Result<Box<dyn Folder>, String> {
        // The literal is [`NO_FOLDER`], written out so `ENGLISH_TEXTS`' check finds it.
        (self.creator)().ok_or_else(|| tr("Could not be opened"))
    }

    fn open_unavailable(&self) -> bool {
        match &self.when_unavailable {
            Some(action) => {
                action();
                true
            }
            None => false,
        }
    }

    fn is_read_only(&self) -> bool {
        self.is_read_only
    }

    fn holds_destinations(&self) -> bool {
        (self.holds_destinations)()
    }

    fn micro_icon(&self) -> Option<&str> {
        self.micro_icon.as_deref()
    }

    fn has_child_containers(&self) -> Option<bool> {
        self.child_containers.as_ref().and_then(|answer| answer())
    }

    /// The place of the folder the creator makes. Each call makes that folder once (unloaded,
    /// so it lists nothing and reads no disk or network), which costs what the creator costs: a
    /// caller asking often keeps the answer. A top-level row answers no location key: a folder
    /// tree knows it by name.
    fn place(&self) -> Option<PlaceId> {
        self.get_container().ok().and_then(|folder| folder.place())
    }
}
