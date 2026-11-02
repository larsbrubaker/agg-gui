//! What a browser hands its hooks about what it shows: an [`Entry`] (one listed row: an item or a
//! folder row, with the path of the folder listing it) and a [`BrowserRef`] (a snapshot of the
//! browser: the open folder's path, place and writability, and the selection).
//!
//! Both are owned values, cheap to clone (shared `Rc`s), taken before a hook is called, so a hook
//! never runs while the browser's state is borrowed (the crate header's rule) and may keep them
//! as long as it likes. A `BrowserRef` is a snapshot only; a hook that needs to change the
//! browser does it through what its own closures captured. A handle asking the live browser for
//! more belongs here, borrowing the browser only for the length of each call.

use std::fmt;
use std::rc::Rc;

use agg_gui_files::{FolderLink, Item, PlaceId};

/// One listed row: an item, or a folder row (an item answering `as_container_link`), with the
/// names of the folders from the browser's root down to the one listing it.
#[derive(Clone)]
pub struct Entry {
    item: Rc<dyn Item>,
    folder_path: Rc<[String]>,
}

impl Entry {
    /// `item`, listed in the folder whose path from the root is `folder_path`.
    pub fn new(item: Rc<dyn Item>, folder_path: impl Into<Rc<[String]>>) -> Self {
        Self {
            item,
            folder_path: folder_path.into(),
        }
    }

    /// The folder row `link`, listed in the folder whose path from the root is `folder_path`.
    pub fn folder(link: Rc<dyn FolderLink>, folder_path: impl Into<Rc<[String]>>) -> Self {
        Self::new(link, folder_path)
    }

    /// The row's item (a folder row's link, seen as an item).
    pub fn item(&self) -> &Rc<dyn Item> {
        &self.item
    }

    /// The row's name.
    pub fn name(&self) -> &str {
        self.item.name()
    }

    /// The folder row's link, or `None` for an item.
    pub fn link(&self) -> Option<&dyn FolderLink> {
        self.item.as_container_link()
    }

    /// Whether the row opens a folder.
    pub fn is_folder(&self) -> bool {
        self.link().is_some()
    }

    /// The names of the folders from the root down to the one listing the row.
    pub fn folder_path(&self) -> &[String] {
        &self.folder_path
    }
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("name", &self.name())
            .field("is_folder", &self.is_folder())
            .field("folder_path", &self.folder_path)
            .finish()
    }
}

/// A snapshot of a browser, handed to the hooks that act on it (`BrowserActions`): the open
/// folder's path, its place and whether it takes changes, and the selected rows in the order they
/// were picked.
#[derive(Clone, Debug)]
pub struct BrowserRef {
    folder_path: Rc<[String]>,
    folder_place: Option<PlaceId>,
    folder_writable: bool,
    selection: Rc<[Entry]>,
}

impl BrowserRef {
    /// A browser showing the folder at `folder_path` with `selection` selected; the folder is in
    /// no place and takes no changes until said otherwise.
    pub fn new(folder_path: impl Into<Rc<[String]>>, selection: impl Into<Rc<[Entry]>>) -> Self {
        Self {
            folder_path: folder_path.into(),
            folder_place: None,
            folder_writable: false,
            selection: selection.into(),
        }
    }

    /// The same snapshot, its open folder in `place`.
    pub fn with_folder_place(mut self, place: Option<PlaceId>) -> Self {
        self.folder_place = place;
        self
    }

    /// The same snapshot, its open folder taking changes or not.
    pub fn with_folder_writable(mut self, writable: bool) -> Self {
        self.folder_writable = writable;
        self
    }

    /// The names of the folders from the root down to the open one.
    pub fn folder_path(&self) -> &[String] {
        &self.folder_path
    }

    /// The open folder's place.
    pub fn folder_place(&self) -> Option<&PlaceId> {
        self.folder_place.as_ref()
    }

    /// Whether the open folder takes changes (it answers `as_writable`).
    pub fn folder_writable(&self) -> bool {
        self.folder_writable
    }

    /// The selected rows, in the order they were picked.
    pub fn selection(&self) -> &[Entry] {
        &self.selection
    }
}
