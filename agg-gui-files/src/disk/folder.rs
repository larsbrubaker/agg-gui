//! [`DiskFolder`]: a folder on disk as a [`Folder`] - what it is, and its listing. `load` lists
//! the folder's sub-folders as [`DiskFolderLink`]s and, with the `zip` feature, its zip files as
//! `zip_folder::ZipFolderLink`s (sorted together by name), and the files its
//! [`DiskRules`] accept as [`DiskFile`]s (sorted by path), reads an `index.md` into
//! `header_markdown` (shown above the rows), and lists a single message row for a folder that is
//! missing, restricted or unreadable, so the user sees why it is empty. Content-changed is raised
//! by `reload_content` (after a write), never by `load`. The writes are `folder_writes`.
//!
//! While it is the showing folder (`activate` to `deactivate`/`dispose`) it watches its folder
//! and relists itself after an outside change (`live_refresh`, driven by the browser's
//! `pump_live_refresh`); its own writes turn the watch off while they run. It answers the folder
//! tree's arrow from its names alone (`quick_look`) and searches itself (`search`).
//!
//! A folder on disk is on the computer ([`PlaceId::COMPUTER`]) and is known by its path
//! (`paths::disk_location_key`), as its row answers both.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::file::DiskFile;
use super::folder_link::DiskFolderLink;
use super::live_refresh::{LiveRefresh, RefreshInterval};
use super::paths::{disk_location_key, path_file_name};
use super::quick_look::folder_has_child_containers;
use super::rules::DiskRules;
use crate::folder::{ContentChangedEvent, ContentChangedListener, Folder};
use crate::item::{FolderLink, Item};
use crate::message_item::MessageItem;
use crate::place::PlaceId;
use crate::search::SearchProvider;
use crate::sort::{culture_compare, SortBehavior};
use crate::writable::WritableFolder;

/// System folders that only produce permission errors, so they are never listed (ignoring case).
const RESTRICTED_PATTERNS: &[&str] = &[
    "\\$Recycle.Bin",
    "\\System Volume Information",
    "\\Recovery",
    "\\ProgramData\\Microsoft\\Windows\\WER",
];

/// Whether `directory_path` may be read: false for the Windows system folders above.
pub(crate) fn is_readable_directory(directory_path: &Path) -> bool {
    let path = directory_path.to_string_lossy().trim().to_lowercase();
    !RESTRICTED_PATTERNS
        .iter()
        .any(|pattern| path.contains(&pattern.to_lowercase()))
}

/// A folder on disk. A clone is the folder as it stands (name, sort, listing).
#[derive(Clone)]
pub struct DiskFolder {
    pub(super) full_path: PathBuf,
    id: String,
    name: String,
    child_containers: Vec<Rc<dyn FolderLink>>,
    items: Vec<Rc<dyn Item>>,
    header_markdown: Option<String>,
    use_incremented_name_during_type_change: bool,
    default_sort: Option<SortBehavior>,
    /// Which files list, and whether the folder may change.
    pub(super) rules: Rc<dyn DiskRules>,
    /// Raised by `reload_content` (a write's relist), never by `load`.
    content_changed: ContentChangedEvent,
    /// Items listed after the folder's own files that are not in the folder (`with_extra_items`).
    extra_items: Vec<Rc<dyn Item>>,
    /// The directory watch and its pending refresh while this is the showing folder.
    pub(super) live: LiveRefresh,
}

impl DiskFolder {
    /// The folder at `full_path`, following `rules`: named for the folder, not protected, empty
    /// until loaded.
    pub fn new(full_path: impl Into<PathBuf>, rules: Rc<dyn DiskRules>) -> Self {
        let full_path = full_path.into();
        Self {
            name: path_file_name(&full_path),
            id: String::new(),
            full_path,
            child_containers: Vec::new(),
            items: Vec::new(),
            header_markdown: None,
            use_incremented_name_during_type_change: false,
            default_sort: None,
            rules,
            content_changed: ContentChangedEvent::default(),
            extra_items: Vec::new(),
            live: LiveRefresh::default(),
        }
    }

