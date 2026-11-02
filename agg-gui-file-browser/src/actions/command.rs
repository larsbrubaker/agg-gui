//! A menu row the app (or the browser, for its own actions) hands a menu: [`BrowserCommand`],
//! with its title, whether it runs now, what it does, and how its row looks: an icon key the
//! look resolves (`BrowserLook::icon_for_key`), a shortcut shown on the right, a hover help, the
//! menus it belongs to ([`ActionScope`]), whether it is left out rather than greyed while it
//! can't run, and the rows of a submenu. [`BrowserCommand::builtin`] places one of the browser's
//! own actions with the app's dressing (a tooltip, another icon key or shortcut): the menu runs
//! the built-in as it always does and shows the row with what the app set. A row marked
//! [`BrowserCommand::as_remove`] (the built-in Remove is one) is the menus' Remove, which the
//! Delete and Backspace keys run in the list and the tree. `actions.rs` places commands in the ordered list; the
//! list's menu builder (`list/list_menu.rs`) turns them into agg-gui `MenuItem`s.

use std::fmt;
use std::rc::Rc;

use super::{BrowserAction, BuiltinAction};

/// The menus a row belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionScope {
    /// The selection's menus (a right press on a row, a row's menu button, the tree's menu): the
    /// row acts on the selected items.
    Selection,
    /// The list's own menu (a right press on empty space): the row acts on the open folder.
    Folder,
}

/// An app's own menu command (see the module header). The closure captures what it acts on (the
/// [`BrowserRef`](crate::BrowserRef) its menu was built from).
#[derive(Clone)]
pub struct BrowserCommand {
    title: String,
    enabled: bool,
    run: Rc<dyn Fn()>,
    icon: Option<String>,
    shortcut: Option<String>,
    tooltip: Option<String>,
    scope: Option<ActionScope>,
    only_when_enabled: bool,
    submenu: Vec<BrowserAction>,
    /// The built-in this row stands for, run by the browser ([`Self::builtin`]).
    builtin: Option<BuiltinAction>,
    /// Whether this row is the menus' Remove ([`Self::as_remove`]).
    removes: bool,
}

impl BrowserCommand {
    /// The command `title`, enabled, running `run`, in every menu.
    pub fn new(title: impl Into<String>, run: impl Fn() + 'static) -> Self {
        Self {
            title: title.into(),
            enabled: true,
            run: Rc::new(run),
            icon: None,
            shortcut: None,
            tooltip: None,
            scope: None,
            only_when_enabled: false,
            submenu: Vec::new(),
            builtin: None,
            removes: false,
        }
    }

    /// The browser's own `action` as a row the app dresses: `.with_tooltip(..)` (MatterCAD's Add
    /// says which files it takes), `.with_icon(..)`, `.with_shortcut(..)`. The browser runs the
    /// action, decides whether it can run and which menus it is in, as for
    /// [`BrowserAction::Builtin`]; a menu that doesn't offer the built-in leaves the row out.
    pub fn builtin(action: BuiltinAction) -> Self {
        Self {
            builtin: Some(action),
            ..Self::new(action.title(), || {})
        }
    }

    /// The built-in this row stands for, if it was made with [`Self::builtin`].
    pub fn builtin_action(&self) -> Option<BuiltinAction> {
        self.builtin
    }

    /// This row is the menus' Remove: the Delete and Backspace keys run it, in the list for the
    /// selection and in the folder tree for the folder drawn selected, when it can run (an app
    /// whose own command removes, in place of the built-in, marks it so the keys run it).
    pub fn as_remove(mut self) -> Self {
        self.removes = true;
        self
    }

    /// Whether this row is the menus' Remove: marked [`Self::as_remove`], or the built-in.
    pub fn is_remove(&self) -> bool {
        self.removes || self.builtin == Some(BuiltinAction::Remove)
    }

    /// The browser's own row `self` with what the app's row `app` set on top: its tooltip, icon
    /// key and shortcut, where it set one.
    pub(crate) fn dressed_as(mut self, app: &BrowserCommand) -> Self {
        if let Some(tooltip) = &app.tooltip {
            self.tooltip = Some(tooltip.clone());
        }
        if let Some(icon) = &app.icon {
            self.icon = Some(icon.clone());
        }
        if let Some(shortcut) = &app.shortcut {
            self.shortcut = Some(shortcut.clone());
        }
        self
    }

    /// A row `title` opening a submenu of `rows` (an ordered list as the menu's own, built-ins
    /// included); it runs nothing itself.
    pub fn submenu(title: impl Into<String>, rows: Vec<BrowserAction>) -> Self {
        Self {
            submenu: rows,
            ..Self::new(title, || {})
        }
    }

    /// The same command, enabled or not (shown greyed and doing nothing when not).
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The same command with the icon the look gives for `key` (`BrowserLook::icon_for_key`; no
    /// icon when the look has none).
    pub fn with_icon(mut self, key: impl Into<String>) -> Self {
        self.icon = Some(key.into());
        self
    }

    /// The same command showing `shortcut` (already in the platform's words: "Delete", "⌘C") on
    /// its row's right. Only shown: the key itself is the app's to bind.
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// The same command with `tooltip` (already translated) as its row's hover help.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// The same command, only in `scope`'s menus (and the overflow menu, which holds every row).
    pub fn with_scope(mut self, scope: ActionScope) -> Self {
        self.scope = Some(scope);
        self
    }

    /// The same command, left out of every menu while it can't run rather than shown greyed: a
    /// row that means nothing outside one folder (a trash's Empty Trash).
    pub fn only_when_enabled(mut self) -> Self {
        self.only_when_enabled = true;
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

    /// The icon key the look resolves, if the row has an icon.
    pub fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    /// The shortcut the row shows, if any.
    pub fn shortcut(&self) -> Option<&str> {
        self.shortcut.as_deref()
    }

    /// The row's hover help, if any.
    pub fn tooltip(&self) -> Option<&str> {
        self.tooltip.as_deref()
    }

    /// The menus the row is in: `None` for every one.
    pub fn scope(&self) -> Option<ActionScope> {
        self.scope
    }

    /// Whether the row is left out while it can't run.
    pub fn is_only_when_enabled(&self) -> bool {
        self.only_when_enabled
    }

    /// The submenu's rows (empty for a plain command).
    pub fn submenu_rows(&self) -> &[BrowserAction] {
        &self.submenu
    }

    /// The same command with its submenu's rows replaced by `rows`.
    pub(crate) fn with_submenu_rows(mut self, rows: Vec<BrowserAction>) -> Self {
        self.submenu = rows;
        self
    }

    /// The same command running `run` instead (the tree wraps a command so it runs with the
    /// clicked folder as the selection).
    pub(crate) fn with_run(mut self, run: impl Fn() + 'static) -> Self {
        self.run = Rc::new(run);
        self
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
            .field("icon", &self.icon)
            .field("shortcut", &self.shortcut)
            .field("scope", &self.scope)
            .field("only_when_enabled", &self.only_when_enabled)
            .field("submenu", &self.submenu)
            .field("builtin", &self.builtin)
            .field("removes", &self.removes)
            .finish()
    }
}
