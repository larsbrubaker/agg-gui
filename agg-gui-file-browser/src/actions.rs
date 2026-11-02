//! What a browser's menus hold and what activating a row does: the [`BrowserActions`] hook, the
//! browser's own actions an app places in its menu ([`BuiltinAction`], offered through
//! [`Builtins`]), the app's own commands ([`BrowserCommand`]) and how a row was activated
//! ([`Activation`]). [`NoActions`] is the hook for an app with nothing to add: the built-ins in
//! their usual order, and activation left to the browser (a folder row opens).
//!
//! An app's menu is the whole ordered list ([`BrowserAction`]s), so it decides where the
//! built-ins sit among its own commands. The browser turns each `Builtin` into its own action,
//! which runs (`builtins/`), and each `Command` into a menu row with the command's icon,
//! shortcut, scope and submenu (`command.rs`, built by `list/list_menu.rs`). A folder refused for
//! the sign-in shows the app's Sign In ([`BrowserActions::sign_in`], a [`SignInOffer`]) under its
//! message. Open File Location asks the app where an item lives ([`BrowserActions::can_locate`],
//! [`BrowserActions::locate`], an [`ItemLocation`]): a search result's folder or a shortcut's
//! real folder is found the app's own way.

use std::fmt;
use std::rc::Rc;

use agg_gui_files::{tr, Folder, Item};

use crate::entry::{BrowserRef, Entry};

mod command;

pub use command::{ActionScope, BrowserCommand};

/// One of the browser's own actions, which an app places in its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BuiltinAction {
    /// Add files to the open folder.
    Add,
    /// Make a new folder in the open folder.
    CreateFolder,
    /// Show the selected item in the system's file manager.
    OpenFileLocation,
    /// Rename the selected item.
    Rename,
    /// Move the selected items to another folder.
    Move,
    /// Remove the selected items.
    Remove,
}

impl BuiltinAction {
    /// Every built-in, in the order the default menu lists them.
    pub const ALL: &'static [BuiltinAction] = &[
        BuiltinAction::Add,
        BuiltinAction::CreateFolder,
        BuiltinAction::OpenFileLocation,
        BuiltinAction::Rename,
        BuiltinAction::Move,
        BuiltinAction::Remove,
    ];

    /// The title its menu row shows, translated.
    pub fn title(self) -> String {
        match self {
            BuiltinAction::Add => tr("Add"),
            BuiltinAction::CreateFolder => format!("{}...", tr("Create Folder")),
            BuiltinAction::OpenFileLocation => tr("Open File Location"),
            BuiltinAction::Rename => tr("Rename"),
            BuiltinAction::Move => tr("Move"),
            BuiltinAction::Remove => tr("Remove"),
        }
    }

    /// Whether the action acts on the selected rows (an item's menu) rather than on the open
    /// folder (the list's menu on its empty space).
    pub fn acts_on_selection(self) -> bool {
        !matches!(self, BuiltinAction::Add | BuiltinAction::CreateFolder)
    }

    /// The menus its row is in.
    pub fn scope(self) -> ActionScope {
        match self.acts_on_selection() {
            true => ActionScope::Selection,
            false => ActionScope::Folder,
        }
    }

    /// The icon key its row asks the look for (`BrowserLook::icon_for_key`): "add",
    /// "create_folder", "open_file_location", "rename", "move" or "remove". The default look has
    /// none, so the row shows no icon until the app's look gives one.
    pub fn icon_key(self) -> &'static str {
        match self {
            BuiltinAction::Add => "add",
            BuiltinAction::CreateFolder => "create_folder",
            BuiltinAction::OpenFileLocation => "open_file_location",
            BuiltinAction::Rename => "rename",
            BuiltinAction::Move => "move",
            BuiltinAction::Remove => "remove",
        }
    }

    /// The default menu's group: adding to the folder, finding the item, changing it. Groups are
    /// parted by separators.
    fn group(self) -> u8 {
        match self {
            BuiltinAction::Add | BuiltinAction::CreateFolder => 0,
            BuiltinAction::OpenFileLocation => 1,
            BuiltinAction::Rename | BuiltinAction::Move | BuiltinAction::Remove => 2,
        }
    }
}

/// The built-ins a browser offers where its menu opens, which [`BrowserActions::menu`] places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Builtins {
    offered: Vec<BuiltinAction>,
}

impl Builtins {
    /// The built-ins `offered`, kept in [`BuiltinAction::ALL`]'s order whatever order they come
    /// in, each once.
    pub fn new(offered: &[BuiltinAction]) -> Self {
        Self {
            offered: BuiltinAction::ALL
                .iter()
                .copied()
                .filter(|a| offered.contains(a))
                .collect(),
        }
    }

    /// Every built-in.
    pub fn all() -> Self {
        Self::new(BuiltinAction::ALL)
    }

    /// The offered built-ins, in [`BuiltinAction::ALL`]'s order.
    pub fn offered(&self) -> &[BuiltinAction] {
        &self.offered
    }