    /// Lists `items` after the folder's own files at every listing, though they are not in the
    /// folder (an app's built-in tools on a shelf). They should be protected, so the user can't
    /// rename or remove what the folder does not hold.
    pub fn with_extra_items(mut self, items: Vec<Rc<dyn Item>>) -> Self {
        self.extra_items = items;
        self
    }

    /// Sets the default sort.
    pub fn with_default_sort(mut self, sort: SortBehavior) -> Self {
        self.default_sort = Some(sort);
        self
    }

    /// Sets the name (the link that opened it names it).
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets `use_incremented_name_during_type_change`.
    pub fn with_incremented_name_during_type_change(mut self, value: bool) -> Self {
        self.use_incremented_name_during_type_change = value;
        self
    }

    /// Whether a save that changes a file's type numbers the new name rather than replacing a
    /// file of that name.
    pub fn use_incremented_name_during_type_change(&self) -> bool {
        self.use_incremented_name_during_type_change
    }

    /// The folder's full path.
    pub fn full_path(&self) -> &Path {
        &self.full_path
    }

    /// The rules the folder follows.
    pub fn rules(&self) -> &Rc<dyn DiskRules> {
        &self.rules
    }

    /// The watch reported a change (marshalled to this thread). While active it arms the
    /// coalescing refresh, which relists once half a second has passed with no further change.
    pub fn directory_contents_changed(&mut self) {
        self.live.contents_changed();
    }

    /// The armed refresh interval, if a change is waiting to relist.
    pub fn pending_refresh(&self) -> Option<RefreshInterval> {
        self.live.pending_refresh()
    }

    /// Stops watching and cancels a pending refresh; a disposed folder is not active, so a late
    /// report cannot arm a new one.
    pub fn dispose(&mut self) {
        self.live.teardown();
    }

    /// Runs `write` with the watch off, so the folder's own writes are not reported back to it,
    /// and turns it back on afterwards whatever happened.
    pub(super) fn unwatched<T>(&mut self, write: impl FnOnce(&mut Self) -> T) -> T {
        self.live.set_watch_enabled(false);
        let result = write(self);
        self.live.set_watch_enabled(true);
        result
    }

    fn show_message(&mut self, message: String) {
        self.child_containers = Vec::new();
        self.items = vec![Rc::new(MessageItem::new(message))];
    }

    /// Lists the folder (and, when `recursive`, every folder below it).
    pub fn load_recursive(&mut self, recursive: bool) {
        // Each listing reads the header afresh, so a removed `index.md` takes its header with it
        // rather than describing a file that is no longer there.
        self.header_markdown = None;
        if !is_readable_directory(&self.full_path) {
            let name = path_file_name(&self.full_path);
            self.show_message(format!("Cannot access directory: {name} (restricted)"));
            return;
        }
        if !self.full_path.is_dir() {
            let name = path_file_name(&self.full_path);
            self.show_message(format!("Missing directory: {name}"));
            return;
        }
        let (mut files, mut directories) = (Vec::new(), Vec::new());
        if let Err(e) = list(&self.full_path, recursive, &mut files, &mut directories) {
            self.show_message(format!("Error loading container - {e}"));
            return;
        }
        // Zip files list as folders after the directories (with the `zip` feature); the name sort
        // below orders them together.
        let (zip_links, files) = zip_folder_links(files);
        let mut links: Vec<Rc<dyn FolderLink>> = directories
            .into_iter()
            .map(|d| Rc::new(DiskFolderLink::new(d, self.rules.clone())) as Rc<dyn FolderLink>)
            .chain(zip_links)
            .collect();
        links.sort_by(|a, b| culture_compare(a.name(), b.name()));
        self.child_containers = links;
        let mut matched: Vec<PathBuf> = files
            .iter()
            .filter(|f| self.rules.lists_file(&path_file_name(f)))
            .cloned()
            .collect();
        matched.sort_by(|a, b| culture_compare(&a.to_string_lossy(), &b.to_string_lossy()));
        self.items = matched
            .into_iter()
            .map(|f| Rc::new(DiskFile::new(f)) as Rc<dyn Item>)
            .chain(self.extra_items.iter().cloned())
            .collect();
        if let Some(index_md) = files
            .iter()
            .find(|f| f.to_string_lossy().ends_with("index.md"))
        {
            if let Ok(text) = std::fs::read_to_string(index_md) {
                self.header_markdown = Some(text);
            }
        }
    }

