//! The folder tree's right-click menu. A right press on a tree row shows that folder in the list
//! and asks for this menu (`row.rs`); the press bubbles to the tree (`mod.rs`), which opens it
//! where the press was. It is the selection's menu for the clicked folder (`list_menu.rs`'s
//! rules: the rows that can run): the app's ordered list from `BrowserActions::menu`, with the
//! built-ins that act on a selection placed in it, asked for and resolved with the folder as the
//! browser's temporary selection, listed in the folder above it. So "Remove" removes the clicked
//! folder from the folder above, and "Rename" asks for the folder's new name and shows it under
//! that name (`builtins/run.rs`). Each row runs with that temporary selection set again, then
//! the one there was put back. A menu with no rows (a root row's, when the app offers nothing
//! there) does not open.
//!
//! The app's hook is asked with the state let go (the crate header's rule): the temporary
//! selection is set and the snapshot taken under one borrow, which ends before the hook runs.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use agg_gui::Point;
use agg_gui_files::FolderLink;

use crate::actions::{BrowserAction, BrowserCommand};
use crate::list::list_menu::{builtins_for, resolve_builtins, select_rows, ListMenu, MenuScope};
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

/// `rows`, each command (in submenus too) run with `selection` as the temporary selection of
/// the browser `state` (if it is still open).
fn wrapped(
    rows: Vec<BrowserAction>,
    state: &Weak<RefCell<BrowserState>>,
    selection: &TreeSelection,
) -> Vec<BrowserAction> {
    rows.into_iter()
        .map(|row| match row {
            BrowserAction::Command(command) => {
                BrowserAction::Command(wrap(command, Weak::clone(state), selection.clone()))
            }
            other => other,
        })
        .collect()
}

/// `command`, run with `selection` as the temporary selection of the browser `state`.
fn wrap(
    command: BrowserCommand,
    state: Weak<RefCell<BrowserState>>,
    selection: TreeSelection,
) -> BrowserCommand {
    if !command.submenu_rows().is_empty() {
        let inner = wrapped(command.submenu_rows().to_vec(), &state, &selection);
        return command.with_submenu_rows(inner);
    }
    let inner = command.clone();
    command.with_run(move || match state.upgrade() {
        Some(state) => {
            with_temporary_selection(&state, &selection, || inner.run());
        }
        None => {
            inner.run();
        }
    })
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
        let builtins = builtins_for(MenuScope::Selection);
        let rows = snapshot.map(|snapshot| actions.menu(&snapshot, &builtins))?;
        let rows = resolve_builtins(state, rows, &builtins);
        Some(select_rows(rows, MenuScope::Selection))
    });
    wrapped(asked.unwrap_or_default(), &Rc::downgrade(state), &selection)
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
    let look = Rc::clone(&state.borrow().config().look);
    menu.open_rows(rows, at, "tree menu", look.as_ref())
}

/// Delete or Backspace in the tree: the menus' Remove for the folder the tree shows selected,
/// with its question, as its menu's Remove runs it (nothing when it can't run, or at a root
/// folder, which nothing lists that could remove it).
pub(crate) fn remove_selected_folder(
    state: &Rc<RefCell<BrowserState>>,
    model: &Rc<RefCell<TreeModel>>,
) {
    let selected = {
        let s = state.borrow();
        model
            .borrow()
            .selected_link(&s)
            .map(|link| (link, s.depth()))
    };
    // The showing folder is the path's last; the folder listing it is the one above.
    let Some((link, depth)) = selected.filter(|(_, depth)| *depth >= 2) else {
        return;
    };
    let selection: TreeSelection = (link, depth - 2);
    with_temporary_selection(state, &selection, || {
        crate::builtins::run_builtin(state, crate::actions::BuiltinAction::Remove)
    });
}