    /// Whether `action` is offered.
    pub fn offers(&self, action: BuiltinAction) -> bool {
        self.offered.contains(&action)
    }

    /// The default menu: the offered built-ins in order, a separator between groups (adding,
    /// finding, changing), none at either end or doubled.
    pub fn default_menu(&self) -> Vec<BrowserAction> {
        let mut menu = Vec::new();
        let mut last_group = None;
        for &action in &self.offered {
            if last_group.is_some_and(|g| g != action.group()) {
                menu.push(BrowserAction::Separator);
            }
            last_group = Some(action.group());
            menu.push(BrowserAction::Builtin(action));
        }
        menu
    }
}

/// One row of a browser's menu.
#[derive(Clone, Debug)]
pub enum BrowserAction {
    /// One of the browser's own actions, which the browser runs (`builtins/`); left out of a menu
    /// that doesn't offer it ([`Builtins`]).
    Builtin(BuiltinAction),
    /// The app's own command, a submenu included.
    Command(BrowserCommand),
    /// A line between groups; a menu shows it only between two rows it keeps.
    Separator,
}

/// Where "Open File Location" takes the user: the folders `folders` opened below the path's
/// folder at `base` (0 = the root), the last of them showing, or `None` when the item's folder
/// is gone (said on the status line). See [`BrowserActions::locate`].
pub struct ItemLocation {
    /// The depth of the open path's folder the located folders open below.
    pub base: usize,
    /// The folders, outermost first; `None` when there is nowhere to go.
    pub folders: Option<Vec<Box<dyn Folder>>>,
}

impl fmt::Debug for ItemLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ItemLocation")
            .field("base", &self.base)
            .field(
                "folders",
                &self
                    .folders
                    .as_ref()
                    .map(|v| v.iter().map(|f| f.name().to_string()).collect::<Vec<_>>()),
            )
            .finish()
    }
}

/// How a row was activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Activation {
    /// A double click.
    DoubleClick,
    /// One tap, in a browser whose taps activate (`BrowserOptions::tap_activates`).
    Tap,
}

/// The Sign In a folder refused for the sign-in offers under its message: what the message
/// says now, the button's title, whether it can be pressed now, and what pressing it does. The
/// browser asks again at each layout, so the app may change any of it as signing in goes on
/// ("Waiting for the browser...").
#[derive(Clone)]
pub struct SignInOffer {
    /// The message shown above the button (already translated).
    pub text: String,
    /// The button's title (already translated).
    pub action: String,
    /// Whether the button takes a press now.
    pub enabled: bool,
    /// Starts signing in; the folder lists itself again when it is done.
    pub press: Rc<dyn Fn()>,
}

impl fmt::Debug for SignInOffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignInOffer")
            .field("text", &self.text)
            .field("action", &self.action)
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// What a browser's menus hold and what activating a row does.
pub trait BrowserActions {
    /// The whole ordered menu for `state`, the built-ins `builtins` offers placed where the app
    /// wants them. The default is [`Builtins::default_menu`].
    fn menu(&self, state: &BrowserRef, builtins: &Builtins) -> Vec<BrowserAction> {
        let _ = state;
        builtins.default_menu()
    }

    /// Activates `entry` (`how`: a double click or a tap) the app's way, answering whether it did.
    /// When it did not, the browser does its own: a folder row opens, an item does nothing. The
    /// default leaves every activation to the browser.
    fn activate(&self, state: &BrowserRef, entry: &Entry, how: Activation) -> bool {
        let _ = (state, entry, how);
        false
    }

    /// The Sign In for the open folder of `state`, refused for the sign-in, whose own message is
    /// `folder_text`; `None` (the default) shows the message with no button, for an app that
    /// cannot sign in.
    fn sign_in(&self, state: &BrowserRef, folder_text: &str) -> Option<SignInOffer> {
        let _ = (state, folder_text);
        None
    }

    /// Whether "Open File Location" can find where `item`, the one selected row of `state`,
    /// lives. Only the app knows: a search result's folder, a shortcut's real folder, a cloud
    /// design's folder are each found its own way, so the browser asks rather than guesses. The
    /// default finds nothing, leaving the action greyed.
    fn can_locate(&self, state: &BrowserRef, item: &dyn Item) -> bool {
        let _ = (state, item);
        false
    }

    /// Where `item`, the one selected row of `state`, lives (see [`BrowserActions::can_locate`]);
    /// the browser then shows it (`BrowserState::show_location`). Asked with the state not
    /// borrowed: the hook may borrow it through `state.live()` to read the open path. The
    /// default finds nothing.
    fn locate(&self, state: &BrowserRef, item: &Rc<dyn Item>) -> Option<ItemLocation> {
        let _ = (state, item);
        None
    }
}

/// The actions of an app with nothing to add (see the module header).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoActions;

impl BrowserActions for NoActions {}
