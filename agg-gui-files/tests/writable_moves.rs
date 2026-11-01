//! Moves between writable folders (`WritableFolder::move_items`, `add_then_remove`): a move is an
//! add followed by a remove, and the remove happens only once the add has landed. Also the
//! names a partly failed move reports (`names_of_items_gone_from_source`) and the recorded forms'
//! defaults, which run the plain change and record nothing. The folders and the folder link are
//! the test's own; an in-memory item is `StreamItem`, a file item with no content, which is all a
//! move looks at.
//!
//! Moves run to completion in the call and take the items as a slice the caller cannot change
//! while the move holds it, so the test about the caller's selection clearing mid-move hands the
//! move the caller's selection, clears it the moment the move returns, and checks both items
//! arrived.

use std::rc::Rc;

use agg_gui_files::{
    names_of_items_gone_from_source, Asset, AssetStream, CreateFolderItem, Folder, FolderLink,
    Item, LoadError, WritableFolder,
};

const UPLOAD_FAILURE_MESSAGE: &str = "the connection dropped mid-upload";

/// An in-memory library item named `name`.
struct StreamItem(String);

impl Item for StreamItem {
    fn id(&self) -> &str {
        &self.0
    }
    fn name(&self) -> &str {
        &self.0
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn as_asset_stream(&self) -> Option<&dyn AssetStream> {
        Some(self)
    }
}

impl Asset for StreamItem {
    fn content_type(&self) -> &str {
        "mcx"
    }
    fn category(&self) -> &str {
        ""
    }
    fn file_name(&self) -> String {
        format!("{}.mcx", self.0)
    }
    fn asset_path(&self) -> &str {
        ""
    }
}

impl AssetStream for StreamItem {
    fn get_stream(&self) -> Result<Option<Vec<u8>>, LoadError> {
        Ok(None)
    }
}

/// A folder in a library listing.
struct StubContainerLink(String);

impl Item for StubContainerLink {
    fn id(&self) -> &str {
        &self.0
    }
    fn name(&self) -> &str {
        &self.0
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn is_visible(&self) -> bool {
        true
    }
    fn as_container_link(&self) -> Option<&dyn FolderLink> {
        Some(self)
    }
}

impl FolderLink for StubContainerLink {
    /// No container: no move opens the folder.
    fn get_container(&self) -> Result<Box<dyn Folder>, String> {
        Err(format!("{} has no container", self.0))
    }
    fn is_read_only(&self) -> bool {
        false
    }
}

fn item(name: &str) -> Rc<dyn Item> {
    Rc::new(StreamItem(name.to_string()))
}

/// How a test container's add behaves.
#[derive(Default)]
enum AddBehavior {
    /// Takes everything.
    #[default]
    Records,
    /// Every add fails.
    Fails,
    /// The add of the named item fails.
    FailsOn(String),
}

/// A recording container: its items, and what it was asked to delete.
#[derive(Default)]
struct RecordingContainer {
    items: Vec<Rc<dyn Item>>,
    removed_items: Vec<Rc<dyn Item>>,
    add_behavior: AddBehavior,
}

impl RecordingContainer {
    fn with(add_behavior: AddBehavior) -> Self {
        Self {
            add_behavior,
            ..Self::default()
        }
    }

    fn contains(&self, item: &Rc<dyn Item>) -> bool {
        self.items.iter().any(|i| Rc::ptr_eq(i, item))
    }

