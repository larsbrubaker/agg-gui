//! What a browser remembers between runs, through the app's own store: how each folder is shown
//! ([`ViewState`]: its [`ViewMode`] and sort), the width of the folder tree beside the list, and
//! the last folder a browser was in under a key the app picks ([`BrowserMemory`]). The app keeps
//! its own keys and stored shapes; the browser only hands over and takes back values.
//! [`NoMemory`] remembers nothing: every folder opens on the browser's defaults.
//!
//! [`ViewMode`]'s numbers are the ones stored view states hold (row view 0, icon views 1 to 4),
//! as `agg_gui_files::SortKey`'s numbers are, so an app storing them keeps reading what it
//! stored.

use std::future::Future;
use std::pin::Pin;

use agg_gui_files::{Folder, SortBehavior};

use crate::state::OpenFolder;

/// How a folder's entries are shown: in rows, or as icons of one of four sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// Rows: a small picture beside each name and its details.
    Rows = 0,
    /// Icons of the usual size (100).
    Icons = 1,
    /// Tiny icons (18).
    Icons18 = 2,
    /// Small icons (70).
    Icons70 = 3,
    /// Large icons (256).
    Icons256 = 4,
}

impl ViewMode {
    /// Every mode, in stored-number order.
    pub const ALL: [ViewMode; 5] = [
        ViewMode::Rows,
        ViewMode::Icons,
        ViewMode::Icons18,
        ViewMode::Icons70,
        ViewMode::Icons256,
    ];

    /// The mode an icon view of `thumb_size` reads as. A size with no mode of its own (a dialog
    /// opened on 75) reads as the usual icon view.
    pub fn for_icon_size(thumb_size: i32) -> Self {
        match thumb_size {
            18 => ViewMode::Icons18,
            70 => ViewMode::Icons70,
            256 => ViewMode::Icons256,
            _ => ViewMode::Icons,
        }
    }

    /// The picture side the mode shows (the row view's 50).
    pub fn thumb_size(self) -> i32 {
        match self {
            ViewMode::Rows => 50,
            ViewMode::Icons => 100,
            ViewMode::Icons18 => 18,
            ViewMode::Icons70 => 70,
            ViewMode::Icons256 => 256,
        }
    }

    /// Whether the mode shows icons rather than rows.
    pub fn is_icons(self) -> bool {
        self != ViewMode::Rows
    }

    /// The mode's stored number.
    pub fn number(self) -> i32 {
        self as i32
    }

    /// The mode stored as `number`, or `None` for a number no mode has.
    pub fn from_number(number: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.number() as i64 == number)
    }
}

/// How one folder is shown: its view mode and sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewState {
    pub view_mode: ViewMode,
    pub sort: SortBehavior,
}

impl Default for ViewState {
    /// The usual icons, sorted by name ascending: a folder with nothing remembered.
    fn default() -> Self {
        Self {
            view_mode: ViewMode::Icons,
            sort: SortBehavior::default(),
        }
    }
}

/// A remembered folder being found again: the folders from just below the browser's root down
/// to the remembered one, or `None` when it can no longer be reached (the browser then stays at
/// its root). Run on the config's spawner.
pub type LocationFuture = Pin<Box<dyn Future<Output = Option<Vec<Box<dyn Folder>>>>>>;

/// What a browser remembers, in the app's store. Every method defaults to remembering nothing.
pub trait BrowserMemory {
    /// How the folder at `path_names` (the names from the root down) was last shown, or `None`.
    fn view(&self, path_names: &[String]) -> Option<ViewState> {
        let _ = path_names;
        None
    }

    /// Remembers how the folder at `path_names` is shown.
    fn set_view(&self, path_names: &[String], view: &ViewState) {
        let _ = (path_names, view);
    }

    /// The folder tree's remembered width, in logical pixels.
    fn tree_width(&self) -> Option<f64> {
        None
    }

    /// Remembers the folder tree's width.
    fn set_tree_width(&self, width: f64) {
        let _ = width;
    }

    /// Starts finding the folder last remembered under `key` (one key per kind of browser: a
    /// main browser, a save dialog), or `None` when none was.
    fn last_location(&self, key: &str) -> Option<LocationFuture> {
        let _ = key;
        None
    }

    /// Remembers `path` under `key`: the whole open path, the browser's root first and the open
    /// folder last ([`remember_open_location`]), so an app can store where the path starts (its
    /// root's name) as well as where it ends. The folders are the browser's own, shared, handed
    /// over with the browser's state not borrowed (`BrowserState::remember_location`).
    fn remember_location(&self, key: &str, path: &[OpenFolder]) {
        let _ = (key, path);
    }
}

/// Hands `memory` the browser's open path (`open`: its root first, the open folder last) to
/// remember under `key`, the way every browser view remembers where it is.
pub fn remember_open_location(memory: &dyn BrowserMemory, key: &str, open: &[OpenFolder]) {
    memory.remember_location(key, open);
}

/// The memory of an app that remembers nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMemory;

impl BrowserMemory for NoMemory {}
