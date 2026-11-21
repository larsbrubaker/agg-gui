//! Drag and drop of the browser's items onto its folders: [`DropManager`], kept in the browser's
//! state (`BrowserState::drop_manager`), moves dragged items into the folder they are dropped on,
//! a folder row or tile in the list or a row of the folder tree.
//!
//! Rather than hit-testing the widget tree, the folders a drop can land on record their window
//! rectangles as they paint (`register_target`, logical units, Y up), each painter forgetting its
//! own kind first (`clear_targets`), so a folder scrolled away or folded shut is gone by the next
//! frame; the list and the tree record their own rectangles (`set_list_rect`, `set_tree_rect`),
//! and a folder takes a drop only inside its own view's rectangle, so a row scrolled past the
//! edge of its view, still painted under the clip, takes none. The drag looks them up.
//!
//! The target under the drag is `highlighted()`: the row, tile or tree row with that key fills
//! itself with the accent at alpha 40 while it paints, and `drop_label()` says what the drop will
//! do ("Move to Brackets", or "Copy to Brackets" with a "+" badge, `badge.rs`). A folder the
//! drop can't land on (read-only, one of the dragged items, or the folder the items are already
//! in, told apart by place and location key as Move does, `FolderKey`) is never highlighted.
//!
//! A folder the app takes drops on itself (`PlaceRules::app_drop`, asked once per folder per drag
//! by the gesture with no state borrowed, `gesture::ask_app_drops`, and told by its link's
//! allocation) is a target even when read-only, labelled as the app says; one the app refuses is
//! never a target, and its reason is [`DropManager::refusal`] while the drag is over it.
//!
//! Whether a drop moves or copies is the config's `PlaceRules` (the copy modifier, a source that
//! keeps its items, two places); the drop itself is `execute.rs`, which records its history step.
//! The press that becomes a drag, and its release, are `gesture.rs`.

pub mod badge;
pub mod execute;
pub mod gesture;
pub mod picture;
pub mod spring;

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{Point, Rect};
use agg_gui_files::{FolderLink, Item, PlaceId};

use crate::drops::OutsideOver;
use crate::move_dialog::plan::FolderKey;
use crate::places::AppDrop;
use crate::state::BrowserState;

/// Which painter recorded a drop target, so each forgets only its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DropTargetKind {
    /// A folder tree row.
    Tree,
    /// A folder row or tile in the list.
    Content,
    /// The list's empty space, while a drop there lands in the folder showing (one the drag
    /// opened, `spring.rs`); its index is always 0 ([`LIST_DROP_KEY`]).
    List,
}

/// The key of the list's empty space as a drop target: the list lights up as a folder does.
pub const LIST_DROP_KEY: DropKey = DropKey {
    kind: DropTargetKind::List,
    index: 0,
};

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

