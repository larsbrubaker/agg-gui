//! [`ZipFolder`]: a `.zip` on disk as a folder. `load` lists one directory level of the archive,
//! `relative_directory` deep: the entries directly in it become [`ZipEntry`]s (in archive order)
//! and the first segment of every deeper entry becomes a [`ZipFolderLink`] one level further in
//! (in the order they are first seen; `indexmap` keeps both orders). The folder is named for the
//! archive without its extension. A missing archive lists one missing-file row
//! (`MessageItem::missing_file`, which every message-row check sees).
//!
//! An archive that cannot be read (corrupt, or not a zip) lists one error row saying why, as a
//! folder that cannot be read does, rather than failing the listing; a duplicated entry name
//! keeps its first entry. A directory is only matched at a separator boundary, so a sibling
//! folder whose name merely begins with this one's ("Parts2" beside "Parts") is not listed inside
//! it. Content-changed is never raised (the archive is read-only).

use std::any::Any;
use std::path::Path;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};

use super::entry::ZipEntry;
use super::link::ZipFolderLink;
use super::zip_location_key;
use crate::folder::Folder;
use crate::item::{FolderLink, Item};
use crate::message_item::MessageItem;

/// The separators an archive's entry names may use.
const PATH_SEPARATORS: [char; 2] = ['/', '\\'];

/// A `.zip` on disk as a folder.
#[derive(Default)]
pub struct ZipFolder {
    /// The archive on disk.
    pub path: String,
    /// The directory inside the archive this container lists ("" for its
    /// top level).
    pub relative_directory: String,
    name: String,
    child_containers: Vec<Rc<dyn FolderLink>>,
    items: Vec<Rc<dyn Item>>,
}

impl ZipFolder {
    /// The container for `relative_directory` in the archive at `path`.
    pub fn new(path: impl Into<String>, relative_directory: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            relative_directory: relative_directory.into(),
            ..Self::default()
        }
    }

    /// A zip can hold editable content that is kept as a tab.
    pub fn can_persist_as_tab(&self) -> bool {
        true
    }

    fn show_message(&mut self, item: MessageItem) {
        self.child_containers = Vec::new();
        self.items = vec![Rc::new(item)];
    }

    /// The entries one level inside `relative_directory`: the files directly in it (name to
    /// uncompressed size), the directories below it, and the separator it is written with.
    fn list(&self) -> Result<Listing, String> {
        let file = std::fs::File::open(&self.path).map_err(|e| e.to_string())?;
        let mut archive = ::zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let relative = self.relative_directory.as_str();
        let mut listing = Listing::default();
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index).map_err(|e| e.to_string())?;
            let full_name = entry.name().to_string();
            let Some(rest) = full_name.strip_prefix(relative) else {
                continue;
            };
            // Only at a separator: "Parts2/x" is not inside "Parts".
            if !relative.is_empty() && !rest.is_empty() && !rest.starts_with(PATH_SEPARATORS) {
                continue;
            }
            let remaining_path = rest.trim().trim_start_matches(PATH_SEPARATORS);
            if listing.separator.is_none() && !relative.is_empty() {
                listing.separator = rest.chars().next().map(String::from);
            }
            let segments: Vec<&str> = remaining_path.split(PATH_SEPARATORS).collect();
            let first_directory = segments[0];
            let last_segment = segments[segments.len() - 1];
            if !last_segment.is_empty() && segments.len() == 1 {
                // The entry's name, the last segment of the full name. A repeated name keeps its
                // first entry.
                let name = full_name
                    .rsplit(PATH_SEPARATORS)
                    .next()
                    .unwrap_or(&full_name);
                listing
                    .items
                    .entry(name.to_string())
                    .or_insert(entry.size());
            } else if !remaining_path.is_empty() {
                listing.directories.insert(first_directory.to_string());
            }
        }
        Ok(listing)
    }

    /// `name` one level inside this container's directory, as the archive writes it.
    fn inside(&self, name: &str, separator: &str) -> String {
        if self.relative_directory.is_empty() {
            name.to_string()
        } else {
            format!("{}{separator}{name}", self.relative_directory)
        }
    }
}

/// What one directory level of an archive holds.
#[derive(Default)]
struct Listing {
    items: IndexMap<String, u64>,
    directories: IndexSet<String>,
    separator: Option<String>,
}

impl Folder for ZipFolder {
    /// The ID is never set.
    fn id(&self) -> &str {
        ""
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.child_containers.clone()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.items.clone()
    }
    /// Lists the archive's level at `relative_directory`.
    fn load(&mut self) {
        let path = Path::new(&self.path);
        self.name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !path.is_file() {
            let file_name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.show_message(MessageItem::missing_file(&file_name));
            return;
        }
        let listing = match self.list() {
            Ok(listing) => listing,
            Err(e) => {
                // An unreadable archive lists its error row.
                self.show_message(MessageItem::new(format!("Error loading container - {e}")));
                return;
            }
        };
        let separator = listing.separator.unwrap_or_default();
        self.child_containers = listing
            .directories
            .iter()
            .filter(|d| !d.is_empty())
            .map(|d| {
                let mut link = ZipFolderLink::new(&self.path, None);
                link.set_current_directory(self.inside(d, &separator));
                Rc::new(link) as Rc<dyn FolderLink>
            })
            .collect();
        self.items = listing
            .items
            .iter()
            .map(|(name, size)| {
                Rc::new(ZipEntry::new(
                    &self.path,
                    self.inside(name, &separator),
                    *size,
                )) as Rc<dyn Item>
            })
            .collect();
    }
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
    /// The archive and the directory inside it ([`zip_location_key`]), as the row that opens it
    /// answers. It answers no place: it keeps its items, so they leave it only as copies.
    fn location_key(&self) -> Option<String> {
        Some(zip_location_key(
            Path::new(&self.path),
            &self.relative_directory,
        ))
    }
}
