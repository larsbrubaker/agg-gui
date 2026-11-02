//! The drag's side of a press on a row or tile (`list/item_event.rs` calls these): a press that
//! starts in the list and moves past the drag threshold carries the selection (what the press
//! selected, read once as the drag starts) from the folder showing; each move highlights the
//! folder under the pointer and labels what a drop there does with the modifiers held now; the
//! release drops there (`execute.rs`); a folder where the drop would move a protected item is not
//! highlighted, as the drop moves nothing there, and a press lost while the window is still active (a
//! finger lifting) drops where the pointer last was, while one lost to another window drops
//! nothing. A drag held over a folder waits to open it (`spring.rs`); in a folder the list opened
//! that way, a release on the list's empty space drops into it.
//!
//! Outside the browser (neither over the list nor over the folder tree) the drag asks the app's
//! `OutsideDrops` hook what is under it, with the state let go: a view of the app's may take the
//! items, and may show them itself, which hides the drag's picture. A release there hands the
//! items to the hook's `release`; coming back into the browser, or the drag ending any other way
//! after it was out there, calls `cancel`, so the app's view puts itself back.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{Modifiers, Point};
use agg_gui_files::{tr, Item};

use super::{DropLabel, LIST_DROP_KEY};

/// The drag at `at` outside the browser asks the app what is there; back inside, the app's view
/// is told to put itself back.
fn outside_over(state: &Rc<RefCell<BrowserState>>, at: Point) {
    let (drops, inside, was_out, items) = {
        let s = state.borrow();
        let drops = s.config().drops.clone();
        let inside = s.drop.browser_contains(at);
        (
            drops,
            inside,
            s.drop.outside.is_some(),
            s.drop.dragged_items().to_vec(),
        )
    };
    let Some(drops) = drops else {
        return;
    };
    let over = match inside {
        true => {
            if was_out {
                drops.cancel();
            }
            None
        }
        false => Some(drops.over(&items, at)),
    };
    if over != state.borrow().drop.outside {
        agg_gui::animation::request_draw();
    }
    state.borrow_mut().drop.outside = over;
}
use crate::move_dialog::plan::FolderKey;
use crate::places::copy_modifier_held;
use crate::state::BrowserState;

/// The selected rows' items, what a drag carries.
fn selected_items(state: &BrowserState) -> Vec<Rc<dyn Item>> {
    state.selected_entries().iter().map(|e| e.item()).collect()
}

/// The drag of a press is at `at` (window coordinates): it starts carrying the selection if it
/// began in the list, and highlights and labels the folder under it.
pub fn drag_over(state: &Rc<RefCell<BrowserState>>, at: Point) {
    {
        let mut s = state.borrow_mut();
        let pressed_in_list = s
            .item_press()
            .pressed_at()
            .is_some_and(|p| s.item_press().list_contains(p) != Some(false));
        if !s.drop.is_dragging() && pressed_in_list {
            let items = selected_items(&s);
            if !items.is_empty() {
                let source = s.depth().saturating_sub(1);
                let key = s
                    .open_path()
                    .get(source)
                    .and_then(|folder| FolderKey::of_folder(&**folder.borrow()));
                s.drop.begin(items, source);
                s.drop.set_source_key(key);
            }
        }
        if !s.drop.is_dragging() {
            return;
        }
    }
    outside_over(state, at);
    let modifier = copy_modifier_held(agg_gui::event::current_modifiers());
    let target = {
        let mut s = state.borrow_mut();
        let spring = s.drop.spring_target_at(at);
        s.drop
            .spring
            .hover(spring.as_ref().map(|(key, link)| (*key, link.as_ref())));
        let showing = s.depth().saturating_sub(1);
        let target = s.drop.target_link_at(at).or_else(|| {
            s.drop
                .dropped_into_showing(at, showing)
                .map(|link| (LIST_DROP_KEY, link))
        });
        target.map(|(key, link)| {
            let place = s.drop.place_of(link.as_ref());
            (key, link, place)
        })
    };
    let Some((key, link, place)) = target else {
        state.borrow_mut().drop.set_highlight(None, None);
        return;
    };
    let (places, source) = {
        let s = state.borrow();
        let source = s.open_path().get(s.drop.source_depth()).cloned();
        (Rc::clone(&s.config().places), source)
    };
    let copies = match &source {
        Some(source) => places.drop_copies(Some(&**source.borrow()), place, modifier),
        None => places.drop_copies(None, place, modifier),
    };
    // A protected item never moves (`execute.rs`): a drop that would move one is not offered.
    let protected = state
        .borrow()
        .drop
        .dragged_items()
        .iter()
        .any(|item| item.is_protected());
    if protected && !copies {
        state.borrow_mut().drop.set_highlight(None, None);
        return;
    }
    let verb = match copies {
        true => tr("Copy to {0}"),
        false => tr("Move to {0}"),
    };
    let label = DropLabel {
        copies,
        text: verb.replace("{0}", link.name()),
    };
    state
        .borrow_mut()
        .drop
        .set_highlight(Some(key), Some(label));
}

/// The drag of a press was released at `at` with `modifiers`: the items drop on the folder there,
/// if one takes them. True when the move completed or the copy started.
pub fn drag_end(state: &Rc<RefCell<BrowserState>>, at: Point, modifiers: Modifiers) -> bool {
    let (target, items, source_depth, places, source, outside) = {
        let mut s = state.borrow_mut();
        if !s.drop.is_dragging() {
            return false;
        }
        let showing = s.depth().saturating_sub(1);
        let into_showing = s.drop.dropped_into_showing(at, showing);
        let target = s
            .drop
            .target_link_at(at)
            .or_else(|| into_showing.map(|link| (LIST_DROP_KEY, link)));
        let outside = s.drop.outside.is_some() && !s.drop.browser_contains(at);
        let source_depth = s.drop.source_depth();
        let items = s.drop.take_items().unwrap_or_default();
        let source = s.open_path().get(source_depth).cloned();
        (
            target,
            items,
            source_depth,
            Rc::clone(&s.config().places),
            source,
            outside.then(|| s.config().drops.clone()).flatten(),
        )
    };
    agg_gui::animation::request_layout();
    if let Some(drops) = outside {
        crate::trace(|| "drag released outside the browser: handed to the app".to_string());
        return drops.release(&items, at);
    }
    let Some((_, link)) = target else {
        return false;
    };
    // The copy the drag itself asks for (the modifier, a source that keeps its items); a drop
    // between places copies too, decided once the target is open.
    let copy_wanted = match &source {
        Some(source) => places.drop_copies(
            Some(&**source.borrow()),
            None,
            copy_modifier_held(modifiers),
        ),
        None => true,
    };
    super::execute::drop_onto(state, &link, &items, source_depth, copy_wanted)
}

/// A press that ended without its release being heard, after it became a drag: with the window
/// still active it drops where the pointer last was (`at`); otherwise nothing drops.
pub fn drag_lost(state: &Rc<RefCell<BrowserState>>, at: Option<Point>) {
    match at.filter(|_| agg_gui::event::window_is_active()) {
        Some(at) => {
            drag_end(state, at, Modifiers::default());
        }
        None => {
            let (drops, was_out) = {
                let mut s = state.borrow_mut();
                let was_out = s.drop.outside.is_some();
                s.drop.cancel();
                (s.config().drops.clone(), was_out)
            };
            if let (Some(drops), true) = (drops, was_out) {
                drops.cancel();
            }
        }
    }
}
