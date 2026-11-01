//! [`MemoryFolder`]: a writable folder held in memory, for tests and demos that must never touch
//! the user's real files (agg-gui's demo browses one as its sample folder). It behaves as a real
//! folder does: it lists its sub-folders ([`MemoryFolderLink`]s) and files ([`MemoryFile`]s),
//! takes adds, removes and renames, and raises content-changed after each change.
//!
//! A folder is a handle on shared contents: every `MemoryFolder` opened on the same folder (its
//! link's `get_container`, or `add_folder` again) sees the same files and the same listeners, so
//! a change made through one shows in every view of it. Items are immutable, as everywhere in
//! the crate: a rename lists a new item with the same ID. The name a handle and a link answer is
//! the one the folder had when they were made, as a listing is a snapshot.
//!
//! Adding copies: a file's bytes are read ([`crate::AssetStream::get_stream`]), a folder link's
//! folder is opened, loaded and copied whole, and a [`crate::CreateFolderItem`] makes a folder
//! (an existing one of that name is kept; a file of that name refuses it, as a folder on disk
//! can't be made where a file of its name is). A copied file or folder whose name is already in
//! the folder gets a number, before its extension ("gear (2).stl"). A rename to an empty name or
//! to another item's name is refused. A move is the default add-then-remove. Nothing is recorded
//! for undo. IDs (`memory-<n>`) are unique within a run and never stored; a folder's location key
//! is its ID, the same from the folder and its link.
//!
//! A folder made by [`MemoryFolder::new`] is protected; its sub-folders are not. Every folder in a
//! tree answers the one place [`MemoryFolder::with_place`] gives any of them, for as long as any
//! handle on the tree lives; a folder removed from its tree is in no place.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::create_folder_item::CreateFolderItem;
use crate::error::LoadError;
use crate::folder::{ContentChangedEvent, ContentChangedListener, Folder};
use crate::item::{Asset, AssetStream, FolderLink, Item};
use crate::place::PlaceId;
use crate::translate::tr;
use crate::writable::WritableFolder;

/// Counts the folders and files made in this run, for their IDs.
static NEXT_NUMBER: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    format!("memory-{}", NEXT_NUMBER.fetch_add(1, Ordering::Relaxed))
}

/// The place a whole tree of folders answers, one cell shared by every folder in it.
type PlaceCell = Rc<RefCell<Option<PlaceId>>>;

/// One folder's contents, shared by every handle and link on it.
struct Node {
    id: String,
    name: RefCell<String>,
    protected: bool,
    /// The tree's place, shared with every folder in the tree, so it holds for as long as any
    /// handle on any of them lives. A folder taken out of its tree gets a cell of its own.
    place: RefCell<PlaceCell>,
    /// The folder listing this one; empty for a top folder and for one taken out. Weak, as the
    /// parent lists its sub-folders strongly: while a folder above this one is alive, every
    /// folder between them is too (each listed by the one above), so the walk up from here
    /// reaches every folder above that anyone can still name.
    parent: RefCell<Weak<Node>>,
    folders: RefCell<Vec<Rc<Node>>>,
    files: RefCell<Vec<Rc<MemoryFile>>>,
    content_changed: ContentChangedEvent,
}

impl Node {
    fn new(name: &str, protected: bool, parent: Weak<Node>, place: PlaceCell) -> Rc<Self> {
        Rc::new(Self {
            id: next_id(),
            name: RefCell::new(name.to_string()),
            protected,
            place: RefCell::new(place),
            parent: RefCell::new(parent),
            folders: RefCell::default(),
            files: RefCell::default(),
            content_changed: ContentChangedEvent::default(),
        })
    }

    /// A new sub-folder record of this folder (not yet listed), in this folder's tree.
    fn child(self: &Rc<Self>, name: &str) -> Rc<Node> {
        let place = Rc::clone(&self.place.borrow());
        Node::new(name, false, Rc::downgrade(self), place)
    }

    fn place(&self) -> Option<PlaceId> {
        self.place.borrow().borrow().clone()
    }

