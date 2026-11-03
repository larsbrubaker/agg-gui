//! `agg-gui-file-browser`: the widgets of agg-gui's file browser, over the folder and item model
//! in `agg-gui-files`.
//!
//! Rules every module follows:
//!
//! - **The look is the app's.** Every colour, size, icon and widget name a widget draws or
//!   carries comes from one [`BrowserLook`]: [`BrowserStyle`] for colours and sizes,
//!   [`BrowserLook::icon`] for artwork (else the icon's Font Awesome glyph, [`BrowserIcon`]; tinted
//!   through `tinted_icon`, an item's icon keys through `icon_for_key`, and asked for again when
//!   `icon_generation` changes) and
//!   [`BrowserNames`] for the names automation, tests and help articles find widgets by. A look
//!   may also hand over its app's own widgets for a few pieces (the bars' menu, overflow and
//!   search pieces, a search section's toggle, a message's button), which the browser places and
//!   feeds in its own widgets' stead. [`DefaultLook`] is agg-gui's theme with Font Awesome
//!   glyphs.
//! - **Text the user reads** goes through `agg_gui_files::tr`, the translation hook an app points
//!   at its own translations once; every English literal this crate hands to `tr` is in
//!   [`ENGLISH_TEXTS`], so the app can check its translations cover them (a test keeps the list
//!   and the literals the same).
//! - **Names are fitted, never overdrawn.** A name drawn in a room too small for it is cut with
//!   [`fit_name`] (keeping a file's extension) or [`fit_text`] and clipped with
//!   [`fill_text_within`]; the whole name stays the widget's tooltip.
//! - **The app's part comes through one config.** A browser view takes a [`BrowserConfig`]: its
//!   spawner (where work nobody awaits runs: the app's executor, or a [`LocalSpawner`] pumped by
//!   agg-gui's UI-queue drain), its look, and the hooks through which the app adds its own:
//!   [`ItemPictures`] (pictures and tile overlays), [`BrowserActions`] (the ordered menu, with the
//!   browser's [`Builtins`] placed in it, and what activating a row does), [`BrowserMenus`]
//!   (who shows the right-click menus: the app's own menu host, or the browser's), [`OutsideDrops`]
//!   (drags onto the app's other views), [`PlaceRules`] (move or copy, and copies the app runs:
//!   items move within a place and are copied between places, out of folders that keep their
//!   items, or with the copy modifier held), [`BrowserMemory`] (views per folder, the tree's
//!   width, last locations, in the app's own store and keys) and [`BrowserDialogs`]; plus a
//!   report for errors (without one, errors show on the browser's own status line,
//!   [`BrowserState::status`]) and its [`BrowserOptions`]. Every hook has a default ([`OwnPictures`],
//!   [`NoActions`], [`OwnMenus`], [`DefaultPlaces`], [`NoMemory`], [`NoDialogs`]), so
//!   `BrowserConfig::default()` is a working browser; the defaults never pretend: a browser
//!   without dialogs leaves the actions needing one unavailable ([`SheetDialogs`] is a ready set
//!   on agg-gui's `ModalSheet` an app can give instead).
//! - **No hook is called while the browser's state is borrowed.** Hooks are handed owned
//!   snapshots ([`Entry`], [`BrowserRef`]) or plain values, or are run from the UI queue, so a
//!   hook that calls back into the browser never meets a held borrow; [`BrowserState`]'s public
//!   ways to activate a row take the shared state and run the hooks once its borrow has ended.
//!   [`BrowserMemory::remember_location`] is handed the open path's shared folders, the state not
//!   borrowed. A drop's `PlaceRules` and `OutsideDrops` calls run with the state let go too. The
//!   one exception: the memory's `view`/`set_view`, the error report and the options' folder and
//!   item filters may run while the state is borrowed, since they are handed only names, a
//!   message or a row, and no way to reach the browser.
//! - **Tracing is the app's.** The browser's trace lines go through [`trace()`] to the tracer the
//!   app installed ([`set_tracer`]), built only while it is on.
//!
//! Modules (each item is also re-exported at the root): `look` (the look and its defaults),
//! `fitted_text` (fitting names to their room), `link_label` (a text link), `icon_button` (a
//! square icon button), `bread_crumb` (the breadcrumb trail and its builder); `config` (the
//! config and its options), `entry` (what hooks are handed), `pictures`, `actions`, `drops`,
//! `places`, `memory` and `dialogs` (the hooks and their defaults), `builtins` (the browser's own
//! menu actions, which run: Add, Create Folder..., Open File Location, Rename, Move, Remove, each
//! recorded for Undo and asking through the dialogs), `sheet_dialogs` ([`SheetDialogs`] and the
//! [`DialogSheet`] they show on), `move_dialog` ([`MovePage`] over a [`BrowserPage`] of its own,
//! [`MoveController`] its logic for an app's own page, and [`move_verdict`]: where a Move can go
//! and whether it moves or copies), `save_dialog` ([`SavePage`] over [`SaveController`]: where a
//! save goes and its name; the app writes the file), `open_dialog`
//! ([`OpenPage`]: the files to open, for an app with no system Open dialog), `disk_monitor`
//! (feature `disk`: the directory monitor that wakes the UI), `local_spawner` (the
//! spawner for apps without an executor), `trace` (the opt-in trace through the app's tracer);
//! `drag` ([`DropManager`], the drag of the browser's items onto its folders: targets recorded
//! as they paint, the highlight and label, the move or copy the drop runs with its history step,
//! copies offered to the app first); `state` ([`BrowserState`], what a browser is looking at: the open path, the listing, view and
//! sort, the selection and its gestures, activation, the status line and the app slot);
//! `own_pictures` (the default pictures: each item's own); `list` ([`BrowserList`], the rows or
//! tiles of the showing folder, or its message, with their gestures and menus); `tree`
//! ([`BrowserTree`], the folder tree over the same state, its rows as data in [`TreeModel`], each
//! row known by its folder's location key or else its name, and its menu); `search` (the search
//! box and its controller); `nav` (the view and sorting menus, Undo and Redo, the overflow
//! "File" button, the splitter, the provider message); `history`
//! ([`BrowserHistory`], the undo history shared by an app's browser views, steps recorded at once
//! or later through a [`HistoryRecorder`], an undo or redo finishing later holding the next one
//! back, the keys, and the [`BrowserNotice`] announcing each change with its Undo or Redo); `browser` ([`FileBrowser`], the whole browser composed from these over one state: the tree
//! and list form an app's browsing tab shows, or the list-only form of a popup, with its nav
//! bar, search box kept across header rebuilds, notice, and the drag's picture drawn once for
//! the whole browser).
//!
//! The widgets come from MatterCAD's library (mattercad-rust's `mattercad-app`, same author),
//! published here under agg-gui's MIT licence.

