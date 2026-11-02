//! A drop's write: the dragged items move into the folder they were dropped on, or are copied
//! there. A drop and (later) the Move dialog's accept land here.
//!
//! The target is opened from its link first; a folder that can't be opened is an ordinary failed
//! move, reported. A target that is the items' own folder (the same place and location key,
//! `FolderKey`) takes nothing: the drag never offers it, and a drop that reaches it anyway does
//! nothing, as a move there would only rename the items. The drop copies when the drag asked for a copy (the copy modifier, or a source
//! that keeps its items, `PlaceRules::drop_copies`) or when the opened folders turn out to be in
//! two places (`PlaceRules::crosses_places`).
//!
//! A protected item never moves, as the browser's own Move and Remove never take one: a drop
//! holding one copies where the copy rule already copies, and otherwise moves nothing and says
//! why ("Protected items can't be moved").
//!
//! A move runs at once through the folders' recorded form (`WritableFolder::move_in_recorded`):
//! its step goes on the history, the source and any open ancestor that is the target list again,
//! the browser lists again, and a failure is reported naming what did move.
//!
//! A copy is first offered to the app (`PlaceRules::take_over_copy`, handed a `CopyCx` with the
//! items, the target, the source, a recorder for its history step and the report). Declined, the
//! browser runs it: through the target's own copy (`WritableFolder::copy_in`) on the config's
//! spawner, its step recorded when it lands through the recorder, or, for a folder without one,
//! by adding the items (`WritableFolder::add`), recorded when the folder records it. A failure is
//! reported as "Couldn't copy to ...". No hook runs while the browser's state is borrowed.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui_files::{names_of_items_gone_from_source, tr, Folder, FolderLink, Item, Recorded};

use crate::move_dialog::plan::FolderKey;
use crate::places::{CopyCx, CopyTakeover};
use crate::state::{BrowserState, OpenFolder};

/// "Couldn't move to "Brackets": why".
fn couldnt_move_to(name: &str, why: &str) -> String {
    format!("{} \"{name}\": {why}", tr("Couldn't move to"))
}

/// "Couldn't copy to "Brackets": why".
pub fn couldnt_copy_to(name: &str, why: &str) -> String {
    format!("{} \"{name}\": {why}", tr("Couldn't copy to"))
}

/// Opens `target_link` and moves (or copies) `items` into it from the folder at `source_depth`
/// of `state`'s path: [`drop_into`]. True when the move completed or the copy started.
pub fn drop_onto(
    state: &Rc<RefCell<BrowserState>>,
    target_link: &Rc<dyn FolderLink>,
    items: &[Rc<dyn Item>],
    source_depth: usize,
    copy_wanted: bool,
) -> bool {
    let target = match target_link.get_container() {
        Ok(target) => target,
        Err(e) => {
            let report = state.borrow().report();
            report(&couldnt_move_to(target_link.name(), &e));
            return false;
        }
    };
    drop_into(
        state,
        target,
        target_link.name(),
        items,
        source_depth,
        copy_wanted,
    )
}

