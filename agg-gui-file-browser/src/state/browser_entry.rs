//! One row of a browser's listing ([`BrowserEntry`]: a folder row or an item; an item that is a
//! folder's link, as a search reports a folder it found, is a folder row too) and the identity a
//! selection is matched back by when the rows are made again ([`item_identity_key`]). A row is
//! compared by object (`same_row`), so a re-sort can never leave the selection on another item;
//! the identity key is for the times the folder hands back new objects for the same content (a
//! search's next batch, a folder listing itself again).

use std::rc::Rc;

use agg_gui_files::{Asset, FolderLink, Item};

use crate::entry::Entry;
use crate::look::BrowserNames;

/// One row of the listing: a folder or an item.
#[derive(Clone)]
pub enum BrowserEntry {
    /// A row that opens a folder.
    Container(Rc<dyn FolderLink>),
    /// Any other item.
    Item(Rc<dyn Item>),
}

impl BrowserEntry {
    /// The line under the row's name, when the row has one.
    pub fn detail_line(&self) -> Option<String> {
        self.item().detail_line()
    }

    /// The row's name.
    pub fn name(&self) -> &str {
        match self {
            BrowserEntry::Container(link) => link.name(),
            BrowserEntry::Item(item) => item.name(),
        }
    }

    /// The row's widget name in `names` (a folder row's, or an item row's).
    pub fn row_name(&self, names: &BrowserNames) -> String {
        if self.is_container() {
            names.folder_row_for(self.name())
        } else {
            names.item_row_for(self.name())
        }
    }

    /// Whether the row opens a folder: a folder row, or an item that is a folder's link (a
    /// folder a search found by its name).
    pub fn is_container(&self) -> bool {
        self.container_link().is_some()
    }

    /// The link the row opens, for either kind of folder row; `None` for any other item.
    pub fn container_link(&self) -> Option<&dyn FolderLink> {
        match self {
            BrowserEntry::Container(link) => Some(link.as_ref()),
            BrowserEntry::Item(item) => item.as_container_link(),
        }
    }

    /// The row's item (a folder row's link, seen as an item).
    pub fn item(&self) -> Rc<dyn Item> {
        match self {
            BrowserEntry::Container(link) => Rc::clone(link) as Rc<dyn Item>,
            BrowserEntry::Item(item) => Rc::clone(item),
        }
    }

    /// The row as hooks are handed it, listed in the folder at `folder_path`.
    pub fn to_entry(&self, folder_path: Rc<[String]>) -> Entry {
        Entry::new(self.item(), folder_path)
    }

    /// Whether `self` and `other` are the same row's object (not equal content).
    pub fn same_row(&self, other: &BrowserEntry) -> bool {
        match (self, other) {
            (BrowserEntry::Container(a), BrowserEntry::Container(b)) => Rc::ptr_eq(a, b),
            (BrowserEntry::Item(a), BrowserEntry::Item(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// [`item_identity_key`] of the row's item.
    pub fn identity_key(&self) -> String {
        match self {
            BrowserEntry::Container(link) => item_identity_key(link.as_ref()),
            BrowserEntry::Item(item) => item_identity_key(item.as_ref()),
        }
    }
}

/// A stable identity for an item: its ID, else its asset path (content that carries one), else
/// its name. Not the item itself, because a folder that lists itself again hands back new objects
/// for the same content. A stream is content even when its item does not answer `as_asset`, so a
/// key never depends on that answer being there.
pub fn item_identity_key(item: &dyn Item) -> String {
    if !item.id().trim().is_empty() {
        return item.id().to_string();
    }
    let asset_path = item
        .as_asset()
        .or_else(|| item.as_asset_stream().map(|stream| stream as &dyn Asset))
        .map(|asset| asset.asset_path());
    if let Some(path) = asset_path.filter(|path| !path.trim().is_empty()) {
        return path.to_string();
    }
    item.name().to_string()
}