pub mod actions;
pub mod bread_crumb;
pub mod browser;
pub mod builtins;
pub mod config;
pub mod dialogs;
#[cfg(feature = "disk")]
pub mod disk_monitor;
pub mod drag;
pub mod drops;
pub mod entry;
pub mod fitted_text;
pub mod history;
pub mod icon_button;
pub mod link_label;
pub mod list;
pub mod local_spawner;
pub mod look;
pub mod memory;
pub mod menus;
pub mod move_dialog;
pub mod nav;
pub mod open_dialog;
pub mod own_pictures;
pub mod pictures;
pub mod places;
pub mod save_dialog;
pub mod search;
pub mod sheet_dialogs;
pub mod state;
pub mod trace;
pub mod tree;

pub use actions::{
    ActionScope, Activation, BrowserAction, BrowserActions, BrowserCommand, BuiltinAction,
    Builtins, ItemLocation, NoActions, SignInOffer,
};
pub use bread_crumb::{folder_bread_crumb_for, FolderBreadCrumbWidget, UP_ICON_SIZE};
pub use browser::parts::{take_search_box, BottomBorder, ListColumn, SearchSlot};
pub use browser::{
    FileBrowser, BACK_MOUSE_BUTTON, COMPACT_VIEW, NOTICE_BOTTOM_GAP, SEARCH_BOX_WIDTH,
};
pub use builtins::run_builtin;
pub use config::{
    BrowserConfig, BrowserConfigBuilder, BrowserLayout, BrowserOptions, DoubleClickTarget,
    ErrorReport, FolderFilter, ItemFilter,
};
pub use dialogs::{
    append_extension, extension_filter, without_extension, BrowserDialogs, Confirmation, CopiesTo,
    DialogKind, FileNamer, MovePick, MoveRequest, NameRequest, NoDialogs, OpenPick, OpenRequest,
    PickedFiles, PickedMoveTarget, PickedOpenTarget, PickedSaveTarget, ReplaceQuestion,
    ReplaceQuestionHook, SavePick, SaveRequest, SaveTarget, ShownName,
};
#[cfg(feature = "disk")]
pub use disk_monitor::{register_directory_monitor, WakingDirectoryMonitor};
pub use drag::badge::{paint_plus_badge, painted_rect, register_painted};
pub use drag::execute::{couldnt_copy_to, drop_into, drop_onto};
pub use drag::gesture::{drag_end, drag_lost, drag_over};
pub use drag::picture::{DragPicture, CARRIED_ALPHA, PICTURE_SIDE};
pub use drag::spring::{SpringOpen, SPRING_DELAY};
pub use drag::{DropKey, DropLabel, DropManager, DropTargetKind, LIST_DROP_KEY};
pub use drops::{OutsideDrops, OutsideOver};
pub use entry::{BrowserRef, Entry};
pub use fitted_text::{
    centered_text_baseline, fill_text_within, fit_keeping_extension, fit_name, fit_name_with,
    fit_text, name_extension, MIN_STEM_CHARS,
};
pub use history::notice::{
    notice_alpha, BrowserNotice, NoticeButton, NOTICE_FADE_MS, NOTICE_HOLD_MS,
};
pub use history::run::history_key_event;
pub use history::{BrowserHistory, HistoryRecorder, Notice, NoticeAction};
pub use icon_button::BrowserIconButton;
pub use link_label::LinkLabel;
pub use list::icon_view::{BrowserTile, TileStack};
pub use list::list_menu::{menu_actions, select_rows, MenuScope, MENU_ICON_SIZE};
pub use list::menu_button::MenuButtonState;
pub use list::message::FolderMessage;
pub use list::overlay::OverlayRoute;
pub use list::press::{ItemPress, DRAG_THRESHOLD};
pub use list::row::{BrowserRow, RowStack, ROW_HEIGHT, ROW_THUMB_SIZE};
pub use list::section_header::SectionHeaderRow;
pub use list::tile_art::TileArt;
pub use list::{BrowserList, LIVE_REFRESH_WAKE};
pub use local_spawner::{run_ready_local_tasks, unfinished_local_tasks, ErrorLog, LocalSpawner};
pub use look::{glyph_face, BrowserIcon, BrowserLook, BrowserNames, BrowserStyle, DefaultLook};
pub use memory::{
    remember_open_location, BrowserMemory, LocationFuture, NoMemory, ViewMode, ViewState,
};
pub use menus::{BrowserMenus, MenuSource, OwnMenus};
pub use move_dialog::{
    dialog_options, move_verdict, BrowserPage, FolderKey, MoveAnswers, MoveController, MovePage,
    MoveVerdict, MOVE_ITEM_SUMMARY, MOVE_ITEM_WINDOW,
};
pub use nav::buttons::{
    history_key_text, sorting_button, sorting_control, view_options_button, view_options_control,
    HistoryButtons, HISTORY_GAP, NAV_BUTTON_SIDE,
};
pub use nav::menu_button::{NavMenuButton, NavMenuRow, NavMenuRows};
pub use nav::overflow::{overflow_control, overflow_rows, OverflowButton, OverflowRows};
pub use nav::provider_message::{ProviderMessageBar, PROVIDER_MESSAGE_SIZE};
pub use nav::splitter::{BrowserSplitter, DEFAULT_TREE_WIDTH, MIN_TREE_WIDTH, SPLITTER_WIDTH};
pub use open_dialog::{OpenAnswers, OpenController, OpenPage, OPEN_SUMMARY, OPEN_WINDOW};
pub use own_pictures::{OwnPictures, KEPT_PICTURES};
pub use pictures::{
    FolderPicture, ItemPictures, NoPictures, OverlayFrame, PictureCx, PictureKind, PictureState,
};
pub use places::{
    copy_modifier_held, default_crosses_places, default_drop_copies, places_differ, CopyCx,
    CopyTakeover, DefaultPlaces, MoveCx, MoveTakeover, PlaceRules,
};
pub use save_dialog::{
    QuestionHost, SaveAnswers, SaveController, SaveFolderRule, SaveMode, SavePage, SAVE_AS_WINDOW,
};
pub use search::{SearchBox, SearchController, SearchEndedListener, SearchMaker, DEBOUNCE};
pub use sheet_dialogs::{DialogSheet, SheetDialogs};
pub use state::{
    entries_message, is_only_a_message, item_identity_key, search_listing, BrowserEntry,
    BrowserState, ContainerChangedListener, ItemEvent, ListSlot, ListingMessage, OpenFolder,
    RestoreFinishedListener, RestoreOutcome, SearchSectionCollapseState, SectionHeader,
};
pub use trace::{set_tracer, trace, tracing};
pub use tree::menu::tree_menu_rows;
pub use tree::{
    path_keys, BrowserTree, PathLevel, TreeKey, TreeModel, TreeNode, TreePlace, TreeRow,
    TREE_ICON_SIZE, TREE_ROW_HEIGHT,
};

