//! [`BrowserState`]: what a file browser is looking at and what it does, shared by its rows,
//! breadcrumbs and menus. It holds the open path (the showing folder with the folders above it,
//! which the breadcrumbs show), each folder shared ([`OpenFolder`]) so a hook can be handed the
//! path without the state being borrowed; the selected rows; how the list shows the folder (its
//! [`ViewMode`], restored from the folder's remembered view whenever a folder shows and
//! remembered when the user picks one, through the config's `BrowserMemory`; the sort, the
//! folder's `default_sort` whenever it shows, then its remembered sort; folders and items each
//! sorted on their own; and "Show Folders", on by default and not remembered, so a folder never
//! seems empty because its sub-folders were hidden last time); and the app's own data for this
//! browser (the app slot, opaque here); and the rows' menu button and press state
//! (`list/menu_button.rs`, `list/press.rs`), kept here so they survive the list being rebuilt
//! under the pointer.
//!
//! Every change of the showing folder bumps a navigation counter and tells the container-changed
//! listeners (how a search learns the user walked away from its results). Only the showing folder
//! is activated and listened to for content changes (`state/active_watch.rs`); `sync_content`
//! turns a change into a redisplay and `pump_live_refresh` gives the showing folder its turn. A
//! search's results are pushed as one more folder (`state/search.rs`), so the breadcrumbs show
//! them under the folder searched.
//! Writes change the folders on the path through `container_at_mut`, then `after_write`.
//!
//! Children: `browser_entry` (a row and its identity), `selection` (the selection and the row
//! clicks, and which listing a row's index means), `listing_message` (a folder listing only a
//! message), `gesture` (Shift ranges, the rubber band, keeping a multi-selection on a press, the
//! keyboard's moves), `search` (search results in sections), `activate` (double clicks and taps, the activation queue `run_hooks`
//! drains, the config, the report and the app slot) and `active_watch`.
//!
//! No hook runs while the state is borrowed (the crate header's rule): the public ways to
//! activate a row run `run_hooks` once their borrow has ended, and `remember_location` hands the
//! memory a copy of the path's handles with the state free. The exception the crate header
//! names: the memory's `view`/`set_view` (handed the path's names alone) and the error report
//! (a message) may run while the state is borrowed.

use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::rc::{Rc, Weak};

use agg_gui_files::{
    tr, Folder, FolderLink, Item, PictureFuture, SearchResultsFolder, SortBehavior, SortKey,
};

use crate::actions::Activation;
use crate::config::{BrowserConfig, DoubleClickTarget, FolderFilter};
use crate::entry::{BrowserRef, Entry};
use crate::list::menu_button::MenuButtonState;
use crate::list::press::ItemPress;
use crate::memory::{remember_open_location, ViewMode, ViewState};

mod activate;
mod active_watch;
mod browser_entry;
mod gesture;
mod listing_message;
mod search;
mod selection;

use active_watch::ActiveWatch;
pub use browser_entry::{item_identity_key, BrowserEntry};
pub use listing_message::{entries_message, is_only_a_message, ListingMessage};
pub use search::{search_listing, ListSlot, SearchSectionCollapseState, SectionHeader};

/// A folder on a browser's open path: shared, so a hook can be handed the path without the
/// browser's state being borrowed, and changeable, since loading and writing need `&mut`.
pub type OpenFolder = Rc<RefCell<Box<dyn Folder>>>;

/// A container-changed handler (see `BrowserState::subscribe_container_changed`): told the new
/// showing folder's id, dropped once it answers false.
pub type ContainerChangedListener = Rc<dyn Fn(&str) -> bool>;

/// What a browser whose double clicks go to its host hears from its list.
#[derive(Clone)]
pub enum ItemEvent {
    /// A plain click on an item row (folder rows send none).
    Clicked(BrowserEntry),
    /// A double click on an item (a folder's opens it).
    Activated(BrowserEntry),
}

