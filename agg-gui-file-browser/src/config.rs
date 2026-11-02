//! [`BrowserConfig`]: everything one browser view takes from its app, in one value: where work
//! runs, its look, pictures, menus and who shows them, drops outside it, move-or-copy rules, memory, dialogs, where
//! errors go, and its [`BrowserOptions`]. Every hook has a default, so
//! `BrowserConfig::default()` is a working browser for an app with no hooks: agg-gui's look,
//! the items' own pictures (`OwnPictures`), the built-in menu shown in the browser's own popup
//! menus (`OwnMenus`), no outside drops, the default
//! places rule, nothing remembered, no dialogs (the actions needing one unavailable), errors on
//! the browser's own status line (`BrowserState::status`), work on a [`LocalSpawner`] (whose
//! faults, with no report given, go to standard error: it is made before any browser it could
//! show them in) and an undo history of its own (an app shares one between its views by giving
//! each the same [`BrowserHistory`]).
//!
//! [`BrowserConfig::builder`] sets only what the app has; its `build` fills the rest, the
//! default spawner reporting through the report the builder was given, or to standard error
//! without one.

use std::fmt;
use std::rc::Rc;

use agg_gui_files::{FolderLink, Spawner};

use crate::actions::{BrowserActions, NoActions};
use crate::dialogs::{BrowserDialogs, NoDialogs};
use crate::drops::OutsideDrops;
use crate::history::BrowserHistory;
use crate::local_spawner::LocalSpawner;
use crate::look::{BrowserLook, DefaultLook};
use crate::memory::{BrowserMemory, NoMemory, ViewMode};
use crate::menus::{BrowserMenus, OwnMenus};
use crate::own_pictures::OwnPictures;
use crate::pictures::ItemPictures;
use crate::places::{DefaultPlaces, PlaceRules};

/// Shows the user an error the browser met (already translated).
pub type ErrorReport = Rc<dyn Fn(&str)>;

/// Which folder rows a browser lists (a Move dialog lists only folders that take items).
pub type FolderFilter = Rc<dyn Fn(&dyn FolderLink) -> bool>;

/// Whether a browser shows a folder tree beside its list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrowserLayout {
    /// The folder tree beside the list.
    #[default]
    TreeAndList,
    /// The list alone (a browser in a popup or a narrow panel).
    ListOnly,
}

/// Who hears an item's double click (a folder row's always opens it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DoubleClickTarget {
    /// The app, through `BrowserActions::activate`.
    #[default]
    Actions,
    /// The page hosting the browser (a save or open dialog, where a double click picks the
    /// item): the browser keeps the activation for the page to take.
    Host,
}

/// How one browser view behaves.
#[derive(Clone)]
pub struct BrowserOptions {
    /// Tree beside the list, or the list alone.
    pub layout: BrowserLayout,
    /// Who hears an item's double click.
    pub double_click: DoubleClickTarget,
    /// One tap on any row activates it, as a double click does (a phone's popup browser).
    pub tap_activates: bool,
    /// A finger's tap on a folder row opens it, while a mouse click still only selects (dialogs
    /// on touch screens).
    pub finger_tap_opens_folders: bool,
    /// Whether the list shows folder rows at all ("Show Folders").
    pub show_folders: bool,
    /// The folder rows the list shows, or `None` for all of them.
    pub folder_filter: Option<FolderFilter>,
    /// The picture side the list opens on when a folder has no remembered view (icons of 100;
    /// a dialog may ask for 75).
    pub thumb_size: i32,
}

impl Default for BrowserOptions {
    fn default() -> Self {
        Self {
            layout: BrowserLayout::default(),
            double_click: DoubleClickTarget::default(),
            tap_activates: false,
            finger_tap_opens_folders: false,
            show_folders: true,
            folder_filter: None,
            thumb_size: ViewMode::Icons.thumb_size(),
        }
    }
}

impl fmt::Debug for BrowserOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowserOptions")
            .field("layout", &self.layout)
            .field("double_click", &self.double_click)
            .field("tap_activates", &self.tap_activates)
            .field("finger_tap_opens_folders", &self.finger_tap_opens_folders)
            .field("show_folders", &self.show_folders)
            .field("folder_filter", &self.folder_filter.is_some())
            .field("thumb_size", &self.thumb_size)
            .finish()
    }
}

/// Where the default spawner reports a fault when the app gives no report: standard error.
fn report_to_stderr(message: &str) {
    eprintln!("{message}");
}

