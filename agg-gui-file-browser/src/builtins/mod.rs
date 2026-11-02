//! The browser's own actions ([`BuiltinAction`]) as menu rows that run: each built-in an app
//! places in its ordered list becomes a [`BrowserCommand`] here, titled through `tr`, with its
//! icon key (`BuiltinAction::icon_key`), its scope, whether it can run now, and what it does
//! (`run.rs`).
//!
//! Whether one can run follows the rules the folders and the app's dialogs set:
//! - Add: the open folder takes items (`ContainerActions::AddItems`) and the app can pick files.
//! - Create Folder...: the open folder takes folders (`AddContainers`) and the app asks names.
//! - Open File Location: one row selected, and the app can find where it lives
//!   (`BrowserActions::can_locate`).
//! - Rename: one row selected, not protected, in a folder that renames (`RenameItems`), and the
//!   app asks names.
//! - Move: rows selected, none protected, in a folder that removes (`RemoveItems`), at least one
//!   of them a file or folder that can move, and the app has a Move dialog.
//! - Remove: rows selected, none protected, in a folder that removes, and the app confirms.
//!
//! The selection is the browser's (the tree's temporary selection, listed in the folder above
//! the clicked one, while the tree's menu is asked and run). Rules are read under a borrow of the
//! state that ends before any hook (`can_locate`) is asked.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui_files::{ContainerActions, Item};

use crate::actions::{BrowserCommand, BuiltinAction};
use crate::dialogs::DialogKind;
use crate::state::BrowserState;

pub(crate) mod run;

/// What a built-in acts on, read from the state when its row is made or it runs.
pub(crate) struct Target {
    /// The depth of the folder the selection is listed in (the open folder, or the folder above
    /// the tree's clicked one).
    pub depth: usize,
    /// The selected rows' items, a folder row as its link.
    pub items: Vec<Rc<dyn Item>>,
    /// Whether the selection is the tree's (its clicked folder, listed above it).
    pub from_tree: bool,
}

impl Target {
    /// What `state` acts on now.
    pub fn of(state: &BrowserState) -> Self {
        let (depth, from_tree) = match state.temporary_selection() {
            Some((_, depth)) => (*depth, true),
            None => (state.depth().saturating_sub(1), false),
        };
        let items = state
            .selected_entries()
            .iter()
            .map(|entry| entry.item())
            .collect();
        Self {
            depth,
            items,
            from_tree,
        }
    }
}

/// Whether the folder at `depth` of `state`'s path takes changes and allows `action`.
fn allows(state: &BrowserState, depth: usize, action: ContainerActions) -> bool {
    state
        .path_containers()
        .nth(depth)
        .and_then(|folder| folder.as_writable().map(|w| w.allow_action(action)))
        .unwrap_or(false)
}

/// Whether `item` is a file or folder a Move can carry (a generated row is neither).
pub(crate) fn can_move(item: &Rc<dyn Item>) -> bool {
    item.as_asset_stream().is_some() || item.as_container_link().is_some()
}

/// Whether `action` can run in `state` now (the rules in the module header).
pub(crate) fn is_enabled(state: &Rc<RefCell<BrowserState>>, action: BuiltinAction) -> bool {
    let (dialogs, actions, snapshot, target, rules) = {
        let s = state.borrow();
        let target = Target::of(&s);
        let open = s.depth().saturating_sub(1);
        let unprotected =
            !target.items.is_empty() && target.items.iter().all(|i| !i.is_protected());
        let rules = match action {
            BuiltinAction::Add => allows(&s, open, ContainerActions::AddItems),
            BuiltinAction::CreateFolder => allows(&s, open, ContainerActions::AddContainers),
            BuiltinAction::OpenFileLocation => target.items.len() == 1,
            BuiltinAction::Rename => {
                target.items.len() == 1
                    && unprotected
                    && allows(&s, target.depth, ContainerActions::RenameItems)
            }
            BuiltinAction::Move => {
                unprotected
                    && target.items.iter().any(can_move)
                    && allows(&s, target.depth, ContainerActions::RemoveItems)
            }
            BuiltinAction::Remove => {
                unprotected && allows(&s, target.depth, ContainerActions::RemoveItems)
            }
        };
        let snapshot = s.snapshot_at(target.depth);
        let config = s.config();
        (
            Rc::clone(&config.dialogs),
            Rc::clone(&config.actions),
            snapshot,
            target,
            rules,
        )
    };
    if !rules {
        return false;
    }
    match action {
        BuiltinAction::Add => dialogs.offers(DialogKind::PickFiles),
        BuiltinAction::CreateFolder | BuiltinAction::Rename => dialogs.offers(DialogKind::AskName),
        BuiltinAction::Move => dialogs.offers(DialogKind::PickMoveTarget),
        BuiltinAction::Remove => dialogs.offers(DialogKind::Confirm),
        BuiltinAction::OpenFileLocation => match (snapshot, target.items.first()) {
            (Some(snapshot), Some(item)) => actions.can_locate(&snapshot, item.as_ref()),
            _ => false,
        },
    }
}

/// `action`'s menu row for `state`: titled, with its icon key and scope, enabled by the rules
/// now, running it with the state let go (`run.rs`).
pub(crate) fn builtin_command(
    state: &Rc<RefCell<BrowserState>>,
    action: BuiltinAction,
) -> BrowserCommand {
    let enabled = is_enabled(state, action);
    let weak = Rc::downgrade(state);
    BrowserCommand::new(action.title(), move || {
        if let Some(state) = weak.upgrade() {
            run::run(&state, action);
        }
    })
    .with_icon(action.icon_key())
    .with_scope(action.scope())
    .enabled(enabled)
}

/// Runs `action` in `state` when it can run now (the Delete key's Remove); answers whether it
/// did.
pub fn run_builtin(state: &Rc<RefCell<BrowserState>>, action: BuiltinAction) -> bool {
    if !is_enabled(state, action) {
        return false;
    }
    run::run(state, action);
    true
}
