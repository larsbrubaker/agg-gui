//! A row's or tile's pointer handling, shared by both views (`list/row.rs`, `list/icon_view.rs`).
//! A left press hands the press (with its keys) to the browser's state, which selects and
//! detects the double click (`BrowserState::press`), and gives the list the keys; its release
//! settles a selection kept for a drag (`BrowserState::release_press`). A press whose double
//! click activated the row ends there and never drags: what it opened may take the list off
//! screen, which then never hears the release. A press that moves far enough is a drag
//! (`list/press.rs`), whose release is no click. A right press selects the row (Control or
//! Command joins it to the selection) and asks for the selection's menu, and is left to bubble
//! to the list, which opens the menu where it was pressed; a click on the menu button a hovered
//! row draws asks for the same menu hanging from the button (`list/menu_button.rs`). A press
//! that loses the pointer without a release (a finger lifting, the window deactivating) ends
//! without a click: no selection settled, no menu, no tap. A drag that ends outside the list
//! gives the keys back (`drag_ended`), so the view it ended over has them.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{Event, EventResult, MouseButton, Point, Size};

use crate::state::BrowserState;

use super::menu_button::{in_menu_button, menu_button_rect};
use super::press::pointer;

/// Gives the list the keys: a press in the list (on a row or its empty space) asks for them.
pub fn focus_list(state: &Rc<RefCell<BrowserState>>) {
    let id = state.borrow().list_focus_id();
    agg_gui::focus::request_focus(id);
}

/// Row `index`'s handling of `event`, for a row whose own box (without the margins its view
/// adds) starts at `origin` in the widget and is `size`.
pub fn item_event(
    event: &Event,
    state: &Rc<RefCell<BrowserState>>,
    index: usize,
    origin: Point,
    size: Size,
) -> EventResult {
    let local = |pos: &Point| Point::new(pos.x - origin.x, pos.y - origin.y);
    match event {
        Event::MouseEnter => {
            state.borrow_mut().menu_button_mut().hover(index, true);
            agg_gui::animation::request_draw();
            EventResult::Ignored
        }
        Event::MouseLeave => {
            state.borrow_mut().menu_button_mut().hover(index, false);
            agg_gui::animation::request_draw();
            EventResult::Ignored
        }
        Event::MouseMove { pos } => match press_moved(state, *pos) {
            true => EventResult::Consumed,
            false => EventResult::Ignored,
        },
        Event::MouseDown {
            button: MouseButton::Left,
            pos,
            modifiers,
        } => {
            focus_list(state);
            let on_button = in_menu_button(local(pos), size.width, size.height);
            crate::trace(|| format!("tile down #{index} on_button={on_button}"));
            state.borrow_mut().menu_button_mut().press(index, on_button);
            // The activation a double click makes runs here, the state let go.
            let double = BrowserState::press(state, index, *modifiers);
            let mut s = state.borrow_mut();
            match double {
                true => {
                    s.item_press_mut().end();
                }
                false => s.item_press_mut().begin(pointer(*pos)),
            }
            EventResult::Consumed
        }
        Event::MouseDown {
            button: MouseButton::Right,
            modifiers,
            ..
        } => {
            crate::trace(|| format!("tile right down #{index}"));
            let mut s = state.borrow_mut();
            // Command joins as Control does (the Mac's selection modifier).
            s.ensure_selected(index, modifiers.ctrl || modifiers.meta);
            // This menu opens where it was pressed, not from a button.
            s.menu_button_mut().take_anchor();
            s.request_item_menu();
            EventResult::Ignored
        }
        Event::MouseUp {
            button: MouseButton::Left,
            pos,
            ..
        } => release(state, index, *pos, local(pos), size),
        Event::MouseCaptureLost => match press_lost(state, Some(index)) {
            true => EventResult::Consumed,
            false => EventResult::Ignored,
        },
        _ => EventResult::Ignored,
    }
}

/// The release of a left press on row `index` at `pos` (`here` in the row's own box `size`): a
/// drag's release is no click; a click on the menu button asks for the item menu hanging from it
/// and bubbles to the list, which opens it; a tap activates where taps do.
fn release(
    state: &Rc<RefCell<BrowserState>>,
    index: usize,
    pos: Point,
    here: Point,
    size: Size,
) -> EventResult {
    let dragged = {
        let mut s = state.borrow_mut();
        let dragged = s.item_press_mut().end();
        s.release_press(dragged);
        dragged
    };
    if dragged {
        drag_ended(state, pointer(pos));
    }
    let on_button = in_menu_button(here, size.width, size.height);
    let menu = state
        .borrow_mut()
        .menu_button_mut()
        .release(index, on_button)
        && !dragged;
    crate::trace(|| format!("tile up #{index} dragged={dragged} menu={menu}"));
    if let Some(button) = menu_button_rect(size.width, size.height).filter(|_| menu) {
        // The row's window origin is the pointer less its row-local position.
        let at = pointer(pos);
        let anchor = Point::new(at.x - here.x + button.x, at.y - here.y + button.y);
        state.borrow_mut().menu_button_mut().set_anchor(anchor);
    }
    BrowserState::finish_tap(state, index, !dragged && !menu);
    if menu {
        state.borrow_mut().request_item_menu();
        return EventResult::Ignored;
    }
    EventResult::Consumed
}

/// A press's pointer move to `pos`: whether the press is a drag. A press whose left button is no
/// longer held (released where the list never heard it) ends here without a click.
pub fn press_moved(state: &Rc<RefCell<BrowserState>>, pos: Point) -> bool {
    if state.borrow().item_press().released_unheard() {
        press_lost(state, None);
        return false;
    }
    let dragging = state.borrow_mut().item_press_mut().moved(pointer(pos));
    if dragging {
        agg_gui::animation::request_draw();
    }
    dragging
}

/// A press that ended without the list hearing its release, on row `index` when it still holds
/// the press: never a click (the selection kept for a drag stays, no menu opens, no tap
/// activates). False with no press under way.
pub fn press_lost(state: &Rc<RefCell<BrowserState>>, index: Option<usize>) -> bool {
    if !state.borrow().item_press().is_pressed() {
        return false;
    }
    let (dragged, at) = {
        let mut s = state.borrow_mut();
        let at = s.item_press().last_pointer();
        let dragged = s.item_press_mut().end();
        s.release_press(true);
        (dragged, at)
    };
    if let (true, Some(at)) = (dragged, at) {
        drag_ended(state, at);
    }
    match index {
        Some(index) => {
            state.borrow_mut().menu_button_mut().release(index, false);
            BrowserState::finish_tap(state, index, false);
        }
        // The row the press began on is gone: no tap of it may activate later.
        None => state.borrow_mut().begin_tap(None),
    }
    true
}

/// A drag ended at `at` (window coordinates): outside the list, the list gives up the keys.
fn drag_ended(state: &Rc<RefCell<BrowserState>>, at: Point) {
    let s = state.borrow();
    if s.item_press().list_contains(at) == Some(false) {
        crate::trace(|| "drag ended outside the list: keys given back".to_string());
        agg_gui::focus::release_focus(s.list_focus_id());
    }
}
