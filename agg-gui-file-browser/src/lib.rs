//! `agg-gui-file-browser`: the widgets of agg-gui's file browser, over the folder and item model
//! in `agg-gui-files`.
//!
//! Rules every module follows:
//!
//! - **The look is the app's.** Every colour, size, icon and widget name a widget draws or
//!   carries comes from one [`BrowserLook`]: [`BrowserStyle`] for colours and sizes,
//!   [`BrowserLook::icon`] for artwork (else the icon's Font Awesome glyph, [`BrowserIcon`]) and
//!   [`BrowserNames`] for the names automation, tests and help articles find widgets by.
//!   [`DefaultLook`] is agg-gui's theme with Font Awesome glyphs.
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
//!   browser's [`Builtins`] placed in it, and what activating a row does), [`OutsideDrops`]
//!   (drags onto the app's other views), [`PlaceRules`] (move or copy, and copies the app runs:
//!   items move within a place and are copied between places, out of folders that keep their
//!   items, or with the copy modifier held), [`BrowserMemory`] (views per folder, the tree's
//!   width, last locations, in the app's own store and keys) and [`BrowserDialogs`]; plus a
//!   report for errors and its [`BrowserOptions`]. Every hook has a default ([`NoPictures`],
//!   [`NoActions`], [`DefaultPlaces`], [`NoMemory`], [`NoDialogs`]), so
//!   `BrowserConfig::default()` is a working browser; the defaults never pretend: a browser
//!   without dialogs leaves the actions needing one unavailable.
//! - **No hook is called while the browser's state is borrowed.** Hooks are handed owned
//!   snapshots ([`Entry`], [`BrowserRef`]) or plain values, or are run from the UI queue, so a
//!   hook that calls back into the browser never meets a held borrow. The one hook lent part of
//!   the state, [`BrowserMemory::remember_location`] (the open path from the root), is handed no
//!   way to reach the browser.
//! - **Tracing is the app's.** The browser's trace lines go through [`trace`] to the tracer the
//!   app installed ([`set_tracer`]), built only while it is on.
//!
//! Modules (each item is also re-exported at the root): `look` (the look and its defaults),
//! `fitted_text` (fitting names to their room), `link_label` (a text link), `icon_button` (a
//! square icon button), `bread_crumb` (the breadcrumb trail and its builder); `config` (the
//! config and its options), `entry` (what hooks are handed), `pictures`, `actions`, `drops`,
//! `places`, `memory` and `dialogs` (the hooks and their defaults), `local_spawner` (the
//! spawner for apps without an executor), `trace` (the opt-in trace through the app's tracer).
//!
//! The widgets come from MatterCAD's library (mattercad-rust's `mattercad-app`, same author),
//! published here under agg-gui's MIT licence.

pub mod actions;
pub mod bread_crumb;
pub mod config;
pub mod dialogs;
pub mod drops;
pub mod entry;
pub mod fitted_text;
pub mod icon_button;
pub mod link_label;
pub mod local_spawner;
pub mod look;
pub mod memory;
pub mod pictures;
pub mod places;
pub mod trace;

pub use actions::{
    Activation, BrowserAction, BrowserActions, BrowserCommand, BuiltinAction, Builtins, NoActions,
};
pub use bread_crumb::{folder_bread_crumb_for, FolderBreadCrumbWidget, UP_ICON_SIZE};
pub use config::{
    BrowserConfig, BrowserConfigBuilder, BrowserLayout, BrowserOptions, DoubleClickTarget,
    ErrorReport, FolderFilter,
};
pub use dialogs::{BrowserDialogs, Confirmation, DialogKind, NameRequest, NoDialogs, PickedFiles};
pub use drops::{OutsideDrops, OutsideOver};
pub use entry::{BrowserRef, Entry};
pub use fitted_text::{
    centered_text_baseline, fill_text_within, fit_keeping_extension, fit_name, fit_name_with,
    fit_text, name_extension, MIN_STEM_CHARS,
};
pub use icon_button::BrowserIconButton;
pub use link_label::LinkLabel;
pub use local_spawner::{run_ready_local_tasks, unfinished_local_tasks, ErrorLog, LocalSpawner};
pub use look::{glyph_face, BrowserIcon, BrowserLook, BrowserNames, BrowserStyle, DefaultLook};
pub use memory::{
    remember_open_location, BrowserMemory, LocationFuture, NoMemory, ViewMode, ViewState,
};
pub use pictures::{FolderPicture, ItemPictures, NoPictures, PictureCx, PictureState};
pub use places::{
    copy_modifier_held, default_crosses_places, default_drop_copies, places_differ, CopyCx,
    CopyTakeover, DefaultPlaces, PlaceRules,
};
pub use trace::{set_tracer, trace, tracing};

/// Every English literal this crate hands to `agg_gui_files::tr`.
pub const ENGLISH_TEXTS: &[&str] = &["Click to go back"];