    fn names(&self) -> String {
        self.items
            .iter()
            .map(|i| i.name().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl WritableFolder for RecordingContainer {
    fn add(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        let fails = match &self.add_behavior {
            AddBehavior::Records => false,
            AddBehavior::Fails => true,
            AddBehavior::FailsOn(name) => items.iter().any(|i| i.name() == name),
        };
        if fails {
            return Err(UPLOAD_FAILURE_MESSAGE.to_string());
        }
        self.items.extend(items.iter().cloned());
        Ok(())
    }

    fn remove(&mut self, items: &[Rc<dyn Item>]) -> Result<(), String> {
        for item in items {
            self.removed_items.push(Rc::clone(item));
            self.items.retain(|i| !Rc::ptr_eq(i, item));
        }
        Ok(())
    }

    fn rename(&mut self, _item: &Rc<dyn Item>, _new_name: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn move_whose_add_fails_leaves_the_item_in_the_source_container() {
    let mut source = RecordingContainer::default();
    let mut destination = RecordingContainer::with(AddBehavior::Fails);

    let item = item("Only Copy");
    source.items.push(Rc::clone(&item));

    let reported = destination
        .move_items(&[Rc::clone(&item)], &mut source)
        .expect_err("a move whose add never landed must fail");

    assert_eq!(
        reported, UPLOAD_FAILURE_MESSAGE,
        "a move whose add never landed must fail loudly with the destination's own failure"
    );
    assert_eq!(
        source.removed_items.len(),
        0,
        "nothing may be deleted from the source until the destination has the item"
    );
    assert!(
        source.contains(&item),
        "a failed move must leave the user the copy they had, not nothing at all"
    );
}

#[test]
fn move_whose_add_succeeds_removes_the_item_from_the_source_container() {
    let mut source = RecordingContainer::default();
    let mut destination = RecordingContainer::default();

    let item = item("Moving On");
    source.items.push(Rc::clone(&item));

    destination
        .move_items(&[Rc::clone(&item)], &mut source)
        .expect("the move completes");

    assert!(
        destination.contains(&item),
        "the destination has to end up holding the item"
    );
    assert!(
        !source.contains(&item),
        "a completed move takes the item out of the source"
    );
}

#[test]
fn move_moves_folders_even_when_the_callers_selection_is_cleared_while_the_move_is_running() {
    let mut source = RecordingContainer::default();
    let mut destination = RecordingContainer::default();

    let file = item("A File");
    let folder: Rc<dyn Item> = Rc::new(StubContainerLink("A Folder".to_string()));

    // The shape of the real caller: the live selection.
    let mut selection = vec![Rc::clone(&file), Rc::clone(&folder)];
    source.items.push(Rc::clone(&file));
    source.items.push(Rc::clone(&folder));

    let moved = destination.move_items(&selection, &mut source);

    // What a browser's Move action does the instant the move is started.
    selection.clear();
    moved.expect("the move completes");

    assert!(
        destination.contains(&folder),
        "the folder the user asked to move has to arrive, not vanish from the move half way through"
    );
    assert!(
        !source.contains(&folder),
        "a folder that moved is gone from where it was"
    );
    assert!(
        destination.contains(&file),
        "the file moves too - the folder must not come at its expense"
    );
}

#[test]
fn move_that_fails_partway_keeps_the_items_it_already_moved_and_leaves_the_rest_in_the_source() {
    let mut source = RecordingContainer::default();
    let mut destination = RecordingContainer::with(AddBehavior::FailsOn("Third".to_string()));

    let first = item("First");
    let second = item("Second");
    let third = item("Third");

    source.items.push(Rc::clone(&first));
    source.items.push(Rc::clone(&second));
    source.items.push(Rc::clone(&third));

    assert!(destination
        .move_items(&[first, second, third], &mut source)
        .is_err());

    assert_eq!(
        source.names(),
        "Third",
        "only the item whose add failed is still the source's problem"
    );
    assert_eq!(
        destination.names(),
        "First, Second",
        "the items that did land stay landed - the failure does not undo them"
    );
}

/// A folder listing fresh rows with the given IDs, as a folder that reloaded mid-move does.
struct ListedFolder {
    items: Vec<&'static str>,
    folders: Vec<&'static str>,
}

impl Folder for ListedFolder {
    fn id(&self) -> &str {
        "listed"
    }
    fn name(&self) -> &str {
        "Listed"
    }
    fn is_protected(&self) -> bool {
        false
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.folders
            .iter()
            .map(|id| Rc::new(StubContainerLink(id.to_string())) as Rc<dyn FolderLink>)
            .collect()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        self.items.iter().map(|id| item(id)).collect()
    }
    fn load(&mut self) {}
}

#[test]
fn a_partly_failed_move_names_only_what_the_source_no_longer_lists() {
    let asked: Vec<Rc<dyn Item>> = vec![
        item("Moved File"),
        Rc::new(StubContainerLink("Moved Folder".to_string())),
        item("Stayed File"),
        Rc::new(StubContainerLink("Stayed Folder".to_string())),
    ];
    // Fresh rows with the same IDs: identity must not be what tells them apart.
    let source = ListedFolder {
        items: vec!["Stayed File"],
        folders: vec!["Stayed Folder"],
    };
    assert_eq!(
        names_of_items_gone_from_source(&asked, &source),
        vec!["Moved File".to_string(), "Moved Folder".to_string()],
        "the user is told about exactly what moved, folders included, in the order they asked"
    );
}

#[test]
fn recorded_forms_run_the_plain_change_and_record_nothing_by_default() {
    let mut source = RecordingContainer::default();
    let mut destination = RecordingContainer::default();
    let moving = item("Recorded Move");
    source.items.push(Rc::clone(&moving));

    let moved = destination.move_in_recorded(&[Rc::clone(&moving)], &mut source);
    assert!(moved.result.is_ok(), "the move itself still happens");
    assert!(
        moved.step.is_none(),
        "a folder that keeps no history hands back no step"
    );
    assert!(destination.contains(&moving) && !source.contains(&moving));

    let removed = destination.remove_recorded(&[Rc::clone(&moving)]);
    assert!(removed.result.is_ok() && removed.step.is_none());
    assert!(
        !destination.contains(&moving),
        "the remove itself still happens"
    );

    let mut failing = RecordingContainer::with(AddBehavior::Fails);
    let refused = failing.create_folder_recorded("New Folder");
    assert_eq!(
        refused.result,
        Err(UPLOAD_FAILURE_MESSAGE.to_string()),
        "a refused change hands its failure back unchanged"
    );
}

#[test]
fn create_folder_recorded_hands_the_folder_a_create_folder_item_by_its_name() {
    let mut destination = RecordingContainer::default();
    let made = destination.create_folder_recorded("Brackets");
    assert!(made.result.is_ok() && made.step.is_none());
    let added = destination
        .items
        .first()
        .expect("the folder was asked to add one item");
    assert_eq!(added.name(), "Brackets");
    assert!(
        added.as_any().is_some_and(|a| a.is::<CreateFolderItem>()),
        "a writable folder recognizes the request by its type, so it makes a folder, not a file"
    );
    let other = CreateFolderItem::new("Brackets");
    assert_ne!(
        added.id(),
        other.id(),
        "two requests are two items, even for the same name"
    );
}

#[test]
fn copy_in_by_default_has_no_copy_of_its_own() {
    let destination = RecordingContainer::default();
    let task = destination.copy_in(vec![item("Copy Me")], Box::new(|_| {}));
    assert!(
        task.is_none(),
        "a folder without its own copy leaves the caller to copy another way"
    );
}