/// Everything one browser view takes from its app (see the module header).
#[derive(Clone)]
pub struct BrowserConfig {
    /// Where work nobody awaits runs (loads, pictures, copies).
    pub spawner: Rc<dyn Spawner>,
    /// Colours, sizes, icons and widget names.
    pub look: Rc<dyn BrowserLook>,
    /// The pictures entries show and what lies over a tile.
    pub pictures: Rc<dyn ItemPictures>,
    /// The menus and what activating a row does.
    pub actions: Rc<dyn BrowserActions>,
    /// Who shows the list's and tree's right-click menus.
    pub menus: Rc<dyn BrowserMenus>,
    /// The app's views that take a drag of the browser's items; `None` keeps drags inside.
    pub drops: Option<Rc<dyn OutsideDrops>>,
    /// Move or copy, and copies the app runs itself.
    pub places: Rc<dyn PlaceRules>,
    /// The views per folder, the tree's width and last locations, in the app's store.
    pub memory: Rc<dyn BrowserMemory>,
    /// Confirming, asking for a name, picking files.
    pub dialogs: Rc<dyn BrowserDialogs>,
    /// Shows the user an error the browser met (already translated); `None` shows it on the
    /// browser's own status line (`BrowserState::report`).
    pub report: Option<ErrorReport>,
    /// How the view behaves.
    pub options: BrowserOptions,
    /// The undo history; views given the same one share it, so a change made in one can be
    /// taken back from another.
    pub history: Rc<BrowserHistory>,
}

impl BrowserConfig {
    /// A builder starting from every default.
    pub fn builder() -> BrowserConfigBuilder {
        BrowserConfigBuilder::default()
    }
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Builds a [`BrowserConfig`] from what the app has; `build` fills the rest with defaults.
#[derive(Default)]
pub struct BrowserConfigBuilder {
    spawner: Option<Rc<dyn Spawner>>,
    look: Option<Rc<dyn BrowserLook>>,
    pictures: Option<Rc<dyn ItemPictures>>,
    actions: Option<Rc<dyn BrowserActions>>,
    menus: Option<Rc<dyn BrowserMenus>>,
    drops: Option<Rc<dyn OutsideDrops>>,
    places: Option<Rc<dyn PlaceRules>>,
    memory: Option<Rc<dyn BrowserMemory>>,
    dialogs: Option<Rc<dyn BrowserDialogs>>,
    report: Option<ErrorReport>,
    options: BrowserOptions,
    history: Option<Rc<BrowserHistory>>,
}

impl BrowserConfigBuilder {
    pub fn spawner(mut self, spawner: Rc<dyn Spawner>) -> Self {
        self.spawner = Some(spawner);
        self
    }

    pub fn look(mut self, look: Rc<dyn BrowserLook>) -> Self {
        self.look = Some(look);
        self
    }

    pub fn pictures(mut self, pictures: Rc<dyn ItemPictures>) -> Self {
        self.pictures = Some(pictures);
        self
    }

    pub fn actions(mut self, actions: Rc<dyn BrowserActions>) -> Self {
        self.actions = Some(actions);
        self
    }

    /// Who shows the right-click menus (the app's own menu host).
    pub fn menus(mut self, menus: Rc<dyn BrowserMenus>) -> Self {
        self.menus = Some(menus);
        self
    }

    pub fn drops(mut self, drops: Rc<dyn OutsideDrops>) -> Self {
        self.drops = Some(drops);
        self
    }

    pub fn places(mut self, places: Rc<dyn PlaceRules>) -> Self {
        self.places = Some(places);
        self
    }

    pub fn memory(mut self, memory: Rc<dyn BrowserMemory>) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn dialogs(mut self, dialogs: Rc<dyn BrowserDialogs>) -> Self {
        self.dialogs = Some(dialogs);
        self
    }

    /// Where errors go; the default spawner reports its faults here too.
    pub fn report(mut self, report: ErrorReport) -> Self {
        self.report = Some(report);
        self
    }

    pub fn options(mut self, options: BrowserOptions) -> Self {
        self.options = options;
        self
    }

    /// The undo history, to share one between an app's browser views.
    pub fn history(mut self, history: Rc<BrowserHistory>) -> Self {
        self.history = Some(history);
        self
    }

    /// The config: what was set, and the module header's defaults for the rest.
    pub fn build(self) -> BrowserConfig {
        let report = self.report;
        let spawner = self.spawner.unwrap_or_else(|| {
            let faults = report
                .clone()
                .unwrap_or_else(|| Rc::new(report_to_stderr) as ErrorReport);
            Rc::new(LocalSpawner::new(faults))
        });
        BrowserConfig {
            spawner,
            look: self.look.unwrap_or_else(|| Rc::new(DefaultLook::default())),
            pictures: self.pictures.unwrap_or_else(|| Rc::new(OwnPictures::new())),
            actions: self.actions.unwrap_or_else(|| Rc::new(NoActions)),
            menus: self.menus.unwrap_or_else(|| Rc::new(OwnMenus)),
            drops: self.drops,
            places: self.places.unwrap_or_else(|| Rc::new(DefaultPlaces)),
            memory: self.memory.unwrap_or_else(|| Rc::new(NoMemory)),
            dialogs: self.dialogs.unwrap_or_else(|| Rc::new(NoDialogs)),
            report,
            options: self.options,
            history: self.history.unwrap_or_default(),
        }
    }
}