/// Whether `link` is the very link `held` (the same allocation).
fn same_link(held: &Rc<dyn FolderLink>, link: &dyn FolderLink) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(held), link as *const dyn FolderLink)
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
    /// The key of the folder the drag came from, when it has one: that folder takes no drop.
    source_key: Option<FolderKey>,
    /// The key of each folder the drag has been over, by its ID, asked once per drag.
    target_keys: RefCell<Vec<(String, Option<FolderKey>)>>,
    /// What the app does with the drop on each folder the drag has been over
    /// (`PlaceRules::app_drop`), asked once per drag. A folder is told by its link's allocation,
    /// not its ID, as an app's folders may all answer the same empty ID; the link is held so its
    /// allocation is not reused by another folder while the drag lasts.
    app_drops: Vec<(Rc<dyn FolderLink>, AppDrop)>,
    /// Why the folder under the drag refuses it, while it is over one the app refuses.
    refusal: Option<String>,
    /// The folder the drag waits over to open, and the ones it opened (`spring.rs`).
    pub(crate) spring: spring::SpringOpen,
    /// What the app said is under the drag outside the browser, while it is there.
    pub(crate) outside: Option<OutsideOver>,
    /// The drag came back into the browser from the app's view, which kept itself as the drag
    /// left it (`OutsideDrops::cancel_on_return`): the drag's end still owes it a word.
    pub(crate) app_view_kept: bool,
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
        self.source_key = None;
        self.target_places.clear();
        self.target_keys.borrow_mut().clear();
        self.app_drops.clear();
        self.refusal = None;
    }

    /// Names the folder the drag came from (its place and location key), which then takes no
    /// drop: dropping items on their own folder would only rename them.
    pub fn set_source_key(&mut self, key: Option<FolderKey>) {
        self.source_key = key;
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
        self.target_keys.borrow_mut().clear();
        self.source_key = None;
        self.app_drops.clear();
        self.refusal = None;
        self.spring.clear();
        self.outside = None;
        self.app_view_kept = false;
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

    /// Whether a folder can take the drop: not one of the dragged items, not the folder they came
    /// from, and either one the app takes the drop on or, left to the move-or-copy rule, not
    /// read-only.
    /// Read-only is asked first: telling a folder from the items' own may make it (a dynamic
    /// folder's key), which a read-only folder left to the rule never needs.
    fn takes_drop(&self, link: &dyn FolderLink) -> bool {
        match self.app_drop(link) {
            AppDrop::NotApp => !link.is_read_only() && self.offered(link),
            AppDrop::Accepts { .. } => self.offered(link),
            AppDrop::Refuses { .. } => false,
        }
    }

    /// Whether a drop on `link` is one to ask about at all: it is not one of the dragged items
    /// and not the folder they came from.
    fn offered(&self, link: &dyn FolderLink) -> bool {
        !self.dragged_items().iter().any(|i| i.id() == link.id()) && !self.is_source(link)
    }

    /// What the app said it does with the drop on `link` ([`AppDrop::NotApp`] until asked).
    pub fn app_drop(&self, link: &dyn FolderLink) -> AppDrop {
        self.app_drops
            .iter()
            .find(|(asked, _)| same_link(asked, link))
            .map_or(AppDrop::NotApp, |(_, drop)| drop.clone())
    }

    /// Whether the app has been asked about the drop on `link` this drag.
    pub(crate) fn app_drop_asked(&self, link: &dyn FolderLink) -> bool {
        self.app_drops
            .iter()
            .any(|(asked, _)| same_link(asked, link))
    }

    /// Records what the app does with the drop on `link`.
    pub(crate) fn record_app_drop(&mut self, link: Rc<dyn FolderLink>, drop: AppDrop) {
        if !self.app_drop_asked(link.as_ref()) {
            self.app_drops.push((link, drop));
        }
    }

    /// Why the folder under the drag refuses it, while it is over one the app refuses.
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    pub(crate) fn set_refusal(&mut self, refusal: Option<String>) {
        if refusal != self.refusal {
            agg_gui::animation::request_draw();
        }
        self.refusal = refusal;
    }

    /// The folders a drop at `at` could ask the app about: the tree row or listed folder there,
    /// and the folder showing (path depth `showing`) when the list opened it for this drag.
    pub(crate) fn folders_at(&self, at: Point, showing: usize) -> Vec<Rc<dyn FolderLink>> {
        let tree = self.tree_target_at(at);
        let mut links: Vec<Rc<dyn FolderLink>> = match tree {
            Some(tree) => vec![Rc::clone(&tree.link)],
            None => self
                .targets
                .iter()
                .filter(|t| t.key.kind == DropTargetKind::Content && contains(&t.rect, at))
                .filter(|_| self.list_rect.is_none() || self.list_contains(at))
                .map(|t| Rc::clone(&t.link))
                .collect(),
        };
        if self.source_depth != showing && self.list_contains(at) {
            links.extend(self.spring.showing_folder(showing));
        }
        links
    }

    /// Why the folder a drop at `at` lands on refuses it, when the app refuses it there: the
    /// tree row or listed folder there, else the folder showing when the list opened it for this
    /// drag and `at` is on its empty space.
    pub(crate) fn refusal_at(&self, at: Point, showing: usize) -> Option<String> {
        let refused = |link: &dyn FolderLink| match self.app_drop(link) {
            AppDrop::Refuses { reason } if self.offered(link) => Some(reason),
            _ => None,
        };
        if let Some(tree) = self.tree_target_at(at) {
            return refused(tree.link.as_ref());
        }
        if self.list_rect.is_some() && !self.list_contains(at) {
            return None;
        }
        let listed = self
            .targets
            .iter()
            .find(|t| t.key.kind == DropTargetKind::Content && contains(&t.rect, at));
        match listed {
            Some(t) => refused(t.link.as_ref()),
            None if self.source_depth != showing => self
                .spring
                .showing_folder(showing)
                .and_then(|link| refused(link.as_ref())),
            None => None,
        }
    }

    /// Whether `link` opens the folder the drag came from, its key asked once per drag.
    fn is_source(&self, link: &dyn FolderLink) -> bool {
        let Some(source) = &self.source_key else {
            return false;
        };
        let mut keys = self.target_keys.borrow_mut();
        let key = match keys.iter().find(|(id, _)| id == link.id()) {
            Some((_, key)) => key.clone(),
            None => {
                let key = FolderKey::of_link(link);
                keys.push((link.id().to_string(), key.clone()));
                key
            }
        };
        key.as_ref() == Some(source)
    }

    /// The tree row at `at`, when `at` is inside the tree's rectangle (which clips its rows).
    fn tree_target_at(&self, at: Point) -> Option<&DropTarget> {
        if self.tree_rect.is_some_and(|r| !contains(&r, at)) {
            return None;
        }
        self.targets
            .iter()
            .find(|t| t.key.kind == DropTargetKind::Tree && contains(&t.rect, at))
    }

    /// The target the drop at `at` lands on: a tree row first, then a folder in the list (only
    /// inside the list's rectangle, which clips its rows; a tree row likewise only inside the tree's); `None` over anything else, or over a
    /// folder that can't take it.
    fn target_at(&self, at: Point) -> Option<&DropTarget> {
        let tree = self.tree_target_at(at);
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
        let tree = self.tree_target_at(at);
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
        self.spring
            .showing_folder(showing)
            .filter(|link| !matches!(self.app_drop(link.as_ref()), AppDrop::Refuses { .. }))
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