    /// Lists the folder again, then raises content-changed.
    pub fn reload_content(&mut self) {
        self.load_recursive(false);
        self.content_changed.raise();
    }
}

/// The folder rows of the zip files among `files` ([`crate::zip_folder::ZipFolderLink`]), and the
/// rest of the files.
#[cfg(feature = "zip")]
fn zip_folder_links(files: Vec<PathBuf>) -> (Vec<Rc<dyn FolderLink>>, Vec<PathBuf>) {
    use crate::zip_folder::ZipFolderLink;
    let (zips, rest): (Vec<PathBuf>, Vec<PathBuf>) = files
        .into_iter()
        .partition(|f| super::paths::lists_as_folder(f));
    let links = zips
        .into_iter()
        .map(|f| Rc::new(ZipFolderLink::new(f, None)) as Rc<dyn FolderLink>)
        .collect();
    (links, rest)
}

/// Without the `zip` feature a zip file is a file like any other.
#[cfg(not(feature = "zip"))]
fn zip_folder_links(files: Vec<PathBuf>) -> (Vec<Rc<dyn FolderLink>>, Vec<PathBuf>) {
    (Vec::new(), files)
}

/// The files and the folders of `dir` (every level below when `recursive`).
fn list(
    dir: &Path,
    recursive: bool,
    files: &mut Vec<PathBuf>,
    directories: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            directories.push(path.clone());
            if recursive {
                list(&path, true, files, directories)?;
            }
        } else {
            files.push(path);
        }
    }
    Ok(())
}

impl Folder for DiskFolder {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_protected(&self) -> bool {
        false
    }

    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        self.content_changed.subscribe(listener);
    }

    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        self.content_changed.unsubscribe(listener);
    }

    fn default_sort(&self) -> Option<SortBehavior> {
        self.default_sort
    }

    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.child_containers.clone()
    }

    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.items.clone()
    }

    /// The folder's `index.md`, shown above its rows.
    fn header_markdown(&self) -> String {
        self.header_markdown.clone().unwrap_or_default()
    }

    /// Lists the folder's own level.
    fn load(&mut self) {
        self.load_recursive(false);
    }

    /// Reads names only up to the first sub-folder (`quick_look`): `load` would also look up
    /// every file's type, size and dates.
    fn has_child_containers(&mut self) -> Option<bool> {
        folder_has_child_containers(&self.full_path)
    }

    /// Active, and watching the folder.
    fn activate(&mut self) {
        let should_monitor = is_readable_directory(&self.full_path);
        self.live.activate(&self.full_path, should_monitor);
    }

    /// Stops watching and cancels a pending refresh.
    fn deactivate(&mut self) {
        self.live.teardown();
    }

    /// The UI thread's turn of the live refresh: a copy task's finished writes relist at once, a
    /// report from the watch becomes `directory_contents_changed`, and a refresh whose quiet half
    /// second has passed relists (`reload_content`). True while a refresh is still waiting.
    fn pump_live_refresh(&mut self) -> bool {
        if self.live.take_relist() {
            self.reload_content();
        }
        if self.live.take_report() {
            self.directory_contents_changed();
        }
        if self.live.refresh_due() {
            self.reload_content();
        }
        self.live.pending_refresh().is_some()
    }

    /// This folder's search (`search`).
    fn search_provider(&self) -> Option<&dyn SearchProvider> {
        Some(self)
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }

    /// A folder on disk is on the computer.
    fn place(&self) -> Option<PlaceId> {
        Some(PlaceId::COMPUTER)
    }

    /// The folder's path, as its row answers it.
    fn location_key(&self) -> Option<String> {
        Some(disk_location_key(&self.full_path))
    }

    fn as_writable(&self) -> Option<&dyn WritableFolder> {
        Some(self)
    }

    fn as_writable_mut(&mut self) -> Option<&mut dyn WritableFolder> {
        Some(self)
    }
}
