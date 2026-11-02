//! The list's side of the selection gestures (`list/mod.rs` calls these): a left press on the
//! list's empty space starts a rubber band (`BrowserState::begin_band`), each move selects the
//! rows and tiles the band touches, and the band is drawn over them in the accent, its corners
//! rounded like theirs; and the keys the list takes while it has the keyboard (a press anywhere
//! in the list asks for it): Control+A (Command+A on a Mac) selects every row, Escape empties the
//! selection, and the arrow keys move it (Shift extends it from the anchor), Up and Down to the
//! nearest row or tile in the line above or below. Row rectangles are read from the list's own
//! widgets in the scrolled content's coordinates, which the band is kept in, so it stays on its
//! rows as the list scrolls. A folder's message (`list/message.rs`) stands alone in the list's
//! place with nothing to select: a press there starts no band.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult, Key, Modifiers, MouseButton, Point, Rect, Widget,
};

use crate::state::BrowserState;

use super::icon_view::BrowserTile;
use super::item_event::focus_list;
use super::message::FolderMessage;
use super::row::BrowserRow;

/// The band's fill opacity over the rows.
const BAND_FILL_ALPHA: f32 = 0.2;

/// The listing index of a row or tile widget; `None` for anything else.
fn listed_index(widget: &dyn Widget) -> Option<usize> {
    let any = widget.as_any()?;
    any.downcast_ref::<BrowserTile>()
        .map(BrowserTile::index)
        .or_else(|| any.downcast_ref::<BrowserRow>().map(BrowserRow::index))
}

/// The list's content (the rows or tiles, inside the list's `ScrollView`) and its origin in the
/// list's coordinates.
fn content(children: &[Box<dyn Widget>]) -> Option<(Point, &dyn Widget)> {
    let scroll = children.first()?;
    let stack = scroll.children().first()?;
    let (s, c) = (scroll.bounds(), stack.bounds());
    Some((Point::new(s.x + c.x, s.y + c.y), stack.as_ref()))
}

/// `pos` (the list's coordinates) in the content's coordinates.
fn to_content(children: &[Box<dyn Widget>], pos: Point) -> Option<Point> {
    let (origin, _) = content(children)?;
    Some(Point::new(pos.x - origin.x, pos.y - origin.y))
}

/// Every row or tile of the list with its rectangle in content coordinates, in widget order.
fn item_rects(children: &[Box<dyn Widget>]) -> Vec<(usize, Rect)> {
    let Some((_, stack)) = content(children) else {
        return Vec::new();
    };
    stack
        .children()
        .iter()
        .filter_map(|child| Some((listed_index(child.as_ref())?, child.bounds())))
        .collect()
}

/// Whether a folder's message stands in the list's place.
fn shows_folder_message(children: &[Box<dyn Widget>]) -> bool {
    children
        .first()
        .is_some_and(|child| child.as_any().is_some_and(|any| any.is::<FolderMessage>()))
}

fn intersects(a: &Rect, b: &Rect) -> bool {
    a.x <= b.x + b.width && b.x <= a.x + a.width && a.y <= b.y + b.height && b.y <= a.y + a.height
}

/// The rubber band's pointer handling: a left press on empty space starts it, moves while it is
/// held select what it covers. `None` for events the band leaves to the list.
pub fn band_pointer_event(
    state: &Rc<RefCell<BrowserState>>,
    children: &[Box<dyn Widget>],
    event: &Event,
) -> Option<EventResult> {
    match event {
        Event::MouseDown {
            button: MouseButton::Left,
            pos,
            modifiers,
        } => {
            // Only presses no row or scroll bar took reach the list: empty space.
            focus_list(state);
            if shows_folder_message(children) {
                return Some(EventResult::Consumed);
            }
            let at = to_content(children, *pos)?;
            state
                .borrow_mut()
                .begin_band(at, modifiers.ctrl || modifiers.meta);
            Some(EventResult::Consumed)
        }
        Event::MouseMove { pos } if state.borrow().is_banding() => {
            let at = to_content(children, *pos)?;
            let rect = state.borrow_mut().drag_band(at);
            if let Some(rect) = rect {
                let covered: Vec<usize> = item_rects(children)
                    .into_iter()
                    .filter(|(_, item)| intersects(item, &rect))
                    .map(|(index, _)| index)
                    .collect();
                state.borrow_mut().select_band(&covered);
            }
            Some(EventResult::Consumed)
        }
        Event::MouseCaptureLost if state.borrow().is_banding() => {
            state.borrow_mut().end_band(false);
            Some(EventResult::Consumed)
        }
        _ => None,
    }
}

