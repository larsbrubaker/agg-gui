//! Who shows the browser's right-click menus: the [`BrowserMenus`] hook. The list and the folder
//! tree build a menu's rows themselves (the app's ordered list from `BrowserActions::menu`, the
//! built-ins resolved, filtered for the menu's scope, and in the tree each row run with the
//! clicked folder as the temporary selection) and then offer them to the hook with where the menu
//! opens. An app that hosts menus of its own (its own look, placement, names that its tests and
//! help find rows by, long-press handling) shows them there and answers `true`; the default
//! ([`OwnMenus`]) answers `false`, and the list or tree opens the rows in its own popup menu
//! (`list/list_menu.rs`'s `ListMenu`). Either way the rows are the same and run the same.

use agg_gui::Point;

use crate::actions::BrowserAction;
use crate::list::list_menu::MenuScope;

/// Which of the browser's menus is opening.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSource {
    /// The list's: the selection's menu (a right press on a row or its menu button) or the
    /// list's own (a right press on empty space or a section header, a click in its corner).
    List(MenuScope),
    /// The folder tree's menu for the row right-pressed (the selection's menu for that folder).
    Tree,
}

/// Shows the browser's right-click menus (see the module header).
pub trait BrowserMenus {
    /// Shows `rows` (never empty) as a menu at `window_at` (window coordinates, Y up: where the
    /// press was, or the bottom-left corner of the menu button that asked); answers whether the
    /// app showed them. On `false` the browser opens them in its own menu.
    fn open(&self, rows: Vec<BrowserAction>, window_at: Point, source: MenuSource) -> bool;
}

/// The default: the browser shows its menus itself.
#[derive(Clone, Copy, Debug, Default)]
pub struct OwnMenus;

impl BrowserMenus for OwnMenus {
    fn open(&self, _rows: Vec<BrowserAction>, _window_at: Point, _source: MenuSource) -> bool {
        false
    }
}
