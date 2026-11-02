//! What a browser's menus hold and what activating a row does: the [`BrowserActions`] hook, the
//! browser's own actions an app places in its menu ([`BuiltinAction`], offered through
//! [`Builtins`]), the app's own commands ([`BrowserCommand`]) and how a row was activated
//! ([`Activation`]). [`NoActions`] is the hook for an app with nothing to add: the built-ins in
//! their usual order, and activation left to the browser (a folder row opens).
//!
//! An app's menu is the whole ordered list ([`BrowserAction`]s), so it decides where the
//! built-ins sit among its own commands. The browser turns each `Builtin` into its own action
//! and each `Command` into a menu row.

use std::fmt;
use std::rc::Rc;

use crate::entry::{BrowserRef, Entry};

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

/// An app's own menu command: a title (already translated), whether it can run now, and what it
/// does. The closure captures what it acts on (the [`BrowserRef`] its menu was built from).
#[derive(Clone)]
pub struct BrowserCommand {
    title: String,
    enabled: bool,
    run: Rc<dyn Fn()>,
}

impl BrowserCommand {
    /// The command `title`, enabled, running `run`.
    pub fn new(title: impl Into<String>, run: impl Fn() + 'static) -> Self {
        Self {
            title: title.into(),
            enabled: true,
            run: Rc::new(run),
        }
    }

    /// The same command, enabled or not (shown greyed and doing nothing when not).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The title the menu row shows.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Whether the command can run.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Runs the command when it is enabled; answers whether it ran.
    pub fn run(&self) -> bool {
        if self.enabled {
            (self.run)();
        }
        self.enabled
    }
}

impl fmt::Debug for BrowserCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowserCommand")
            .field("title", &self.title)
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// One row of a browser's menu.
#[derive(Clone, Debug)]
pub enum BrowserAction {
    /// One of the browser's own actions.
    Builtin(BuiltinAction),
    /// The app's own command.
    Command(BrowserCommand),
    /// A line between groups.
    Separator,
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
}

/// The actions of an app with nothing to add (see the module header).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoActions;

impl BrowserActions for NoActions {}