/// Draws the band, if one is drawn, over the list `width` by `height` (clipped to it) in
/// `color`, its corners rounded by `radius` like the rows it selects.
pub fn paint_band(
    ctx: &mut dyn DrawCtx,
    state: &Rc<RefCell<BrowserState>>,
    children: &[Box<dyn Widget>],
    (width, height): (f64, f64),
    color: Color,
    radius: f64,
) {
    let Some(band) = state.borrow().band_rect() else {
        return;
    };
    let Some((origin, _)) = content(children) else {
        return;
    };
    let left = (band.x + origin.x).max(0.0);
    let bottom = (band.y + origin.y).max(0.0);
    let right = (band.x + band.width + origin.x).min(width);
    let top = (band.y + band.height + origin.y).min(height);
    if right <= left || top <= bottom {
        return;
    }
    ctx.set_fill_color(color.with_alpha(BAND_FILL_ALPHA));
    ctx.begin_path();
    ctx.rounded_rect(left, bottom, right - left, top - bottom, radius);
    ctx.fill();
    ctx.set_stroke_color(color);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    // The 1-wide outline is centred half a unit in, so its outer edge follows the fill's corners.
    ctx.rounded_rect(
        left + 0.5,
        bottom + 0.5,
        right - left - 1.0,
        top - bottom - 1.0,
        (radius - 0.5).max(0.0),
    );
    ctx.stroke();
}

/// The keys the focused list takes; anything else is left to the app.
pub fn list_key_event(
    state: &Rc<RefCell<BrowserState>>,
    children: &[Box<dyn Widget>],
    key: &Key,
    modifiers: Modifiers,
) -> EventResult {
    let command = modifiers.ctrl || modifiers.meta;
    match key {
        Key::Char(c) if command && c.eq_ignore_ascii_case(&'a') => {
            state.borrow_mut().select_all();
            EventResult::Consumed
        }
        Key::Escape if state.borrow_mut().clear_selection() => EventResult::Consumed,
        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown if !command => {
            if let Some(index) = arrow_target(state, children, key) {
                state.borrow_mut().move_selection_to(index, modifiers.shift);
            }
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}

/// The row an arrow key moves to from the lead row: the one before or after it for Left and
/// Right, the nearest one in the line above or below for Up and Down; the lead itself at an end;
/// the first row when nothing is selected.
fn arrow_target(
    state: &Rc<RefCell<BrowserState>>,
    children: &[Box<dyn Widget>],
    key: &Key,
) -> Option<usize> {
    let items = item_rects(children);
    let first = items.iter().map(|(index, _)| *index).min()?;
    let Some(lead) = state.borrow().lead_index() else {
        return Some(first);
    };
    let at = items.iter().position(|(index, _)| *index == lead)?;
    let center = |r: &Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let (cx, cy) = center(&items[at].1);
    let target = match key {
        Key::ArrowLeft => at.checked_sub(1).map(|i| items[i].0),
        Key::ArrowRight => items.get(at + 1).map(|(index, _)| *index),
        _ => {
            // Y is up: the line above has larger centres. The nearest line first, then the row
            // nearest across in it (the earlier one on a tie, as `min_by` keeps the first).
            let up = matches!(key, Key::ArrowUp);
            let away = |r: &Rect| match up {
                true => center(r).1 - cy,
                false => cy - center(r).1,
            };
            let line = items
                .iter()
                .map(|(_, r)| away(r))
                .filter(|d| *d > 0.5)
                .min_by(f64::total_cmp)?;
            items
                .iter()
                .filter(|(_, r)| (away(r) - line).abs() <= 0.5)
                .min_by(|(_, a), (_, b)| {
                    (center(a).0 - cx)
                        .abs()
                        .total_cmp(&(center(b).0 - cx).abs())
                })
                .map(|(index, _)| *index)
        }
    };
    target.or(Some(lead))
}
