//! Drag and drop of the browser's items onto its folders: [`DropManager`], kept in the browser's
//! state (`BrowserState::drop_manager`), moves dragged items into the folder they are dropped on,
//! a folder row or tile in the list or a row of the folder tree.
//!
//! Rather than hit-testing the widget tree, the folders a drop can land on record their window
//! rectangles as they paint (`register_target`, logical units, Y up), each painter forgetting its
//! own kind first (`clear_targets`), so a folder scrolled away or folded shut is gone by the next
//! frame; the list records its own rectangle (`set_list_rect`). The drag looks them up.
//!
//! The target under the drag is `highlighted()`: the row, tile or tree row with that key fills
//! itself with the accent at alpha 40 while it paints, and `drop_label()` says what the drop will
//! do ("Move to Brackets", or "Copy to Brackets" with a "+" badge, `badge.rs`). A folder the
//! drop can't land on (read-only, one of the dragged items) is never highlighted.
//!
//! Whether a drop moves or copies is the config's `PlaceRules` (the copy modifier, a source that
//! keeps its items, two places); the drop itself is `execute.rs`, which records its history step.
//! The press that becomes a drag, and its release, are `gesture.rs`.

pub mod badge;
pub mod execute;
pub mod gesture;
pub mod picture;
pub mod spring;

use std::rc::Rc;

use agg_gui::{Point, Rect};
use agg_gui_files::{FolderLink, Item, PlaceId};

use crate::drops::OutsideOver;
use crate::state::BrowserState;

/// Which painter recorded a drop target, so each forgets only its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DropTargetKind {
    /// A folder tree row.
    Tree,
    /// A folder row or tile in the list.
    Content,
}

/// What a target is known by: its kind and its index (tree row, or listed row).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DropKey {
    pub kind: DropTargetKind,
    pub index: usize,
}

/// What a drop on the highlighted folder will do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropLabel {
    /// True for a copy (the "+" badge shows), false for a move.
    pub copies: bool,
    /// "Copy to Brackets" or "Move to Brackets".
    pub text: String,
}

/// One folder a drop can land on, where it was last painted.
struct DropTarget {
    rect: Rect,
    link: Rc<dyn FolderLink>,
    key: DropKey,
}

fn contains(rect: &Rect, at: Point) -> bool {
    at.x >= rect.x && at.y >= rect.y && at.x <= rect.x + rect.width && at.y <= rect.y + rect.height
}

/// The drag of the browser's items (see the module header).
#[derive(Default)]
pub struct DropManager {
    targets: Vec<DropTarget>,
    /// The list's window rectangle.
    list_rect: Option<Rect>,
    /// The items the drag carries, while one is under way.
    items: Option<Vec<Rc<dyn Item>>>,
    /// The depth in the browser's path of the folder the drag came from.
    source_depth: usize,
    highlighted: Option<DropKey>,
    label: Option<DropLabel>,
    /// The place of each folder the drag has been over, by its ID, asked once per drag.
    target_places: Vec<(String, Option<PlaceId>)>,
    /// The folder tree's window rectangle.
    tree_rect: Option<Rect>,
    /// The folder the drag waits over to open, and the ones it opened (`spring.rs`).
    pub(crate) spring: spring::SpringOpen,
    /// What the app said is under the drag outside the browser, while it is there.
    pub(crate) outside: Option<OutsideOver>,
    /// The picture the drag carries (`picture.rs`).
    pub(crate) picture: picture::DragPicture,
}

impl DropManager {
    /// Forgets the targets of `kind`, before their painter records them again.
    pub fn clear_targets(&mut self, kind: DropTargetKind) {
        self.targets.retain(|t| t.key.kind != kind);
    }

    /// Records a folder a drop can land on, at `rect` (window coordinates, logical units).
    pub fn register_target(&mut self, rect: Rect, link: Rc<dyn FolderLink>, key: DropKey) {
        self.targets.retain(|t| t.key != key);
        self.targets.push(DropTarget { rect, link, key });
    }

    /// Records the list's window rectangle.
    pub fn set_list_rect(&mut self, rect: Rect) {
        self.list_rect = Some(rect);
    }

    /// Records the folder tree's window rectangle.
    pub fn set_tree_rect(&mut self, rect: Rect) {
        self.tree_rect = Some(rect);
    }

    /// Whether `at` (window coordinates) is over the list or the folder tree: inside the browser.
    pub fn browser_contains(&self, at: Point) -> bool {
        self.list_contains(at) || self.tree_rect.is_some_and(|r| contains(&r, at))
    }

    /// Whether `at` (window coordinates) is over the list.
    pub fn list_contains(&self, at: Point) -> bool {
        self.list_rect.is_some_and(|r| contains(&r, at))
    }

    /// Whether a drag is under way.
    pub fn is_dragging(&self) -> bool {
        self.items.is_some()
    }

    /// The items the drag carries, empty with none under way.
    pub fn dragged_items(&self) -> &[Rc<dyn Item>] {
        self.items.as_deref().unwrap_or(&[])
    }

    /// The depth in the browser's path of the folder the drag came from.
    pub fn source_depth(&self) -> usize {
        self.source_depth
    }

    /// The target drawn highlighted, if any.
    pub fn highlighted(&self) -> Option<DropKey> {
        self.highlighted
    }

    /// What a drop on the highlighted folder does, while there is one.
    pub fn drop_label(&self) -> Option<&DropLabel> {
        self.label.as_ref()
    }