/// The browser's state (see the module header).
pub struct BrowserState {
    /// The folders from the root down to the one showing.
    path: Vec<OpenFolder>,
    /// The selected rows, in the order they were selected.
    selected: Vec<BrowserEntry>,
    /// The anchor, pending release and rubber band of the selection gestures.
    gesture: gesture::SelectionGesture,
    /// The rows the list on screen was built from: what row indices mean (`state/selection.rs`).
    listed: Vec<BrowserEntry>,
    /// Whether a list view marks the listings (`mark_listed`); until one does, the state keeps
    /// `listed` current itself.
    view_lists: bool,
    /// Bumped whenever what the view shows changes, so the view rebuilds.
    generation: u64,
    /// The showing folder's content-changed subscription.
    watch: ActiveWatch,
    content_seen: u64,
    /// Bumped whenever the list shows its content anew and so starts at the top.
    list_resets: u64,
    /// The status line: what went wrong last, shared with the report a browser without one of
    /// the app's writes to, so it can be written while the state is borrowed.
    status: Rc<RefCell<String>>,
    view_mode: ViewMode,
    /// The picture side: what `view_mode` gives, or the size the list was opened with.
    thumb_size: i32,
    sort: SortBehavior,
    show_containers: bool,
    navigation: u64,
    container_changed: Vec<ContainerChangedListener>,
    /// A search's results and where on `path` they stand, while they are on it (`state/search.rs`).
    results: Option<(Rc<SearchResultsFolder>, usize)>,
    /// The identity keys of selected results whose section is rolled up.
    parked_selection: Vec<String>,
    /// Identity keys of rows to select once listed, and whether to scroll them into view.
    select_when_listed: Vec<String>,
    reveal_selection: bool,
    item_menu_requested: bool,
    /// While a folder tree's menu acts, the selection is its folder, listed at this depth.
    temporary_selection: Option<(Rc<dyn FolderLink>, usize)>,
    /// Bumped by each write (`after_write`), so a folder tree lists again.
    writes: u64,
    container_filter: Option<FolderFilter>,
    /// The clicks and double clicks a host hears (`DoubleClickTarget::Host`), else `None`.
    item_events: Option<Vec<ItemEvent>>,
    config: BrowserConfig,
    /// The app's own data for this browser.
    app: Box<dyn Any>,
    /// Activations waiting for `run_hooks`.
    pending_activations: Vec<(BrowserRef, Entry, Activation)>,
    tap_activates: bool,
    tap_pending: Option<usize>,
    finger_tap_opens_folders: bool,
    /// The rows' menu button: hover, press and where the menu it asked for hangs.
    menu_button: MenuButtonState,
    /// A press on a row or tile and whether it became a drag.
    item_press: ItemPress,
    /// This state's own handle, which snapshots carry back to it.
    me: Weak<RefCell<BrowserState>>,
}

impl BrowserState {
    /// The browser at `root` with `config` and no app data.
    pub fn new(root: Box<dyn Folder>, config: BrowserConfig) -> Rc<RefCell<Self>> {
        Self::with_config(root, config, ())
    }

