//! The "safe triangle" of each open menu level: whether the pointer,
//! crossing a row, is on its way into the submenu open beside that level
//! rather than choosing the row.  Ports agg-sharp `SubMenuAimWedge`
//! (Gui/Menu/SubMenuAimWedge.cs); `PopupMenuState::update_hover`
//! (`state.rs`) consults it the way agg-sharp `PopupMenu.OnRowHover` does.
//!
//! A submenu hangs down from the row that opened it, so every row of it
//! below the first is reached by moving down and to the right, a path that
//! crosses the rows under the opening row.  The wedge decides on geometry
//! alone (no dwell timer): the apex is where the pointer last was on a row of
//! the level, and the pointer is in transit while it stays inside the
//! triangle from the apex to the submenu's near edge.  A pointer moving
//! along the menu (same x) is never inside it, so hovering a sibling still
//! closes the submenu.

use crate::geometry::{Point, Rect};

/// One apex per menu level, keyed by the level's path prefix.
#[derive(Clone, Debug, Default)]
pub(super) struct AimWedges {
    apexes: Vec<(Vec<usize>, Point)>,
}

impl AimWedges {
    /// Makes `pointer`, where the pointer just was on a row of `level`, that
    /// level's apex.
    pub(super) fn remember(&mut self, level: &[usize], pointer: Point) {
        match self.apexes.iter_mut().find(|(l, _)| l == level) {
            Some((_, apex)) => *apex = pointer,
            None => self.apexes.push((level.to_vec(), pointer)),
        }
    }

    /// Drops `level`'s apex, so the next hover on one of its rows is judged
    /// on its own.
    pub(super) fn forget(&mut self, level: &[usize]) {
        self.apexes.retain(|(l, _)| l != level);
    }

    pub(super) fn clear(&mut self) {
        self.apexes.clear();
    }

    /// True when `pointer` is between `level`'s apex and the near edge of
    /// `sub_menu` (the open submenu's panel): on its way in.
    pub(super) fn aiming_at(&self, level: &[usize], pointer: Point, sub_menu: Rect) -> bool {
        let Some(apex) = self
            .apexes
            .iter()
            .find(|(l, _)| l == level)
            .map(|(_, p)| *p)
        else {
            return false;
        };
        if sub_menu.width <= 0.0 || sub_menu.height <= 0.0 {
            return false;
        }
        // The edge the pointer has to cross to get in.  A submenu that had
        // to open to the left is entered through its right edge instead.
        let edge_x = if sub_menu.x >= apex.x {
            sub_menu.x
        } else {
            sub_menu.x + sub_menu.width
        };
        let to_edge = edge_x - apex.x;
        let travelled = pointer.x - apex.x;
        if to_edge * travelled <= 0.0 || travelled.abs() > to_edge.abs() {
            // Not headed for the edge at all, or already past it.
            return false;
        }
        point_in_triangle(
            pointer,
            apex,
            Point::new(edge_x, sub_menu.y),
            Point::new(edge_x, sub_menu.y + sub_menu.height),
        )
    }
}

/// Half-plane test; points on an edge count as inside, so a pointer skimming
/// the wedge boundary is not rejected.
fn point_in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let side = |from: Point, to: Point| {
        (to.x - from.x) * (p.y - from.y) - (to.y - from.y) * (p.x - from.x)
    };
    let ab = side(a, b);
    let bc = side(b, c);
    let ca = side(c, a);
    (ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0)
}
