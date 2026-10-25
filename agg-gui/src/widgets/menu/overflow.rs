//! Vertical placement of popup panels: which way a panel opens, and how a
//! panel too tall for its room is clamped and scrolled.
//!
//! Ports agg-sharp's `PopupLayoutEngine.RecalculatePosition` (Gui/Menu/
//! PopupWidget.cs) for menus opened from a bar title, and `PopupWidget.
//! MakeMenuHaveScroll` / `SystemWindow.ShowPopup`'s clamp for context menus
//! and submenus:
//!
//! - A bar menu ([`MenuAnchorKind::Bar`] / [`MenuAnchorKind::BottomBar`])
//!   opens toward its preferred side if it fits there, otherwise toward the
//!   opposite side if it fits there, otherwise toward the roomier side
//!   (the preferred one on a tie) and scrolls within that room.
//! - A context menu or a submenu keeps its clamp into the window; one taller
//!   than the window fills it, shows its top, and scrolls.
//! - A submenu that fits beside its parent on neither side (a phone-width
//!   window) hangs from its row by the bar rule instead ([`place_hanging`]),
//!   so it is never clamped back over the row that closes it.
//!
//! agg-gui keeps every popup a window margin clear of the window edge
//! ([`super::MenuMetrics::window_margin`], 4 px by default) where C# sits
//! flush (margin `0`) and subtracts 5 px from the room of a scrolling drop down;
//! here the room on each side already excludes the margin, so a scrolling
//! panel is exactly as tall as its room.
//!
//! [`super::geometry::stack_layout_with_metrics`] calls [`place_vertical`]
//! for every level, and paint, hit testing, wheel and keyboard scrolling
//! (`state.rs`, `state_scroll.rs`) all read the [`PopupScroll`] it yields.

use crate::geometry::{Point, Rect};
use crate::widgets::scroll_view::{current_scroll_style, ScrollBarStyle};

use super::state::MenuAnchorKind;

/// A scrolling panel's scroll state, carried on its
/// [`PopupLayout`](super::geometry::PopupLayout).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PopupScroll {
    /// Height of every row laid end to end (the unclamped panel height).
    pub content_height: f64,
    /// How far the rows are scrolled up from the first row, in
    /// `0..=content_height - panel height`.
    pub offset: f64,
    /// The strip at the panel's right edge that holds the scroll bar.  The
    /// panel is this much wider than its rows, as C#'s `MakeMenuHaveScroll`
    /// widens the popup by a scroll bar.
    pub bar: Rect,
}

impl PopupScroll {
    /// The largest offset for a panel `height` tall.
    pub fn max_offset(&self, height: f64) -> f64 {
        (self.content_height - height).max(0.0)
    }
}

/// Everything about a stack of popups that is not in the item tree: how far
/// the opener reaches (so a flip clears it), where the menu's coordinate
/// space sits in root space, and each level's scroll offset.
pub(crate) struct StackPlacement<'a> {
    /// Height of the bar title that opened the menu; `0.0` for a point.
    pub anchor_span: f64,
    /// Root position of the space the anchor is given in, when known.  A
    /// [`super::MenuBar`] runs its popup in bar-local coordinates; once it
    /// knows its root origin the window can be mapped into that space and
    /// bar menus flip and scroll.  `None` keeps the bar's original
    /// unclamped downward placement.
    pub space_origin: Option<Point>,
    /// The stored scroll offset of the level whose path prefix is given.
    pub scroll: &'a dyn Fn(&[usize]) -> f64,
}

impl StackPlacement<'_> {
    /// No span, no known origin, every level at its first row.
    pub(crate) fn unscrolled() -> StackPlacement<'static> {
        StackPlacement {
            anchor_span: 0.0,
            space_origin: None,
            scroll: &|_| 0.0,
        }
    }
}

/// Width of the scroll bar strip a scrolling panel gains.
pub(crate) fn scroll_gutter(style: &ScrollBarStyle) -> f64 {
    style.bar_width + style.outer_margin + style.inner_margin
}

/// The current theme's scroll bar strip width.
pub(crate) fn current_scroll_gutter() -> f64 {
    scroll_gutter(&current_scroll_style())
}