    /// The browser at `root` for the view `config` is for, holding the app's data `app` in its
    /// app slot: opened on the options' picture size, their folder filter applied, double clicks
    /// going where the options say and a finger's tap opening a folder where they say so.
    pub fn with_config<A: 'static>(
        mut root: Box<dyn Folder>,
        config: BrowserConfig,
        app: A,
    ) -> Rc<RefCell<Self>> {
        let options = config.options.clone();
        let watch = ActiveWatch::default();
        watch.listen(root.as_ref());
        root.load();
        root.activate();
        let state = Rc::new(RefCell::new(Self {
            path: vec![Rc::new(RefCell::new(root))],
            selected: Vec::new(),
            gesture: Default::default(),
            listed: Vec::new(),
            view_lists: false,
            generation: 0,
            watch,
            content_seen: 0,
            list_resets: 0,
            status: Rc::default(),
            view_mode: ViewMode::for_icon_size(options.thumb_size),
            thumb_size: options.thumb_size,
            sort: SortBehavior::default(),
            show_containers: options.show_folders,
            navigation: 0,
            container_changed: Vec::new(),
            results: None,
            parked_selection: Vec::new(),
            select_when_listed: Vec::new(),
            reveal_selection: false,
            item_menu_requested: false,
            temporary_selection: None,
            writes: 0,
            container_filter: options.folder_filter,
            item_events: (options.double_click == DoubleClickTarget::Host).then(Vec::new),
            config,
            app: Box::new(app),
            pending_activations: Vec::new(),
            tap_activates: options.tap_activates,
            tap_pending: None,
            finger_tap_opens_folders: options.finger_tap_opens_folders,
            menu_button: MenuButtonState::default(),
            item_press: ItemPress::default(),
            me: Weak::new(),
        }));
        {
            let mut s = state.borrow_mut();
            s.me = Rc::downgrade(&state);
            s.restore_user_view();
            s.keep_own_listing();
        }
        state
    }

    /// The showing folder's `default_sort`, then the folder's remembered view and sort when it
    /// has one. The remembered view is compared with what this list shows, so a list opened on a
    /// size of its own keeps it unless the folder remembers a different mode.
    fn restore_user_view(&mut self) {
        let default_sort = self.active_container().and_then(|c| c.default_sort());
        if let Some(sort) = default_sort {
            self.sort = sort;
        }
        if let Some(view) = self.config.memory.view(&self.path_names()) {
            if view.view_mode != self.view_mode {
                self.view_mode = view.view_mode;
                self.thumb_size = view.view_mode.thumb_size();
            }
            self.sort = view.sort;
        }
    }

    /// The picture side the list shows.
    pub fn thumb_size(&self) -> i32 {
        self.thumb_size
    }

    /// The key and direction the list is sorted on.
    pub fn sort(&self) -> SortBehavior {
        self.sort
    }

    /// Sorts on `key` and remembers it for the folder; the list is shown again only when the
    /// sort changed.
    pub fn set_user_sort_key(&mut self, key: SortKey) {
        let changed = self.sort.sort_key != key as i32;
        self.sort.sort_key = key as i32;
        self.persist_user_view();
        if changed {
            self.redisplay();
        }
    }

    /// Sorts in that direction and remembers it for the folder; the list is shown again only
    /// when the direction changed.
    pub fn set_user_sort_ascending(&mut self, ascending: bool) {
        let changed = self.sort.ascending != ascending;
        self.sort.ascending = ascending;
        self.persist_user_view();
        if changed {
            self.redisplay();
        }
    }

    /// Remembers the view and sort this list shows for the folder.
    fn persist_user_view(&self) {
        let view = ViewState {
            view_mode: self.view_mode,
            sort: self.sort,
        };
        self.config.memory.set_view(&self.path_names(), &view);
    }

    /// How the list shows its entries.
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    /// The view menu's pick: shows the folder in `mode`, remembers it for the folder, then shows
    /// the list again.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
        self.thumb_size = mode.thumb_size();
        self.persist_user_view();
        self.redisplay();
    }

    /// Whether the list shows folders ("Show Folders").
    pub fn show_containers(&self) -> bool {
        self.show_containers
    }

    /// The view menu's "Show Folders": hides or shows the folders, then lists again.
    pub fn set_show_containers(&mut self, show: bool) {
        if self.show_containers != show {
            self.show_containers = show;
            self.selected.clear();
            self.redisplay();
        }
    }

    /// The showing folder's per-frame turn (`Folder::pump_live_refresh`); whether it still has a
    /// refresh pending, so the widget keeps waking. The widget calls this before `sync_content`.
    pub fn pump_live_refresh(&mut self) -> bool {
        self.path
            .last()
            .is_some_and(|folder| folder.borrow_mut().pump_live_refresh())
    }

    /// When the showing folder changed its rows or header since the last call, the list is shown
    /// again (`redisplay`). The widget calls this before each layout.
    pub fn sync_content(&mut self) {
        let ticks = self.watch.ticks();
        if ticks != self.content_seen {
            crate::trace(|| format!("browser content changed ticks={ticks}"));
            self.content_seen = ticks;
            self.redisplay();
        }
    }

    /// How many content changes the showing folders have raised.
    pub fn content_changes(&self) -> u64 {
        self.watch.ticks()
    }

    /// How many times the list has shown its content anew, each of which puts it back at the
    /// top. A selection changes nothing here, so the list stays where the user scrolled it.
    pub fn list_resets(&self) -> u64 {
        self.list_resets
    }

    /// Shown above the rows when not empty.
    pub fn header_markdown(&self) -> String {
        self.active_container()
            .map(|c| c.header_markdown())
            .unwrap_or_default()
    }

    /// Bumped whenever what the view shows changes.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn changed(&mut self) {
        self.generation += 1;
        crate::trace(|| {
            format!(
                "browser changed gen={} selected={:?}",
                self.generation,
                self.selected.iter().map(|e| e.name()).collect::<Vec<_>>()
            )
        });
        agg_gui::animation::request_draw();
    }

    /// The names of the folders from the root down (the breadcrumbs).
    pub fn path_names(&self) -> Vec<String> {
        self.path
            .iter()
            .map(|c| c.borrow().name().to_string())
            .collect()
    }

    /// The showing folder's rows: its folders (unless "Show Folders" is off, and those the
    /// options' folder filter keeps), then its items, the visible ones only, each group sorted.
    pub fn entries(&self) -> Vec<BrowserEntry> {
        if let Some(results) = self.showing_search_results() {
            return search_listing(results, self.sort).1;
        }
        let Some(folder) = self.active_container() else {
            return Vec::new();
        };
        let mut folders: Vec<_> = folder
            .child_containers()
            .into_iter()
            .filter(|link| self.show_containers && link.is_visible())
            .filter(|link| {
                self.container_filter
                    .as_ref()
                    .is_none_or(|f| f(link.as_ref()))
            })
            .collect();
        agg_gui_files::sort_items(&mut folders, self.sort, |link| link.as_ref() as _);
        let mut items: Vec<_> = folder
            .items()
            .into_iter()
            .filter(|item| item.is_visible())
            .collect();
        agg_gui_files::sort_items(&mut items, self.sort, |item| item.as_ref());
        let folders = folders.into_iter().map(BrowserEntry::Container);
        folders
            .chain(items.into_iter().map(BrowserEntry::Item))
            .collect()
    }

    /// The status line: what went wrong last, empty for nothing.
    pub fn status(&self) -> String {
        self.status.borrow().clone()
    }

    fn set_status(&mut self, status: String) {
        *self.status.borrow_mut() = status;
    }

    /// A row asks for the selection's menu.
    pub fn request_item_menu(&mut self) {
        crate::trace(|| "browser item menu requested".to_string());
        self.item_menu_requested = true;
    }

    /// The rows' menu button state, which outlives the rows rebuilt under the pointer.
    pub fn menu_button(&self) -> &MenuButtonState {
        &self.menu_button
    }

    /// The rows' menu button state, to change it.
    pub fn menu_button_mut(&mut self) -> &mut MenuButtonState {
        &mut self.menu_button
    }

    /// The press on a row or tile under way, which outlives the rows rebuilt under the pointer.
    pub fn item_press(&self) -> &ItemPress {
        &self.item_press
    }

    /// The press on a row or tile, to change it.
    pub fn item_press_mut(&mut self) -> &mut ItemPress {
        &mut self.item_press
    }

    /// Whether a row asked for its menu since the last ask, clearing it.
    pub fn take_item_menu_request(&mut self) -> bool {
        std::mem::take(&mut self.item_menu_requested)
    }

    /// Opens the folder. A link with nothing to give that acts instead (a sign-in) leaves the
    /// list where it is; any other refusal is said on the status line.
    pub fn open(&mut self, link: &dyn FolderLink) {
        match link.get_container() {
            Ok(folder) => self.push_container(folder),
            Err(_) if link.open_unavailable() => {}
            Err(e) => {
                self.set_status(e);
                self.changed();
            }
        }
    }

    /// Shows `folder` below the one showing, as opening its row would.
    pub fn set_active_container(&mut self, folder: Box<dyn Folder>) {
        self.push_container(folder);
    }

    /// Shows `folder` below the one showing.
    pub fn push_container(&mut self, folder: Box<dyn Folder>) {
        self.watch.leave(&self.path);
        self.show_below(folder);
    }

    /// Shows `folder` at the end of the path, the one showing already left: loaded here,
    /// activated and listened to for content changes.
    fn show_below(&mut self, mut folder: Box<dyn Folder>) {
        self.watch.listen(folder.as_ref());
        folder.load();
        folder.activate();
        self.path.push(Rc::new(RefCell::new(folder)));
        self.selected.clear();
        self.restore_user_view();
        self.changed();
        self.navigated();
    }

    /// Shows the folders `located` below the path's folder at `base` (a search's root, or the
    /// showing folder), the last of them showing: where "Open File Location" takes the user once
    /// the app has found where `item` lives. `None` (nowhere to go: a folder since renamed or
    /// deleted) is said on the status line, so the user learns why nothing happened.
    pub fn show_location(
        &mut self,
        item: &dyn Item,
        base: usize,
        located: Option<Vec<Box<dyn Folder>>>,
    ) {
        let Some(located) = located else {
            self.set_status(format!(
                "{} \"{}\"",
                tr("Couldn't find the location of"),
                item.name()
            ));
            self.changed();
            return;
        };
        if located.is_empty() && base + 1 == self.path.len() {
            return;
        }
        self.watch.leave(&self.path);
        self.path.truncate(base + 1);
        let count = located.len();
        for (i, mut folder) in located.into_iter().enumerate() {
            if i + 1 == count {
                self.watch.listen(folder.as_ref());
            }
            folder.load();
            if i + 1 == count {
                folder.activate();
            }
            self.path.push(Rc::new(RefCell::new(folder)));
        }
        if count == 0 {
            self.watch.enter(&self.path);
        }
        self.selected.clear();
        self.restore_user_view();
        self.changed();
        self.navigated();
    }

    /// The one selected row's item, a folder row as its link.
    pub fn single_selected_item(&self) -> Option<Rc<dyn Item>> {
        match self.selected_entries().as_slice() {
            [entry] => Some(entry.item()),
            _ => None,
        }
    }

    /// `listener` hears each change of the showing folder, with the new one's id, until it
    /// answers false.
    pub fn subscribe_container_changed(&mut self, listener: ContainerChangedListener) {
        self.container_changed.push(listener);
    }

    /// How many times the showing folder has changed.
    pub fn navigation(&self) -> u64 {
        self.navigation
    }

    /// The showing folder changed: count it and tell the listeners.
    fn navigated(&mut self) {
        self.navigation += 1;
        self.list_resets += 1;
        self.parked_selection.clear();
        self.select_when_listed.clear();
        if self
            .results
            .as_ref()
            .is_some_and(|(_, depth)| *depth >= self.path.len())
        {
            self.results = None;
        }
        self.keep_own_listing();
        let id = self.active_container_id().unwrap_or_default();
        let listeners = self.container_changed.clone();
        let gone: Vec<bool> = listeners.iter().map(|l| !l(&id)).collect();
        let mut index = 0;
        self.container_changed.retain(|_| {
            index += 1;
            !gone.get(index - 1).copied().unwrap_or(false)
        });
    }

    /// The showing folder's id.
    pub fn active_container_id(&self) -> Option<String> {
        self.active_container().map(|c| c.id().to_string())
    }

    /// The ids of the folders from the root down.
    pub fn path_ids(&self) -> Vec<String> {
        self.path
            .iter()
            .map(|c| c.borrow().id().to_string())
            .collect()
    }

    /// How many folders deep the browser is (1 at the root).
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// The showing folder, borrowed for as long as the answer is held.
    pub fn active_container(&self) -> Option<Ref<'_, dyn Folder>> {
        self.path
            .last()
            .map(|c| Ref::map(c.borrow(), |b| b.as_ref()))
    }

    /// The showing folder's reading of `item`'s picture. A search's results route an item to
    /// the folder it was found in; an item of the searched folder's own listing has no such
    /// origin, so the folder under the results is asked, and the result shows the same picture
    /// as in the folder.
    pub fn container_thumbnail(
        &self,
        item: &Rc<dyn Item>,
        width: i32,
        height: i32,
    ) -> Option<PictureFuture> {
        match self.showing_search_results() {
            Some(results) if results.try_get_origin(item).is_none() && self.path.len() >= 2 => self
                .path[self.path.len() - 2]
                .borrow()
                .get_thumbnail(item, width, height),
            _ => self.active_container()?.get_thumbnail(item, width, height),
        }
    }

    /// The root's folders, what a folder tree lists.
    pub fn root_child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        self.path
            .first()
            .map(|root| root.borrow().child_containers())
            .unwrap_or_default()
    }

    /// The breadcrumb for path entry `depth` (0 = the root): shows that folder again.
    pub fn go_to(&mut self, depth: usize) {
        if depth + 1 < self.path.len() {
            self.watch.leave(&self.path);
            self.path.truncate(depth + 1);
            self.watch.enter(&self.path);
            self.selected.clear();
            self.restore_user_view();
            self.changed();
            self.navigated();
        }
    }

    /// Takes the showing folder out of the path, the one above it showing again: a dialog that
    /// hands its folder to the work it closes on gives it away rather than opening a second copy
    /// of it. `None` at the root.
    pub fn take_active_container(&mut self) -> Option<OpenFolder> {
        if self.path.len() < 2 {
            return None;
        }
        self.watch.leave(&self.path);
        let taken = self.path.pop();
        self.watch.enter(&self.path);
        self.selected.clear();
        self.changed();
        self.navigated();
        taken
    }

    /// The folders from the root down to the showing one, each borrowed while it is held.
    pub fn path_containers(&self) -> impl Iterator<Item = Ref<'_, dyn Folder>> {
        self.path
            .iter()
            .map(|c| Ref::map(c.borrow(), |b| b.as_ref()))
    }

    /// The open path's shared folders, from the root down: what a hook is handed.
    pub fn open_path(&self) -> Vec<OpenFolder> {
        self.path.clone()
    }

    /// The folder at `depth` (0 = the root), to change it.
    pub fn container_at_mut(&mut self, depth: usize) -> Option<RefMut<'_, dyn Folder + 'static>> {
        self.path
            .get(depth)
            .map(|c| RefMut::map(c.borrow_mut(), |b| b.as_mut()))
    }

    /// Sets (or with `None` ends) a folder tree's temporary selection: `link`, listed in the
    /// folder at `depth`. Answers the one it replaces.
    pub fn set_temporary_selection(
        &mut self,
        selection: Option<(Rc<dyn FolderLink>, usize)>,
    ) -> Option<(Rc<dyn FolderLink>, usize)> {
        std::mem::replace(&mut self.temporary_selection, selection)
    }

    /// A folder tree's temporary selection, if one is set.
    pub fn temporary_selection(&self) -> Option<&(Rc<dyn FolderLink>, usize)> {
        self.temporary_selection.as_ref()
    }

    /// How many writes have run.
    pub fn writes(&self) -> u64 {
        self.writes
    }

    /// The showing folder is replaced by what `sync` makes of it (a folder renamed from the
    /// tree). Nothing at the root. While a hook still holds the showing folder it can't be
    /// replaced: it stays, and the user is told through the report (the status line without the
    /// app's) that it shows as it was and how to see the change.
    pub fn replace_active_container(
        &mut self,
        sync: impl FnOnce(Box<dyn Folder>) -> Box<dyn Folder>,
    ) {
        if self.path.len() < 2 {
            return;
        }
        self.watch.leave(&self.path);
        let Some(old) = self.path.pop() else {
            return;
        };
        match Rc::try_unwrap(old) {
            Ok(old) => self.show_below(sync(old.into_inner())),
            Err(still_held) => {
                self.path.push(still_held);
                self.watch.enter(&self.path);
                let message = tr("This folder is busy. Open it again to see the change.");
                (self.report())(&message);
                self.changed();
            }
        }
    }

    /// After a write changed a folder: the selection is cleared and the listing shown again;
    /// `status` is the failure to show, empty for none.
    pub fn after_write(&mut self, status: String) {
        self.writes += 1;
        self.selected.clear();
        self.set_status(status);
        self.redisplay();
    }

    /// Lends the config's memory the open path, the root first, to remember under `key`, with
    /// `state` not borrowed while it runs.
    pub fn remember_location(state: &Rc<RefCell<Self>>, key: &str) {
        let (memory, path) = {
            let s = state.borrow();
            (Rc::clone(&s.config.memory), s.open_path())
        };
        remember_open_location(memory.as_ref(), key, &path);
    }
}

/// When the browser closes, the showing folder is deactivated (a folder following something
/// while shown stops, even if something else still holds it).
impl Drop for BrowserState {
    fn drop(&mut self) {
        self.watch.leave(&self.path);
    }
}
