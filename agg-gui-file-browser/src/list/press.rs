//! A press on a row or tile and whether it has become a drag ([`ItemPress`], kept in
//! `BrowserState::item_press` so it survives the list being rebuilt under the pointer: a press
//! selects its row, which rebuilds the list). Positions are window coordinates
//! (`agg_gui::current_mouse_world`), for the same reason.
//!
//! A press that moves more than [`DRAG_THRESHOLD`] is a drag: its release is no click (the
//! selection it kept for the drag stays, no menu opens, no tap activates). What a drag carries
//! and where it drops is the browser's drag and drop. A press is under way only while its left
//! button is held (`agg_gui::mouse_button_held`): one released where the list never heard it (a
//! page that left the screen mid-press) ends as a cancel at the next move
//! ([`ItemPress::released_unheard`]).
//!
//! The press also keeps where the list is in the window (`set_list_rect`, recorded as the list
//! paints): a drag that ends outside the browser gives the keys back (`list/item_event.rs`), so Delete
//! where the items were dropped is that view's, not the list's.

use agg_gui::{MouseButton, Point, Rect};

/// How far a press must move to become a drag (more than 40 units), so a click with a slight
/// wobble stays a click.
pub const DRAG_THRESHOLD: f64 = 40.0;

/// The pointer in window coordinates, falling back to `local` when none was recorded.
pub fn pointer(local: Point) -> Point {
    agg_gui::current_mouse_world().unwrap_or(local)
}

/// One press on a row or tile and the drag it may become.
#[derive(Debug, Default)]
pub struct ItemPress {
    /// Where the press was (window coordinates), while the button is down.
    down_at: Option<Point>,
    /// Where the pointer last was during the press (window coordinates).
    last_at: Option<Point>,
    /// The press moved far enough to be a drag.
    dragging: bool,
    /// The list's window rectangle (logical units, Y up), from its last paint.
    list_rect: Option<Rect>,
}

impl ItemPress {
    /// A press at `at` (window coordinates) starts watching for a drag.
    pub fn begin(&mut self, at: Point) {
        self.down_at = Some(at);
        self.last_at = Some(at);
        self.dragging = false;
    }

    /// Whether a press is under way.
    pub fn is_pressed(&self) -> bool {
        self.down_at.is_some()
    }

    /// Whether the press under way has become a drag.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Where the press under way was (window coordinates).
    pub fn pressed_at(&self) -> Option<Point> {
        self.down_at
    }

    /// Where the pointer last was during the press under way (window coordinates).
    pub fn last_pointer(&self) -> Option<Point> {
        self.down_at.and(self.last_at)
    }

    /// Whether a press is under way whose left button is no longer held: its release went where
    /// the list never heard it.
    pub fn released_unheard(&self) -> bool {
        self.down_at.is_some() && !agg_gui::mouse_button_held(MouseButton::Left)
    }

    /// The pointer moved to `at` (window coordinates) during the press: whether the press is a
    /// drag now. False with no press under way.
    pub fn moved(&mut self, at: Point) -> bool {
        let Some(down) = self.down_at else {
            return false;
        };
        self.last_at = Some(at);
        let (dx, dy) = (at.x - down.x, at.y - down.y);
        if !self.dragging && (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD {
            self.dragging = true;
            crate::trace(|| "item press became a drag".to_string());
        }
        self.dragging
    }

    /// Records the list's window rectangle (logical units, Y up).
    pub fn set_list_rect(&mut self, rect: Rect) {
        self.list_rect = Some(rect);
    }

    /// Whether `at` (window coordinates) is over the list; `None` before the list has painted.
    pub fn list_contains(&self, at: Point) -> Option<bool> {
        let r = self.list_rect?;
        Some(at.x >= r.x && at.y >= r.y && at.x <= r.x + r.width && at.y <= r.y + r.height)
    }

    /// The press ended (released, cancelled or its capture lost): whether it was a drag.
    pub fn end(&mut self) -> bool {
        self.down_at = None;
        self.last_at = None;
        std::mem::take(&mut self.dragging)
    }
}
