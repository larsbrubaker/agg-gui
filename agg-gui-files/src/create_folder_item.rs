//! The item a menu's "Create Folder..." (and a save dialog's new-folder button) hands a writable
//! folder's `add` (`writable`), which makes a folder of that name instead of copying content. A
//! folder recognizes it by its type, through `as_any`. Its ID only has to tell it apart from
//! every other item of this run: it is never stored.

use std::any::Any;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::item::Item;

/// Counts the items made in this run, for their IDs.
static NEXT_NUMBER: AtomicU64 = AtomicU64::new(1);

/// The item that asks a writable folder for a new folder.
#[derive(Debug, Clone)]
pub struct CreateFolderItem {
    id: String,
    name: String,
}

impl CreateFolderItem {
    /// An item asking for the folder `name`, with an ID no other item of this run has.
    pub fn new(name: impl Into<String>) -> Self {
        let number = NEXT_NUMBER.fetch_add(1, Ordering::Relaxed);
        Self {
            id: format!("create-folder-{number}"),
            name: name.into(),
        }
    }
}

impl Item for CreateFolderItem {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_protected(&self) -> bool {
        false
    }

    fn is_visible(&self) -> bool {
        true
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}
