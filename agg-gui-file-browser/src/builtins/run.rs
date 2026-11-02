//! What each built-in does once picked (`mod.rs` decides whether it can): it reads what it acts
//! on from the state, asks the app's dialog with the state let go, and on the answer writes
//! through `BrowserState::write_recorded`, so the change is recorded for Undo and the browser
//! lists again with any failure on its status line.
//!
//! - Create Folder... asks "Folder Name" and makes the folder in the open folder.
//! - Rename asks "Name" over the current name and renames the row in the folder listing it; a
//!   folder renamed from the tree while it shows is shown again under its new name.
//! - Remove asks first ("Move to Trash?" for a folder on the computer, whose items go to the
//!   system's trash; "Remove Items?" elsewhere), then removes. The tree's folder, removed while
//!   it shows, gives way to the folder above it first.
//! - Move asks the app's Move dialog for a folder, handing it the folder the items leave (its
//!   name, identity and copy rule), then moves (or copies, as the dialog said) into the folder
//!   it picked, as a drop into it does (`drag/execute.rs`). The tree's folder, moved while it
//!   shows, gives way to the folder above it first, as for Remove, so the browser is never left
//!   inside a folder at a path it no longer has.
//! - Add picks files through the app and adds them (unrecorded: a folder's add has no undo).
//! - Open File Location asks the app where the item lives (`BrowserActions::locate`) and shows
//!   it (`BrowserState::show_location`).
//!
//! An empty name, or the name the row already has, changes nothing.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use agg_gui_files::{tr, ContainerActions, FolderLink, Item, PlaceId, Recorded};

use super::{can_move, Target};
use crate::actions::BuiltinAction;
use crate::dialogs::{Confirmation, CopiesTo, MoveRequest, NameRequest};
use crate::drag::execute::drop_into;
use crate::move_dialog::plan::FolderKey;
use crate::state::BrowserState;

/// Runs `action` in `state` (whether it can is the caller's check).
pub(crate) fn run(state: &Rc<RefCell<BrowserState>>, action: BuiltinAction) {
    crate::trace(|| format!("builtin {action:?} runs"));
    match action {
        BuiltinAction::Add => add(state),
        BuiltinAction::CreateFolder => create_folder(state),
        BuiltinAction::OpenFileLocation => open_file_location(state),
        BuiltinAction::Rename => rename(state),
        BuiltinAction::Move => move_items(state),
        BuiltinAction::Remove => remove(state),
    }
}

/// The app's dialogs, and `state` as a weak handle for an answer that comes later.
fn dialogs_of(
    state: &Rc<RefCell<BrowserState>>,
) -> (
    Rc<dyn crate::dialogs::BrowserDialogs>,
    Weak<RefCell<BrowserState>>,
) {
    let dialogs = Rc::clone(&state.borrow().config().dialogs);
    (dialogs, Rc::downgrade(state))
}

/// The name typed, when it is one to use: not empty, and not `current`.
fn usable(name: Option<String>, current: &str) -> Option<String> {
    name.filter(|n| !n.is_empty() && n != current)
}

fn create_folder(state: &Rc<RefCell<BrowserState>>) {
    let depth = state.borrow().depth().saturating_sub(1);
    let (dialogs, weak) = dialogs_of(state);
    let request = NameRequest::new(tr("Create Folder"), tr("Folder Name"), "", tr("Create"));
    dialogs.ask_name(
        request,
        Box::new(move |name| {
            let (Some(state), Some(name)) = (weak.upgrade(), usable(name, "")) else {
                return;
            };
            // A failure shows on the status line (`write_recorded`).
            let _ =
                BrowserState::write_recorded(&state, depth, |w| w.create_folder_recorded(&name));
        }),
    );
}

fn rename(state: &Rc<RefCell<BrowserState>>) {
    let target = Target::of(&state.borrow());
    let [item] = target.items.as_slice() else {
        return;
    };
    let item = Rc::clone(item);
    let (depth, from_tree) = (target.depth, target.from_tree);
    let current = item.name().to_string();
    let (dialogs, weak) = dialogs_of(state);
    let request = NameRequest::new(tr("Rename Item"), tr("Name"), current.clone(), tr("Rename"));
    dialogs.ask_name(
        request,
        Box::new(move |name| {
            let (Some(state), Some(name)) = (weak.upgrade(), usable(name, &current)) else {
                return;
            };
            let showing_it = from_tree
                && state.borrow().path_names().get(depth + 1) == Some(&current)
                && item.as_container_link().is_some();
            let renamed =
                BrowserState::write_recorded(&state, depth, |w| w.rename_recorded(&item, &name));
            if renamed.is_ok() && showing_it {
                show_renamed_folder(&state, depth, &name);
            }
        }),
    );
}