    /// Takes this folder (and everything in it) out of its tree: no parent, and no place.
    fn detach(&self) {
        *self.parent.borrow_mut() = Weak::new();
        self.share_place(&Rc::new(RefCell::new(None)));
    }

    /// Makes `place` the place cell of this folder and everything in it.
    fn share_place(&self, place: &PlaceCell) {
        *self.place.borrow_mut() = Rc::clone(place);
        for folder in self.folders.borrow().iter() {
            folder.share_place(place);
        }
    }

    /// Whether `other` is this folder or one of the folders above it (see `parent`).
    fn is_self_or_above(&self, other: &Node) -> bool {
        if std::ptr::eq(self, other) {
            return true;
        }
        let parent = self.parent.borrow().upgrade();
        parent.is_some_and(|parent| parent.is_self_or_above(other))
    }

    /// Whether a file here is called `name`.
    fn is_file_name(&self, name: &str) -> bool {
        self.files.borrow().iter().any(|f| f.name == name)
    }

    /// Whether a file or folder here is called `name`.
    fn is_taken(&self, name: &str) -> bool {
        self.is_file_name(name)
            || self
                .folders
                .borrow()
                .iter()
                .any(|f| *f.name.borrow() == name)
    }

    /// `name`, or the first "stem (n).ext" not taken here.
    fn free_name(&self, name: &str) -> String {
        if !self.is_taken(name) {
            return name.to_string();
        }
        let (stem, extension) = match name.rfind('.') {
            Some(dot) if dot > 0 => name.split_at(dot),
            _ => (name, ""),
        };
        (2u64..)
            .map(|n| format!("{stem} ({n}){extension}"))
            .find(|candidate| !self.is_taken(candidate))
            .unwrap_or_else(|| name.to_string())
    }

    /// The sub-folder `name`, made when there is none. A file of that name refuses it, as a
    /// folder on disk cannot be made where a file of its name is.
    fn folder_named(self: &Rc<Self>, name: &str) -> Result<Rc<Node>, String> {
        if let Some(found) = self
            .folders
            .borrow()
            .iter()
            .find(|f| *f.name.borrow() == name)
        {
            return Ok(Rc::clone(found));
        }
        if self.is_file_name(name) {
            return Err(tr("{0} is already in this folder").replace("{0}", name));
        }
        let made = self.child(name);
        self.folders.borrow_mut().push(Rc::clone(&made));
        Ok(made)
    }
}

/// A file held in memory.
pub struct MemoryFile {
    id: String,
    name: String,
    bytes: Rc<[u8]>,
}

