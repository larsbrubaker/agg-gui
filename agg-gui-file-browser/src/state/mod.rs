//! [`BrowserState`]: what a file browser is looking at and what it does, shared by its rows,
//! breadcrumbs and menus. It holds the open path (the showing folder with the folders above it,
//! which the breadcrumbs show), each folder shared ([`OpenFolder`]) so a hook can be handed the
//! path without the state being borrowed; the selected rows; how the list shows the folder (its
//! [`ViewMode`], sort and "Show Folders"); the showing folder's listing (folders and items each
//! sorted on their own, the options' filters applied); and the app's own data for this browser
//! (the app slot, opaque here); and the rows' menu button and press state
//! (`list/menu_button.rs`, `list/press.rs`), kept here so they survive the list being rebuilt
//! under the pointer.
//!
//! Only the showing folder is activated and listened to for content changes
//! (`state/active_watch.rs`); `sync_content` turns a change into a redisplay and
//! `pump_live_refresh` gives the showing folder its turn. A search's results are pushed as one
//! more folder (`state/search.rs`), so the breadcrumbs show them under the folder searched.
//! Writes change the folders on the path through `container_at_mut`, then `after_write`.
//!
//! This file holds the state itself, its listing and its counters. Children: `navigation` (the
//! open path and moving along it, the container-changed listeners, remembered places), `view`
//! (the view mode, sort and "Show Folders", remembered per folder), `browser_entry` (a row and
//! its identity), `selection` (the selection and the row clicks, and which listing a row's index
//! means), `listing_message` (a folder listing only a message), `gesture` (Shift ranges, the
//! rubber band, keeping a multi-selection on a press, the keyboard's moves), `search` (search
//! results in sections), `activate` (double clicks and taps, the activation queue `run_hooks`
//! drains, the config, the report and the app slot) and `active_watch` (the hold on the showing
//! folder: listening, relisting on the user's ask, handing it over).
//!
//! No hook runs while the state is borrowed (the crate header's rule): the public ways to
//! activate a row run `run_hooks` once their borrow has ended, and `remember_location` hands the
//! memory a copy of the path's handles with the state free. The exception the crate header
//! names: the memory's `view`/`set_view` (handed the path's names alone) and the error report
//! (a message) may run while the state is borrowed.

use std::any::Any;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

use agg_gui_files::{Folder, FolderLink, Item, PictureFuture, SearchResultsFolder, SortBehavior};

use crate::actions::Activation;
use crate::config::{BrowserConfig, DoubleClickTarget, FolderFilter, ItemFilter};
use crate::entry::{BrowserRef, Entry};
use crate::list::menu_button::MenuButtonState;
use crate::list::press::ItemPress;
use crate::memory::ViewMode;

mod activate;
mod active_watch;
mod browser_entry;
mod gesture;
mod listing_message;
mod navigation;
mod search;
mod selection;
mod view;

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
    /// The options' item filter (`listing_message.rs`'s `keeps_item` applies it).
    item_filter: Option<ItemFilter>,
    /// Whether several rows can be selected (`state/gesture.rs`).
    multi_select: bool,
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
    /// The drag of the browser's items onto its folders (`drag/`).
    pub(crate) drop: crate::drag::DropManager,
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
            item_filter: options.item_filter,
            multi_select: options.multi_select,
            item_events: (options.double_click == DoubleClickTarget::Host).then(Vec::new),
            config,
            app: Box::new(app),
            pending_activations: Vec::new(),
            tap_activates: options.tap_activates,
            tap_pending: None,
            finger_tap_opens_folders: options.finger_tap_opens_folders,
            menu_button: MenuButtonState::default(),
            item_press: ItemPress::default(),
            drop: crate::drag::DropManager::default(),
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

    /// The showing folder's rows: its folders (unless "Show Folders" is off, and those the
    /// options' folder filter keeps), then its items (those the item filter keeps), the visible
    /// ones only, each group sorted.
    pub fn entries(&self) -> Vec<BrowserEntry> {
        if let Some(results) = self.showing_search_results() {
            return search::listing_kept(results, self.sort, &|i| self.keeps_item(i)).1;
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
            .filter(|item| item.is_visible() && self.keeps_item(item.as_ref()))
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

    /// The one selected row's item, a folder row as its link.
    pub fn single_selected_item(&self) -> Option<Rc<dyn Item>> {
        match self.selected_entries().as_slice() {
            [entry] => Some(entry.item()),
            _ => None,
        }
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

    /// After a write changed a folder: the selection is cleared and the listing shown again;
    /// `status` is the failure to show, empty for none.
    pub fn after_write(&mut self, status: String) {
        self.writes += 1;
        self.selected.clear();
        self.set_status(status);
        self.redisplay();
    }
}

/// When the browser closes, the showing folder is deactivated (a folder following something
/// while shown stops, even if something else still holds it).
impl Drop for BrowserState {
    fn drop(&mut self) {
        self.watch.leave(&self.path);
    }
}
