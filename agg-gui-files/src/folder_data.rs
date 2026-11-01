//! [`FolderData`]: the state every plain folder shares (ID, name, protection, child folder links
//! and items). A folder type holds one and forwards the [`crate::Folder`] getters to it; its own
//! `load` fills the lists. [`crate::DynamicFolder`] is the folder that is nothing more.

use std::rc::Rc;

use crate::item::{FolderLink, Item};

/// A folder's data.
pub struct FolderData {
    /// The folder's ID, empty unless set.
    pub id: String,
    /// The folder's name.
    pub name: String,
    /// True unless the folder says otherwise.
    pub is_protected: bool,
    /// The links to child folders, in listing order.
    pub child_containers: Vec<Rc<dyn FolderLink>>,
    /// The items, in listing order.
    pub items: Vec<Rc<dyn Item>>,
}

impl FolderData {
    /// A folder named `name`, empty and protected.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            name: name.into(),
            is_protected: true,
            child_containers: Vec::new(),
            items: Vec::new(),
        }
    }
}