impl MemoryFile {
    /// The file's bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Item for MemoryFile {
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
    fn as_asset(&self) -> Option<&dyn Asset> {
        Some(self)
    }
    fn as_asset_stream(&self) -> Option<&dyn AssetStream> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl Asset for MemoryFile {
    /// The name's extension, as written; empty for a name with none.
    fn content_type(&self) -> &str {
        match self.name.rfind('.') {
            Some(dot) if dot > 0 => &self.name[dot + 1..],
            _ => "",
        }
    }
    fn category(&self) -> &str {
        ""
    }
    fn file_name(&self) -> String {
        self.name.clone()
    }
    /// Empty: the content is in memory, at no path.
    fn asset_path(&self) -> &str {
        ""
    }
}

impl AssetStream for MemoryFile {
    fn get_stream(&self) -> Result<Option<Vec<u8>>, LoadError> {
        Ok(Some(self.bytes.to_vec()))
    }
}

/// The row that opens a sub-folder of a [`MemoryFolder`].
pub struct MemoryFolderLink {
    node: Rc<Node>,
    name: String,
}

impl MemoryFolderLink {
    fn of(node: &Rc<Node>) -> Self {
        Self {
            node: Rc::clone(node),
            name: node.name.borrow().clone(),
        }
    }
}

impl Item for MemoryFolderLink {
    fn id(&self) -> &str {
        &self.node.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_protected(&self) -> bool {
        self.node.protected
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl FolderLink for MemoryFolderLink {
    fn get_container(&self) -> Result<Box<dyn Folder>, String> {
        Ok(Box::new(MemoryFolder::on(&self.node)))
    }
    fn is_read_only(&self) -> bool {
        false
    }
    /// Known at once: the folder is in memory.
    fn has_child_containers(&self) -> Option<bool> {
        Some(!self.node.folders.borrow().is_empty())
    }
    fn place(&self) -> Option<PlaceId> {
        self.node.place()
    }
    fn location_key(&self) -> Option<String> {
        Some(self.node.id.clone())
    }
}

/// A writable folder held in memory (see the module header).
#[derive(Clone)]
pub struct MemoryFolder {
    node: Rc<Node>,
    name: String,
}

impl MemoryFolder {
    /// An empty top folder called `name`, protected and in no place.
    pub fn new(name: impl Into<String>) -> Self {
        let place: PlaceCell = Rc::default();
        Self::on(&Node::new(&name.into(), true, Weak::new(), place))
    }

    fn on(node: &Rc<Node>) -> Self {
        Self {
            node: Rc::clone(node),
            name: node.name.borrow().clone(),
        }
    }

    /// The folder in `place`, with every folder in its tree (above and below it), so moves and
    /// copies between places follow the place rule.
    pub fn with_place(self, place: PlaceId) -> Self {
        *self.node.place.borrow().borrow_mut() = Some(place);
        self
    }

    /// Adds the file `name` holding `bytes` (numbered when the name is taken), raises
    /// content-changed, and answers the item listed.
    pub fn add_file(&self, name: &str, bytes: impl Into<Vec<u8>>) -> Rc<dyn Item> {
        let item = self.put_file(name, bytes.into().into());
        self.node.content_changed.raise();
        item
    }

    /// The sub-folder `name`, made (raising content-changed) when there is none; refused when a
    /// file here has that name.
    pub fn add_folder(&self, name: &str) -> Result<MemoryFolder, String> {
        let before = self.node.folders.borrow().len();
        let folder = MemoryFolder::on(&self.node.folder_named(name)?);
        if self.node.folders.borrow().len() != before {
            self.node.content_changed.raise();
        }
        Ok(folder)
    }

    /// A row opening this folder, for a parent folder or a top level to list.
    pub fn link(&self) -> Rc<dyn FolderLink> {
        Rc::new(MemoryFolderLink::of(&self.node))
    }

    fn put_file(&self, name: &str, bytes: Rc<[u8]>) -> Rc<dyn Item> {
        let file = Rc::new(MemoryFile {
            id: next_id(),
            name: self.node.free_name(name),
            bytes,
        });
        self.node.files.borrow_mut().push(Rc::clone(&file));
        file
    }

    /// Copies one item here.
    fn add_one(&self, item: &Rc<dyn Item>) -> Result<(), String> {
        if let Some(create) = item
            .as_any()
            .and_then(|a| a.downcast_ref::<CreateFolderItem>())
        {
            return self.node.folder_named(create.name()).map(|_| ());
        }
        if let Some(stream) = item.as_asset_stream() {
            let bytes = stream
                .get_stream()
                .map_err(|error| error.to_string())?
                .ok_or_else(|| tr("{0} has no content to copy").replace("{0}", item.name()))?;
            self.put_file(item.name(), bytes.into());
            return Ok(());
        }
        if let Some(link) = item.as_container_link() {
            if let Some(own) = item
                .as_any()
                .and_then(|a| a.downcast_ref::<MemoryFolderLink>())
            {
                if self.node.is_self_or_above(&own.node) {
                    return Err(tr("A folder cannot be copied into itself"));
                }
            }
            let mut source = link.get_container()?;
            source.load();
            let name = self.node.free_name(item.name());
            let copy = MemoryFolder::on(&self.node.child(&name));
            self.node.folders.borrow_mut().push(Rc::clone(&copy.node));
            for child in source.child_containers() {
                let child: Rc<dyn Item> = child;
                copy.add_one(&child)?;
            }
            for child in source.items() {
                copy.add_one(&child)?;
            }
            return Ok(());
        }
        Err(tr("{0} has no content to copy").replace("{0}", item.name()))
    }

    /// Takes one item (by ID) out of this folder.
    fn remove_one(&self, item: &Rc<dyn Item>) -> Result<(), String> {
        let mut files = self.node.files.borrow_mut();
        if let Some(at) = files.iter().position(|f| f.id == item.id()) {
            files.remove(at);
            return Ok(());
        }
        let mut folders = self.node.folders.borrow_mut();
        if let Some(at) = folders.iter().position(|f| f.id == item.id()) {
            folders.remove(at).detach();
            return Ok(());
        }
        Err(tr("{0} is not in this folder").replace("{0}", item.name()))
    }

    /// Runs `change` and raises content-changed, whatever it answers: a change that failed part
    /// way may still have changed something.
    fn changing(&self, change: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let outcome = change();
        self.node.content_changed.raise();
        outcome
    }
}

impl Folder for MemoryFolder {
    fn id(&self) -> &str {
        &self.node.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn is_protected(&self) -> bool {
        self.node.protected
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.node
            .folders
            .borrow()
            .iter()
            .map(|node| Rc::new(MemoryFolderLink::of(node)) as Rc<dyn FolderLink>)
            .collect()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.node
            .files
            .borrow()
            .iter()
            .map(|file| Rc::clone(file) as Rc<dyn Item>)
            .collect()
    }
    /// Nothing to do: the contents are in memory.
    fn load(&mut self) {}
    fn subscribe_content_changed(&self, listener: ContentChangedListener) {
        self.node.content_changed.subscribe(listener);
    }
    fn unsubscribe_content_changed(&self, listener: &ContentChangedListener) {
        self.node.content_changed.unsubscribe(listener);
    }
    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
    fn as_writable(&self) -> Option<&dyn WritableFolder> {
        Some(self)
    }
    fn as_writable_mut(&mut self) -> Option<&mut dyn WritableFolder> {
        Some(self)
    }
    fn place(&self) -> Option<PlaceId> {
        self.node.place()
    }
    fn location_key(&self) -> Option<String> {
        Some(self.node.id.clone())
    }
}

impl WritableFolder for MemoryFolder {
    /// Copies each item here (see the module header); stops at the first failure, the items
    /// before it staying added.
    fn add(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        self.changing(|| items.iter().try_for_each(|item| self.add_one(item)))
    }

    /// Takes the items out; stops at the first one this folder does not list.
    fn remove(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        self.changing(|| items.iter().try_for_each(|item| self.remove_one(item)))
    }

    /// Lists `item` (a file or a sub-folder, by ID) as `new_name`, keeping its ID. An empty name
    /// (or only spaces) and a name another item here has are refused.
    fn rename(&mut self, item: &Rc<dyn Item>, new_name: &str) -> Result<(), String> {
        if new_name.trim().is_empty() {
            return Err(tr("A name cannot be empty"));
        }
        let node = Rc::clone(&self.node);
        self.changing(|| {
            let taken_by_other = node
                .files
                .borrow()
                .iter()
                .any(|f| f.name == new_name && f.id != item.id())
                || node
                    .folders
                    .borrow()
                    .iter()
                    .any(|f| *f.name.borrow() == new_name && f.id != item.id());
            if taken_by_other {
                return Err(tr("{0} is already in this folder").replace("{0}", new_name));
            }
            let mut files = node.files.borrow_mut();
            if let Some(at) = files.iter().position(|f| f.id == item.id()) {
                let old = &files[at];
                files[at] = Rc::new(MemoryFile {
                    id: old.id.clone(),
                    name: new_name.to_string(),
                    bytes: Rc::clone(&old.bytes),
                });
                return Ok(());
            }
            if let Some(folder) = node.folders.borrow().iter().find(|f| f.id == item.id()) {
                *folder.name.borrow_mut() = new_name.to_string();
                return Ok(());
            }
            Err(tr("{0} is not in this folder").replace("{0}", item.name()))
        })
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        Some(self)
    }
}