/// Moves `items` into the opened folder `target` (named `target_name`) from the folder at
/// `source_depth`, or copies them (see the module header). True when the move completed or the
/// copy started.
pub fn drop_into(
    state: &Rc<RefCell<BrowserState>>,
    mut target: Box<dyn Folder>,
    target_name: &str,
    items: &[Rc<dyn Item>],
    source_depth: usize,
    copy_wanted: bool,
) -> bool {
    let (source, places, report) = {
        let s = state.borrow();
        let places = Rc::clone(&s.config().places);
        (s.open_path().get(source_depth).cloned(), places, s.report())
    };
    let into_itself = source.as_ref().is_some_and(|source| {
        let key = FolderKey::of_folder(&**source.borrow());
        key.is_some() && key == FolderKey::of_folder(target.as_ref())
    });
    if into_itself {
        crate::trace(|| format!("drop on {target_name}, the items' own folder: nothing moves"));
        return false;
    }
    target.load();
    let crosses = source
        .as_ref()
        .is_some_and(|source| places.crosses_places(&**source.borrow(), target.as_ref()));
    if copy_wanted || crosses {
        return copy_into(state, target, target_name, items, source);
    }
    let Some(source) = source else {
        return false;
    };
    if items.iter().any(|item| item.is_protected()) {
        report(&couldnt_move_to(
            target_name,
            &tr("Protected items can't be moved"),
        ));
        return false;
    }
    if target.as_writable_mut().is_none() {
        report(&couldnt_move_to(
            target_name,
            &tr("This folder can't be changed"),
        ));
        return false;
    }
    let recorded = {
        let mut source = source.borrow_mut();
        match (target.as_writable_mut(), source.as_writable_mut()) {
            (Some(target), Some(source)) => target.move_in_recorded(items, source),
            _ => Recorded::unrecorded(Err(tr("This folder can't be changed"))),
        }
    };
    let history = state.borrow().history();
    let result = history.keep(recorded);
    source.borrow_mut().load();
    if let Err(e) = &result {
        let gone = names_of_items_gone_from_source(items, &**source.borrow());
        let mut message = couldnt_move_to(target_name, e);
        if !gone.is_empty() {
            message += &format!(" ({}: {})", tr("Moved"), gone.join(", "));
        }
        report(&message);
    }
    reload_open_target(state, target.id(), source_depth);
    state.borrow_mut().after_write(String::new());
    crate::trace(|| format!("drop moved {} items to {target_name}", items.len()));
    result.is_ok()
}

/// The nearest open folder on the path with `target_id` lists itself again (the source, already
/// listed again, is skipped): the folder showing, when the drop landed in it, or one above it.
fn reload_open_target(state: &Rc<RefCell<BrowserState>>, target_id: &str, source_depth: usize) {
    let path = state.borrow().open_path();
    let open = path
        .iter()
        .rposition(|folder| folder.borrow().id() == target_id)
        .filter(|depth| *depth != source_depth);
    if let Some(depth) = open {
        path[depth].borrow_mut().load();
    }
}

/// Copies `items` into the opened folder `target` (see the module header). True when the copy
/// started.
fn copy_into(
    state: &Rc<RefCell<BrowserState>>,
    target: Box<dyn Folder>,
    target_name: &str,
    items: &[Rc<dyn Item>],
    source: Option<OpenFolder>,
) -> bool {
    let (places, spawner, report, history) = {
        let s = state.borrow();
        let config = s.config();
        (
            Rc::clone(&config.places),
            Rc::clone(&config.spawner),
            s.report(),
            s.history(),
        )
    };
    let cx = CopyCx {
        items: items.to_vec(),
        target,
        target_name: target_name.to_string(),
        source,
        recorder: history.recorder(),
        report: Rc::clone(&report),
    };
    let cx = match places.take_over_copy(cx) {
        CopyTakeover::Taken => {
            crate::trace(|| format!("copy to {target_name} taken over by the app"));
            return true;
        }
        CopyTakeover::Declined(cx) => cx,
    };
    let CopyCx {
        items,
        mut target,
        target_name,
        recorder,
        report,
        ..
    } = cx;
    let Some(writable) = target.as_writable() else {
        report(&couldnt_copy_to(
            &target_name,
            &tr("This folder can't be changed"),
        ));
        return false;
    };
    let done = {
        let (report, name, state) = (
            Rc::clone(&report),
            target_name.clone(),
            Rc::downgrade(state),
        );
        Box::new(move |recorded: Recorded| {
            if let Err(e) = recorder.keep(recorded) {
                report(&couldnt_copy_to(&name, &e));
            }
            if let Some(state) = state.upgrade() {
                state.borrow_mut().after_write(String::new());
            }
        })
    };
    if let Some(task) = writable.copy_in(items.clone(), done) {
        spawner.spawn_task(task);
        return true;
    }
    // A folder with no copy of its own takes copies through `add`.
    let result = match target.as_writable_mut() {
        Some(writable) => writable.add(&items),
        None => Err(tr("This folder can't be changed")),
    };
    if let Err(e) = &result {
        report(&couldnt_copy_to(&target_name, e));
    }
    state.borrow_mut().after_write(String::new());
    result.is_ok()
}
