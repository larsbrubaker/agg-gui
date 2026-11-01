//! [`DynamicFolder`]: a folder filled by a callback when it loads, with header markdown of its
//! own to set. Its state is a [`FolderData`] (`folder_data`); a [`crate::DynamicFolderLink`]
//! usually makes it.

use std::rc::Rc;

use crate::folder::Folder;
use crate::folder_data::FolderData;
use crate::item::{FolderLink, Item};

/// Fills the folder when it loads.
pub type FolderInitializer = Rc<dyn Fn(&mut FolderData)>;

/// A folder filled by a callback when it loads.
pub struct DynamicFolder {
    /// The folder's ID, name, protection and lists.
    pub data: FolderData,
    /// Markdown shown above the rows; empty for none.
    pub header_markdown: String,
    /// Run on each `load` to fill `data`; `None` leaves the folder as it is.
    pub initializer: Option<FolderInitializer>,
}

impl DynamicFolder {
    /// An empty folder called `name`, with no initializer.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            data: FolderData::new(name),
            header_markdown: String::new(),
            initializer: None,
        }
    }

    /// The folder with `markdown` shown above its rows.
    pub fn with_header_markdown(mut self, markdown: impl Into<String>) -> Self {
        self.header_markdown = markdown.into();
        self
    }
}

impl Folder for DynamicFolder {
    fn id(&self) -> &str {
        &self.data.id
    }

    fn name(&self) -> &str {
        &self.data.name
    }

    fn is_protected(&self) -> bool {
        self.data.is_protected
    }

    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.data.child_containers.clone()
    }

    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.data.items.clone()
    }

    fn header_markdown(&self) -> String {
        self.header_markdown.clone()
    }

    /// Runs the initializer, which fills the folder's data.
    fn load(&mut self) {
        if let Some(initializer) = self.initializer.clone() {
            initializer(&mut self.data);
        }
    }
}