/// Every English literal this crate hands to `agg_gui_files::tr`.
pub const ENGLISH_TEXTS: &[&str] = &[
    "(Moved: {0})",
    "Add",
    "Are you sure you want to remove the currently selected items?",
    "Ascending",
    "Cancel",
    "Choose a folder you can add to.",
    "Choose a folder you can save in.",
    "Choose a file to open.",
    "Choose files to open.",
    "Clear",
    "Click to go back",
    "Confirm Save As",
    "Copy",
    "Copy to {0}",
    "Copy {0} items to {1}. The originals stay in {2}.",
    "Copy {0} to {1}. The original stays in {2}.",
    "Couldn't copy to \"{0}\": {1}",
    "Couldn't find the location of \"{0}\"",
    "Couldn't move to \"{0}\": {1}",
    "Couldn't put back: {0}",
    "Create",
    "Create Folder",
    "Create Folder...",
    "Date Created",
    "Date Modified",
    "Descending",
    "Folder Name",
    "Move",
    "Move to Recycle Bin",
    "Move to Recycle Bin?",
    "Move to Trash",
    "Move to Trash?",
    "Move to {0}",
    "Move {0} items to {1}.",
    "Move {0} to {1}.",
    "Name",
    "No results",
    "Open",
    "Open File Location",
    "Protected items can't be moved",
    "Redo didn't finish: {0}",
    "Redo",
    "Redone: {0}",
    "Remove",
    "Remove Items?",
    "Rename",
    "Rename Item",
    "Replace",
    "Save",
    "Save As",
    "Search",
    "Searching for '{0}'",
    "Select a new Destination:",
    "Show Folders",
    "Sorting",
    "The folder was busy, so nothing moved. Try again.",
    "The folder was busy, so nothing was saved. Try again.",
    "The selected items will go to the Recycle Bin, where you can restore them.",
    "The selected items will go to the Trash, where you can restore them.",
    "These items are already in {0}.",
    "This folder can't be changed",
    "This folder is busy. Open it again to see the change.",
    "Undo didn't finish: {0}",
    "Undo",
    "Undone: {0}",
    "View Icons",
    "View Large Icons",
    "View List",
    "View Settings",
    "\"{0}\" already exists.\nDo you want to replace it?",
    "{0} can't go inside itself. Choose another folder.",
    "{0} is already in {1}.",
];
