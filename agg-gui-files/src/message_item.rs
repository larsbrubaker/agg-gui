//! [`MessageItem`]: a row that only shows a message (a folder that could not be read, say). Its
//! ID is empty, it is not protected and it is visible. [`MessageItem::loading`] is the
//! "Loading..." row a folder that fills itself in later shows until its listing arrives, and
//! [`MessageItem::missing_file`] the `Missing-<file>` row for content that could not be opened
//! (a zip archive, say). Both are this one type, so every check for a message row (a downcast
//! through `as_any`, as the search walk's placeholder test in `search_walk` does) sees either,
//! and `is_missing_file` tells them apart. No picture is carried: the browser draws its
//! placeholder, and `thumbnail_icon` names the icon a missing-file row asks for, as a key the
//! app's look resolves.

use std::any::Any;

use crate::item::Item;
use crate::translate::tr;

/// The icon key a missing-file row shows as its picture.
const MISSING_FILE_ICON: &str = "part_icon_transparent_40x40.png";

/// A row that only shows a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageItem {
    name: String,
    missing_file: bool,
}

impl MessageItem {
    /// A message row reading `name` (already translated).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            missing_file: false,
        }
    }

    /// `Missing-<name>`, with the transparent part icon.
    pub fn missing_file(name: &str) -> Self {
        Self {
            name: format!("{}-{name}", tr("Missing")),
            missing_file: true,
        }
    }

    /// Whether this is a missing-file row.
    pub fn is_missing_file(&self) -> bool {
        self.missing_file
    }

    /// The icon key the row shows as its picture, if any.
    pub fn thumbnail_icon(&self) -> Option<&'static str> {
        self.missing_file.then_some(MISSING_FILE_ICON)
    }

    /// "Loading..." (translated): the placeholder row of a folder whose listing has not arrived
    /// yet.
    pub fn loading() -> Self {
        Self::new(format!("{}...", tr("Loading")))
    }
}

impl Item for MessageItem {
    fn id(&self) -> &str {
        ""
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