/// One panel's bottom and height for content `h` tall.
///
/// `anchor_y` is where the panel hangs from (its top for `Bar` / `Context`,
/// its bottom for `BottomBar`).  `root` is true for the first level, the
/// only one a bar title opens; submenus clamp.  `bar_legacy` is the bar
/// placement with no known origin: `vis` is then not in the menu's space,
/// so the panel keeps hanging below the bar unclamped and never scrolls.
/// `margin` is the gap kept from the window edge.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_vertical(
    h: f64,
    anchor_y: f64,
    kind: MenuAnchorKind,
    vis: Rect,
    root: bool,
    anchor_span: f64,
    bar_legacy: bool,
    margin: f64,
) -> (f64, f64) {
    let floor = vis.y + margin;
    let ceiling = vis.y + vis.height - margin;
    if bar_legacy && kind == MenuAnchorKind::Bar {
        // Top bar with no root origin: hang below the anchor, allowing
        // negative bar-local y (the window's bottom is unknown here).
        let min_y = vis.y - vis.height;
        let y = (anchor_y - h).clamp(min_y, (ceiling - h).max(min_y));
        return (y, h);
    }
    if root && kind != MenuAnchorKind::Context {
        return place_from_bar(h, anchor_y, kind, floor, ceiling, anchor_span);
    }
    let room = (ceiling - floor).max(0.0);
    if h > room {
        // Taller than the window: show its top (agg-sharp
        // `PopupTallerThanWindowShowsItsTop`) and scroll the rest.
        return (ceiling - room, room);
    }
    let raw_y = match kind {
        MenuAnchorKind::BottomBar => anchor_y,
        MenuAnchorKind::Bar | MenuAnchorKind::Context => anchor_y - h,
    };
    (raw_y.clamp(floor, (ceiling - h).max(floor)), h)
}

/// One panel's bottom and height for a submenu that hangs from the row that
/// opened it (`anchor_y` its bottom for `Bar` / `Context`, its top for
/// `BottomBar`; `span` the row's height), as a phone-width window places
/// one that fits beside its parent on neither side.  The row must stay
/// visible, since a tap on it is the way back out of the submenu (`state.rs`
/// `handle_left_down`), so the panel follows the bar rule rather than the
/// submenu clamp: it opens toward the side it fits on, else scrolls in the
/// roomier one, and is never pushed back over the row.
pub(crate) fn place_hanging(
    h: f64,
    anchor_y: f64,
    kind: MenuAnchorKind,
    vis: Rect,
    span: f64,
    margin: f64,
) -> (f64, f64) {
    let floor = vis.y + margin;
    let ceiling = vis.y + vis.height - margin;
    place_from_bar(h, anchor_y, kind, floor, ceiling, span)
}

/// C#'s `PopupLayoutEngine` direction rule for a menu opened from a bar
/// title that reaches `span` above (`Bar`) or below (`BottomBar`) `anchor_y`.
fn place_from_bar(
    h: f64,
    anchor_y: f64,
    kind: MenuAnchorKind,
    floor: f64,
    ceiling: f64,
    span: f64,
) -> (f64, f64) {
    let span = if span.is_finite() { span.max(0.0) } else { 0.0 };
    // The anchor's bottom (where a downward panel's top sits) and top (where
    // an upward panel's bottom sits).
    let (down_edge, up_edge) = match kind {
        MenuAnchorKind::BottomBar => (anchor_y - span, anchor_y),
        _ => (anchor_y, anchor_y + span),
    };
    let below = (down_edge - floor).max(0.0);
    let above = (ceiling - up_edge).max(0.0);
    let prefers_down = kind != MenuAnchorKind::BottomBar;
    let (preferred, opposite) = if prefers_down {
        (below, above)
    } else {
        (above, below)
    };
    let mut down = prefers_down;
    let mut height = h;
    if h > preferred {
        if h <= opposite {
            down = !down;
        } else {
            // It fits nowhere: open toward whichever side has more room
            // (the preferred one on a tie) and scroll within it.
            if opposite > preferred {
                down = !down;
            }
            height = preferred.max(opposite);
        }
    }
    let y = if down { down_edge - height } else { up_edge };
    (y, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIS: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };

    #[test]
    fn rust_only_bar_menu_that_fits_below_keeps_opening_down() {
        let (y, h) = place_vertical(
            100.0,
            200.0,
            MenuAnchorKind::Bar,
            VIS,
            true,
            26.0,
            false,
            4.0,
        );
        assert_eq!((y, h), (100.0, 100.0));
    }

    #[test]
    fn rust_only_tie_keeps_the_preferred_side() {
        // 146 below, 146 above, 200 needed: stays down and scrolls.
        let (y, h) = place_vertical(
            200.0,
            150.0,
            MenuAnchorKind::Bar,
            VIS,
            true,
            0.0,
            false,
            4.0,
        );
        assert_eq!((y, h), (4.0, 146.0));
    }

    #[test]
    fn rust_only_legacy_bar_placement_is_unchanged() {
        let (y, h) = place_vertical(500.0, 0.0, MenuAnchorKind::Bar, VIS, true, 26.0, true, 4.0);
        assert_eq!((y, h), (-300.0, 500.0));
    }
}
