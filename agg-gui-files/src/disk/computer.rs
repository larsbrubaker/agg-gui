//! [`ComputerFolder`]: the "Computer" folder. It lists Downloads, Desktop and Documents (each only
//! when the folder exists) and the computer's drives, each opening a [`DiskFolder`] with its own
//! default sort; a search across folders covers the user's folders only (`search_sections`):
//! walking a whole volume to answer a search would take minutes and bury the hits in system
//! files.
//!
//! The app says where its user's folders are ([`ComputerFolders`]); this crate does not know an
//! operating system's folder conventions. The drives are read with std alone ([`drive_roots`]):
//! the drive letters that exist on Windows, the root `/` elsewhere. Pure std has no mount-table
//! reader, the system mount points are not places a user keeps files, and `/` reaches every one
//! of them anyway. An app shows this folder natively only; a browser has no disk.

use std::path::PathBuf;
use std::rc::Rc;

use super::folder::DiskFolder;
use super::quick_look::folder_has_child_containers;
use super::rules::DiskRules;
use crate::dynamic_folder_link::DynamicFolderLink;
use crate::folder::Folder;
use crate::folder_data::FolderData;
use crate::item::{FolderLink, Item};
use crate::sort::{SortBehavior, SortKey};
use crate::translate::tr;

/// The computer's drive roots (see the file header).
pub fn drive_roots() -> Vec<PathBuf> {
    if cfg!(windows) {
        (b'A'..=b'Z')
            .map(|letter| PathBuf::from(format!("{}:\\", letter as char)))
            .filter(|root| root.is_dir())
            .collect()
    } else {
        vec![PathBuf::from("/")]
    }
}

/// The folders "Computer" lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComputerFolders {
    pub downloads: PathBuf,
    pub desktop: PathBuf,
    pub documents: PathBuf,
    /// Each drive's name as its row reads, and the folder it opens.
    pub drives: Vec<(String, PathBuf)>,
}

impl ComputerFolders {
    /// The user's Downloads, Desktop and Documents, with this computer's drives
    /// ([`drive_roots`]), each named by its path.
    pub fn with_this_computers_drives(
        downloads: PathBuf,
        desktop: PathBuf,
        documents: PathBuf,
    ) -> Self {
        Self {
            downloads,
            desktop,
            documents,
            drives: drive_roots()
                .into_iter()
                .map(|drive| (drive.to_string_lossy().into_owned(), drive))
                .collect(),
        }
    }
}

/// A row named `name` opening the folder `path` (incremented names during a type change), drawn
/// with the micro icon `glyph`, whose folder sorts by `sort` when it shows. A drive's root is
/// taken to have folders without reading it (a sleeping or removable drive would stall the tree
/// to ask); a user folder is given a quick look at its names.
fn folder_row(
    name: String,
    path: PathBuf,
    glyph: &str,
    sort: SortBehavior,
    is_drive: bool,
    rules: &Rc<dyn DiskRules>,
) -> Rc<dyn FolderLink> {
    let scanned = path.clone();
    let rules = Rc::clone(rules);
    let link = DynamicFolderLink::new(
        name,
        Rc::new(move || {
            Box::new(
                DiskFolder::new(&path, Rc::clone(&rules))
                    .with_incremented_name_during_type_change(true)
                    .with_default_sort(sort),
            ) as Box<dyn Folder>
        }),
    )
    .with_micro_icon(glyph);
    Rc::new(if is_drive {
        link.with_child_containers(|| Some(true))
    } else {
        link.with_child_containers(move || folder_has_child_containers(&scanned))
    })
}

/// Name, A to Z.
fn name_a_to_z() -> SortBehavior {
    SortBehavior::new(SortKey::Name, true)
}

/// The "Computer" folder.
pub struct ComputerFolder {
    data: FolderData,
    /// Downloads, Desktop and Documents, what a search across folders covers here.
    search_sections: Vec<Rc<dyn FolderLink>>,
}

impl ComputerFolder {
    /// Named "Computer", listing the user's folders in `folders` that exist and its drives, each
    /// opening a folder that follows `rules`.
    pub fn new(folders: ComputerFolders, rules: Rc<dyn DiskRules>) -> Self {
        let mut data = FolderData::new(tr("Computer"));
        if folders.downloads.is_dir() {
            data.child_containers.push(folder_row(
                tr("Downloads"),
                folders.downloads,
                "downloads.svg",
                // Downloads is the exception because Windows Explorer itself lists Downloads
                // newest first, so recent downloads are on top.
                SortBehavior::new(SortKey::ModifiedDate, false),
                false,
                &rules,
            ));
        }
        if folders.desktop.is_dir() {
            data.child_containers.push(folder_row(
                tr("Desktop"),
                folders.desktop,
                "desktop.svg",
                name_a_to_z(),
                false,
                &rules,
            ));
        }
        if folders.documents.is_dir() {
            data.child_containers.push(folder_row(
                tr("Documents"),
                folders.documents,
                "documents.svg",
                name_a_to_z(),
                false,
                &rules,
            ));
        }
        let search_sections = data.child_containers.clone();
        for (name, drive) in folders.drives {
            data.child_containers.push(folder_row(
                name,
                drive,
                "drive.svg",
                name_a_to_z(),
                true,
                &rules,
            ));
        }
        Self {
            data,
            search_sections,
        }
    }
}

impl Folder for ComputerFolder {
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

    /// Nothing to do: the rows are made with the folder.
    fn load(&mut self) {}

    /// The user's folders, never the drives.
    fn search_sections(&self) -> Option<Vec<Rc<dyn FolderLink>>> {
        Some(self.search_sections.clone())
    }

    /// An app that remembers and restores "Computer" by its type reaches it as itself.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