/// The tree's folder, renamed while it showed below the folder at `depth`: the browser goes back
/// to that folder and opens the folder by its new name, so the path and breadcrumbs say it.
fn show_renamed_folder(state: &Rc<RefCell<BrowserState>>, depth: usize, name: &str) {
    let mut s = state.borrow_mut();
    s.go_to(depth);
    let link: Option<Rc<dyn FolderLink>> = s.active_container().and_then(|folder| {
        folder
            .child_containers()
            .into_iter()
            .find(|l| l.name() == name)
    });
    if let Some(link) = link {
        s.open(link.as_ref());
    }
}

/// The question Remove asks for items in a folder in `place`: its title, message and button.
fn remove_question(place: Option<&PlaceId>) -> Confirmation {
    if place == Some(&PlaceId::COMPUTER) {
        if cfg!(windows) {
            return Confirmation::new(
                tr("Move to Recycle Bin?"),
                tr("The selected items will go to the Recycle Bin, where you can restore them."),
                tr("Move to Recycle Bin"),
            );
        }
        return Confirmation::new(
            tr("Move to Trash?"),
            tr("The selected items will go to the Trash, where you can restore them."),
            tr("Move to Trash"),
        );
    }
    Confirmation::new(
        tr("Remove Items?"),
        tr("Are you sure you want to remove the currently selected items?"),
        tr("Remove"),
    )
}

fn remove(state: &Rc<RefCell<BrowserState>>) {
    let (target, place) = {
        let s = state.borrow();
        let target = Target::of(&s);
        let place = s
            .path_containers()
            .nth(target.depth)
            .and_then(|f| f.place());
        (target, place)
    };
    if target.items.is_empty() {
        return;
    }
    let (dialogs, weak) = dialogs_of(state);
    let Target {
        depth,
        items,
        from_tree,
    } = target;
    dialogs.confirm(
        remove_question(place.as_ref()),
        Box::new(move |yes| {
            let Some(state) = weak.upgrade().filter(|_| yes) else {
                return;
            };
            // The tree's folder going while it shows: the folder above shows first.
            if from_tree && state.borrow().depth() > depth + 1 {
                state.borrow_mut().go_to(depth);
            }
            let _ = BrowserState::write_recorded(&state, depth, |w| w.remove_recorded(&items));
        }),
    );
}

fn move_items(state: &Rc<RefCell<BrowserState>>) {
    let (target, source, places) = {
        let s = state.borrow();
        let target = Target::of(&s);
        let source = s.open_path().get(target.depth).cloned();
        (target, source, Rc::clone(&s.config().places))
    };
    let Some(source) = source else {
        return;
    };
    let items: Vec<Rc<dyn Item>> = target.items.into_iter().filter(can_move).collect();
    if items.is_empty() {
        return;
    }
    let (place, name, key, keeps) = {
        let folder = source.borrow();
        let keeps = !folder
            .as_writable()
            .is_some_and(|w| w.allow_action(ContainerActions::RemoveItems));
        let key = FolderKey::of_folder(&**folder);
        (folder.place(), folder.name().to_string(), key, keeps)
    };
    // A folder that keeps its items copies them wherever they go.
    let copies_to: CopiesTo = Rc::new(move |target| {
        keeps || places.drop_copies(Some(&**source.borrow()), target.cloned(), false)
    });
    let (depth, from_tree) = (target.depth, target.from_tree);
    let (dialogs, weak) = dialogs_of(state);
    let request = MoveRequest::new(items.clone(), place).with_source(name, key, copies_to);
    dialogs.pick_move_target(
        request,
        Box::new(move |picked| {
            if let (Some(state), Some(pick)) = (weak.upgrade(), picked) {
                // The tree's folder leaving while it shows: the folder above shows first.
                if from_tree && !pick.copy && state.borrow().depth() > depth + 1 {
                    state.borrow_mut().go_to(depth);
                }
                let name = pick.target.name().to_string();
                drop_into(&state, pick.target, &name, &items, depth, pick.copy);
            }
        }),
    );
}

fn add(state: &Rc<RefCell<BrowserState>>) {
    let depth = state.borrow().depth().saturating_sub(1);
    let (dialogs, weak) = dialogs_of(state);
    dialogs.pick_files(Box::new(move |items| {
        let Some(state) = weak.upgrade().filter(|_| !items.is_empty()) else {
            return;
        };
        let _ =
            BrowserState::write_recorded(&state, depth, |w| Recorded::unrecorded(w.add(&items)));
    }));
}

fn open_file_location(state: &Rc<RefCell<BrowserState>>) {
    let (actions, snapshot, item) = {
        let s = state.borrow();
        let target = Target::of(&s);
        let item = match target.items.as_slice() {
            [item] => Rc::clone(item),
            _ => return,
        };
        (
            Rc::clone(&s.config().actions),
            s.snapshot_at(target.depth),
            item,
        )
    };
    let located = snapshot.and_then(|snapshot| actions.locate(&snapshot, &item));
    let mut s = state.borrow_mut();
    match located {
        Some(location) => s.show_location(item.as_ref(), location.base, location.folders),
        None => s.show_location(item.as_ref(), 0, None),
    }
}
