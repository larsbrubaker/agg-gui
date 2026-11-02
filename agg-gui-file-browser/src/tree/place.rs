//! Which folder a tree row stands for, beyond its name. A folder that is somewhere (a folder on
//! disk is its path; a zip archive, or a directory inside one, is the archive file and that
//! directory; a memory folder is its ID) answers that as its location key
//! (`Folder::location_key`, `FolderLink::location_key`): the one thing about it that cannot be two
//! folders. [`BrowserState::path_levels`] gives each level of the browser's path its place, from
//! the folder; the tree model (`model.rs`) gives each row its place, from the link, and the two
//! are compared to find the row the list is showing. A name alone is not enough: a folder "Knife"
//! sits beside its archive "Knife.zip", whose folder is named "Knife" too.
//!
//! The same places key the folders the tree reopens after a re-list ([`TreeKey`]): a row with a
//! place is known by it, any other row (an app's built-in folders, a cloud folder; the root's
//! rows, which answer no location key) by its name, which is all those folders share between
//! listings. Places live only in memory; nothing saves them.

use std::rc::Rc;

use agg_gui_files::{Folder, FolderLink};

use crate::state::BrowserState;

/// Where a folder is, for the folders that are somewhere: its location key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TreePlace(String);

impl TreePlace {
    /// The place of a folder the browser shows, if it answers a location key.
    pub fn of_container(container: &dyn Folder) -> Option<Self> {
        container.location_key().map(Self)
    }

    /// The place of the folder a tree row's link opens, if it answers a location key.
    pub fn of_link(link: &dyn FolderLink) -> Option<Self> {
        link.location_key().map(Self)
    }

    /// The location key.
    pub fn key(&self) -> &str {
        &self.0
    }
}

/// One folder on a tree row's way down from the root, as the tree tells folders apart between
/// listings: by its place where it has one, else by its name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TreeKey {
    /// A folder that is somewhere.
    Place(TreePlace),
    /// Any other folder, by its name.
    Name(String),
}

/// The keys of `path`'s folders from the root down.
pub fn path_keys(path: &[Rc<dyn FolderLink>]) -> Vec<TreeKey> {
    path.iter()
        .map(|link| match TreePlace::of_link(link.as_ref()) {
            Some(place) => TreeKey::Place(place),
            None => TreeKey::Name(link.name().to_string()),
        })
        .collect()
}

/// A level of the browser's path below the root: the folder's name and its place, if it answers
/// one ([`BrowserState::path_levels`]).
pub type PathLevel = (String, Option<TreePlace>);

impl BrowserState {
    /// Below the root, each folder on the path: its name and, for a folder that is somewhere,
    /// its place (what the folder tree opens down to and draws selected, `TreeModel::expand_to`,
    /// `TreeModel::showing_row`).
    pub fn path_levels(&self) -> Vec<PathLevel> {
        self.path_containers()
            .skip(1)
            .map(|c| (c.name().to_string(), TreePlace::of_container(&*c)))
            .collect()
    }
}
