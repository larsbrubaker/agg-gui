//! A folder the user can change: the [`WritableFolder`] trait, the actions a folder is asked
//! about ([`ContainerActions`]), and the default move ([`add_then_remove`]). A writable folder
//! answers [`crate::Folder::as_writable`]/`as_writable_mut`; a browser's menu adds, creates
//! folders, renames and removes through it, asking `allow_action` first.
//!
//! Items are shared and immutable (`Rc<dyn Item>`), so the folder renames its own item
//! (`rename`) and lists itself again in the same call. `move_items` defaults to add-then-remove,
//! which a folder on disk overrides with a rename when the source is on disk too. Each change has
//! a recorded form (`*_recorded`) that also hands back the [`crate::HistoryStep`] undoing it (`history`);
//! the defaults run the plain change and record nothing, so a folder records only what it can
//! take back. `names_of_items_gone_from_source` is how a partly failed move names what it did
//! move.

use std::any::Any;
use std::collections::HashSet;
use std::rc::Rc;

use crate::create_folder_item::CreateFolderItem;
use crate::folder::Folder;
use crate::history::Recorded;
use crate::item::Item;
use crate::spawner::LocalTask;

/// What `allow_action` is asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContainerActions {
    /// Adding files (the menu's "Add").
    AddItems,
    /// Making folders ("Create Folder...").
    AddContainers,
    /// "Rename".
    RenameItems,
    /// "Remove".
    RemoveItems,
}

/// What a copy run by [`WritableFolder::copy_in`] ended in: the first failure (the items copied
/// before it stay copied) and the step that takes the copies back out, when it can.
pub type CopyOutcome = Recorded;

/// Takes a [`WritableFolder::copy_in`]'s outcome once the copy has finished.
pub type CopyDone = Box<dyn FnOnce(CopyOutcome)>;

/// A folder the user can change.
pub trait WritableFolder {
    /// Copies the items' content into this folder (a [`CreateFolderItem`] makes a folder), then
    /// lists it again. Answers the failures.
    fn add(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String>;

    /// Deletes the items (a folder on disk sends them to the system trash), then lists the
    /// folder again.
    fn remove(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String>;

    /// Renames one of this folder's items to `new_name` and lists the folder again.
    fn rename(&mut self, item: &Rc<dyn Item>, new_name: &str) -> Result<(), String>;

    /// Whether the folder takes `action` (by default: always).
    fn allow_action(&self, action: ContainerActions) -> bool {
        let _ = action;
        true
    }

    /// Moves the items into this folder from `source`. The default is [`add_then_remove`].
    /// Answers the first failure; the items moved before it stay moved.
    fn move_items(
        &mut self,
        items: &[Rc<dyn Item>],
        source: &mut dyn WritableFolder,
    ) -> Result<(), String> {
        add_then_remove(self, items, source)
    }

    /// The concrete folder, for a move that acts on its own folder type (a folder on disk moving
    /// from another folder on disk); `None` by default.
    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    /// [`Self::move_items`], with the step that moves back what moved. The default records
    /// nothing.
    fn move_in_recorded(
        &mut self,
        items: &[Rc<dyn Item>],
        source: &mut dyn WritableFolder,
    ) -> Recorded {
        Recorded::unrecorded(self.move_items(items, source))
    }

    /// [`Self::rename`], with the step that gives the old name back. The default records
    /// nothing.
    fn rename_recorded(&mut self, item: &Rc<dyn Item>, new_name: &str) -> Recorded {
        Recorded::unrecorded(self.rename(item, new_name))
    }

    /// Makes the folder `name` here (an `add` of a [`CreateFolderItem`]), with the step that
    /// takes it away again when it is new: a folder already there is kept, and undoing must not
    /// remove it. The default records nothing.
    fn create_folder_recorded(&mut self, name: &str) -> Recorded {
        let item: Rc<dyn Item> = Rc::new(CreateFolderItem::new(name));
        Recorded::unrecorded(self.add(&[item]))
    }

    /// [`Self::remove`], with the step that puts the removed items back (from the trash). The
    /// default records nothing.
    fn remove_recorded(&mut self, items: &[Rc<dyn Item>]) -> Recorded {
        Recorded::unrecorded(self.remove(items))
    }

    /// Copies `items` here as work that runs on after this returns (a download, a server-side
    /// copy), keeping the originals; `done` takes the outcome when it finishes. Answers the task
    /// for the caller to run on its spawner, or `None` (the default) for a folder that has no
    /// such copy, which the caller does another way (an `add`).
    fn copy_in(&self, items: Vec<Rc<dyn Item>>, done: CopyDone) -> Option<LocalTask> {
        let _ = (items, done);
        None
    }
}

/// The default move: each file ([`crate::AssetStream`]), then each folder
/// ([`crate::FolderLink`]), is added here and removed from `source` only once the add has
/// landed, so an item the destination could not take is still where it was. The items are walked
/// from the slice the caller handed in, listed once up front, so folders are not lost when the
/// caller clears its selection mid-move. Other items are not moved.
pub fn add_then_remove<W: WritableFolder + ?Sized>(
    destination: &mut W,
    items: &[Rc<dyn Item>],
    source: &mut dyn WritableFolder,
) -> Result<(), String> {
    let files = items.iter().filter(|i| i.as_asset_stream().is_some());
    let folders = items.iter().filter(|i| i.as_container_link().is_some());
    for item in files.chain(folders) {
        let one = std::slice::from_ref(item);
        destination.add(one)?;
        source.remove(one)?;
    }
    Ok(())
}

/// The names of the items `source` no longer lists - the only evidence that a partly completed
/// move actually moved something. Matched by ID rather than by identity, and against the folders
/// as well as the files: a folder that reloads during a move replaces every row with a fresh
/// one. The names come in the order of `items_to_move`. The caller must reload (or otherwise
/// update) the source before asking.
pub fn names_of_items_gone_from_source(
    items_to_move: &[Rc<dyn Item>],
    source: &dyn Folder,
) -> Vec<String> {
    // Only looked up, never iterated, so its order reaches no output.
    let remaining: HashSet<String> = source
        .items()
        .iter()
        .map(|item| item.id().to_string())
        .chain(source.child_containers().iter().map(|c| c.id().to_string()))
        .collect();
    items_to_move
        .iter()
        .filter(|item| !remaining.contains(item.id()))
        .map(|item| item.name().to_string())
        .collect()
}
