//! The folder tree's right-click menu. A right press on a tree row shows that folder in the list
//! and asks for this menu (`row.rs`); the press bubbles to the tree (`mod.rs`), which opens it
//! where the press was. It is the selection's menu for the clicked folder: the app's ordered list
//! from `BrowserActions::menu`, with the built-ins that act on a selection placed in it (greyed
//! until the browser can run them itself), asked for with the folder as the browser's temporary
//! selection, listed in the folder above it. So the app's "Remove" reads the folder above as the
//! folder it acts in and removes the clicked folder there. Each of the app's commands runs with
//! that temporary selection set again, then the one there was put back. A menu with no rows
//! (a root row's, when the app offers nothing there) does not open.
//!
//! The app's hook is asked with the state let go (the crate header's rule): the temporary
//! selection is set and the snapshot taken under one borrow, which ends before the hook runs.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use agg_gui::Point;
use agg_gui_files::FolderLink;

use crate::actions::{BrowserAction, BrowserCommand};
use crate::list::list_menu::{builtins_for, ListMenu, MenuScope};
use crate::state::BrowserState;

use super::model::TreeModel;

/// The clicked folder and the depth (in the browser's path) of the folder listing it.
type TreeSelection = (Rc<dyn FolderLink>, usize);

/// Runs `run` with `selection` as `state`'s temporary selection, then puts back the one there
/// was; the state is not borrowed while `run` runs.
fn with_temporary_selection<T>(
    state: &Rc<RefCell<BrowserState>>,
    selection: &TreeSelection,
    run: impl FnOnce() -> T,
) -> T {
    let previous = state
        .borrow_mut()
        .set_temporary_selection(Some(selection.clone()));
    let result = run();
    state.borrow_mut().set_temporary_selection(previous);
    result
}

/// `command`, run with `selection` as the temporary selection of the browser `state` (if it is
/// still open).
fn wrapped(
    command: BrowserCommand,
    state: Weak<RefCell<BrowserState>>,
    selection: TreeSelection,
) -> BrowserCommand {
    let enabled = command.is_enabled();
    let title = command.title().to_string();
    BrowserCommand::new(title, move || match state.upgrade() {
        Some(state) => {
            with_temporary_selection(&state, &selection, || command.run());
        }
        None => {
            command.run();
        }
    })
    .enabled(enabled)
}

/// The rows of the menu for tree row `index` of `model`: empty when the row is gone or stands
/// for no folder.
pub(crate) fn tree_menu_rows(
    state: &Rc<RefCell<BrowserState>>,
    model: &Rc<RefCell<TreeModel>>,
    index: usize,
) -> Vec<BrowserAction> {
    let Some(path) = model
        .borrow()
        .rows()
        .get(index)
        .filter(|row| !row.is_placeholder())
        .map(|row| row.path().to_vec())
    else {
        return Vec::new();
    };
    let Some(link) = path.last().cloned() else {
        return Vec::new();
    };
    // The root is at depth 0, so the folder above the row's folder is at `path.len() - 1`.
    let selection: TreeSelection = (link, path.len() - 1);
    let asked = with_temporary_selection(state, &selection, || {
        let (actions, snapshot) = {
            let s = state.borrow();
            (Rc::clone(&s.config().actions), s.snapshot_at(selection.1))
        };
        snapshot.map(|snapshot| actions.menu(&snapshot, &builtins_for(MenuScope::Selection)))
    });
    let weak = Rc::downgrade(state);
    asked
        .unwrap_or_default()
        .into_iter()
        .map(|row| match row {
            BrowserAction::Command(command) => {
                BrowserAction::Command(wrapped(command, Weak::clone(&weak), selection.clone()))
            }
            other => other,
        })
        .collect()
}

/// Opens the menu for tree row `index` at `at` (the tree's own coordinates); answers whether it
/// opened (a menu with no rows does not).
pub(crate) fn open_tree_menu(
    menu: &mut ListMenu,
    state: &Rc<RefCell<BrowserState>>,
    model: &Rc<RefCell<TreeModel>>,
    index: usize,
    at: Point,
) -> bool {
    let rows = tree_menu_rows(state, model, index);
    if !rows
        .iter()
        .any(|row| !matches!(row, BrowserAction::Separator))
    {
        return false;
    }
    menu.open_rows(rows, at, "tree menu");
    true
}