    /// What a drop on target `key` does, empty unless it is the one highlighted: a folder row's,
    /// tile's or tree row's `drop_label` property.
    pub fn label_for(&self, key: DropKey) -> String {
        match &self.label {
            Some(label) if self.highlighted == Some(key) => label.text.clone(),
            _ => String::new(),
        }
    }

    /// The highlighted target when a drop there copies: where the "+" badge shows.
    pub fn copy_target(&self) -> Option<DropKey> {
        self.highlighted
            .filter(|_| self.label.as_ref().is_some_and(|l| l.copies))
    }

    /// Whether `item` is one the drag carries (its row paints lifted), matched by allocation:
    /// the drag carries the very entries the rows were built from.
    pub fn carries(&self, item: &Rc<dyn Item>) -> bool {
        self.carried_rank(item).is_some()
    }

    /// `item`'s place among the carried items, `None` when it isn't carried.
    pub fn carried_rank(&self, item: &Rc<dyn Item>) -> Option<usize> {
        self.dragged_items()
            .iter()
            .position(|c| std::ptr::addr_eq(Rc::as_ptr(c), Rc::as_ptr(item)))
    }

    /// Starts a drag of `items` from the folder at `source_depth`.
    pub fn begin(&mut self, items: Vec<Rc<dyn Item>>, source_depth: usize) {
        crate::trace(|| format!("drag begins with {} items", items.len()));
        self.items = Some(items);
        self.source_depth = source_depth;
        self.target_places.clear();
    }

    /// Ends the drag without a drop (the outside drop hook, if the drag was over the app's
    /// views, is told by the gesture, `gesture::drag_lost`).
    pub fn cancel(&mut self) {
        self.take_items();
    }

    /// Takes the drag's items as it drops, ending it.
    fn take_items(&mut self) -> Option<Vec<Rc<dyn Item>>> {
        let items = self.items.take();
        self.target_places.clear();
        self.spring.clear();
        self.outside = None;
        self.picture = picture::DragPicture::default();
        self.set_highlight(None, None);
        items
    }

    fn set_highlight(&mut self, key: Option<DropKey>, label: Option<DropLabel>) {
        if key != self.highlighted || label != self.label {
            agg_gui::animation::request_draw();
        }
        self.highlighted = key;
        self.label = label;
    }

    /// Whether a folder can take the drop: not read-only, and not one of the dragged items.
    fn takes_drop(&self, link: &dyn FolderLink) -> bool {
        !link.is_read_only() && !self.dragged_items().iter().any(|i| i.id() == link.id())
    }

    /// The target the drop at `at` lands on: a tree row first, then a folder in the list (only
    /// inside the list's rectangle, which clips its rows); `None` over anything else, or over a
    /// folder that can't take it.
    fn target_at(&self, at: Point) -> Option<&DropTarget> {
        let tree = self
            .targets
            .iter()
            .find(|t| t.key.kind == DropTargetKind::Tree && contains(&t.rect, at));
        if let Some(tree) = tree {
            return self.takes_drop(tree.link.as_ref()).then_some(tree);
        }
        if self.list_rect.is_some() && !self.list_contains(at) {
            return None;
        }
        self.targets.iter().find(|t| {
            t.key.kind == DropTargetKind::Content
                && contains(&t.rect, at)
                && self.takes_drop(t.link.as_ref())
        })
    }

    /// The folder recorded as target `key`, where it was last painted.
    pub(crate) fn target_link(&self, key: DropKey) -> Option<Rc<dyn FolderLink>> {
        self.targets
            .iter()
            .find(|t| t.key == key)
            .map(|t| Rc::clone(&t.link))
    }

    /// The folder a drag held at `at` would open: any tree row (a folder that can't take the drop
    /// may hold one that can), or a folder in the list that could take it.
    pub(crate) fn spring_target_at(&self, at: Point) -> Option<(DropKey, Rc<dyn FolderLink>)> {
        let tree = self
            .targets
            .iter()
            .find(|t| t.key.kind == DropTargetKind::Tree && contains(&t.rect, at));
        tree.or_else(|| {
            self.target_at(at)
                .filter(|t| t.key.kind == DropTargetKind::Content)
        })
        .map(|t| (t.key, Rc::clone(&t.link)))
    }

    /// The folder showing (at path depth `showing`), when the list opened it for this drag, the
    /// items came from another folder and `at` is on the list's empty space (no folder there).
    pub(crate) fn dropped_into_showing(
        &self,
        at: Point,
        showing: usize,
    ) -> Option<Rc<dyn FolderLink>> {
        if self.source_depth == showing
            || !self.list_contains(at)
            || self.spring_target_at(at).is_some()
        {
            return None;
        }
        self.spring.showing_folder(showing)
    }

    /// The link and key of the target at `at`.
    pub(crate) fn target_link_at(&self, at: Point) -> Option<(DropKey, Rc<dyn FolderLink>)> {
        self.target_at(at).map(|t| (t.key, Rc::clone(&t.link)))
    }

    /// The place of the folder `link` opens, asked once per drag.
    pub(crate) fn place_of(&mut self, link: &dyn FolderLink) -> Option<PlaceId> {
        if let Some((_, place)) = self.target_places.iter().find(|(id, _)| id == link.id()) {
            return place.clone();
        }
        let place = link.place();
        self.target_places
            .push((link.id().to_string(), place.clone()));
        place
    }
}

impl BrowserState {
    /// The drag of the browser's items onto its folders.
    pub fn drop_manager(&self) -> &DropManager {
        &self.drop
    }

    /// The drag of the browser's items onto its folders, to change it.
    pub fn drop_manager_mut(&mut self) -> &mut DropManager {
        &mut self.drop
    }
}
