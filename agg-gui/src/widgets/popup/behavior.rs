//! Open-state and close-behavior controller for anchored popups.
//!
//! [`Popup`] is a small, content-agnostic controller: it owns the open flag,
//! the anchor rectangle, the [`RectAlign`] placement, the pixel gap, and the
//! [`PopupCloseBehavior`]. A host widget drives it — feeds the anchor and the
//! measured content size, asks for the placed rect to paint, and forwards mouse
//! events so the controller can apply the close behavior.
//!
//! This deliberately does **not** re-implement overlay painting, hit testing,
//! keyboard navigation, or submenu cascades — the menu system
//! (`super::super::menu`) already owns all of that. When a popup needs rich
//! nested-menu content, host it with a [`PopupMenu`](super::super::menu::PopupMenu),
//! which reuses the whole menu machinery. `Popup` adds the one thing the menu
//! system can't express on its own: arbitrary [`RectAlign`] placement plus a
//! selectable close behavior.

use crate::geometry::{Point, Rect, Size};

use super::align::{clamp_rect, RectAlign};

/// When a popup should dismiss itself in response to clicks. Mirrors egui's
/// `PopupCloseBehavior`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupCloseBehavior {
    /// Close as soon as the user clicks anything — inside or outside the popup.
    /// Suits menu-style popups where any activation dismisses the menu.
    CloseOnClick,
    /// Close only when the user clicks outside the popup. Clicks inside are
    /// captured but keep it open — good for popups with interactive content.
    CloseOnClickOutside,
    /// Never close on a click; only an explicit toggle / [`Popup::close`] does.
    IgnoreClicks,
}

impl Default for PopupCloseBehavior {
    fn default() -> Self {
        Self::CloseOnClickOutside
    }
}

impl PopupCloseBehavior {
    /// The three behaviors in a stable order with a label and a one-line
    /// explanation — the demo uses these to build its selector + tooltips.
    pub const ALL: [(Self, &'static str, &'static str); 3] = [
        (
            Self::CloseOnClick,
            "CloseOnClick",
            "Close as soon as the user clicks anywhere, inside or outside.",
        ),
        (
            Self::CloseOnClickOutside,
            "CloseOnClickOutside",
            "Close only when the user clicks outside the popup.",
        ),
        (
            Self::IgnoreClicks,
            "IgnoreClicks",
            "Never close on a click — only an explicit toggle closes it.",
        ),
    ];

    /// Index of this behavior within [`PopupCloseBehavior::ALL`].
    pub fn all_index(self) -> usize {
        Self::ALL
            .iter()
            .position(|(b, _, _)| *b == self)
            .unwrap_or(0)
    }
}

/// What a forwarded mouse-down did to a popup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PopupClickOutcome {
    /// The click landed inside the popup's placed rect.
    pub inside: bool,
    /// The click closed the popup.
    pub closed: bool,
    /// The host should treat the event as consumed (don't pass it through).
    pub consumed: bool,
}

/// Controller for a single anchored popup surface.
#[derive(Clone, Debug)]
pub struct Popup {
    open: bool,
    anchor: Rect,
    size: Size,
    pub align: RectAlign,
    pub gap: f64,
    pub close_behavior: PopupCloseBehavior,
}

impl Default for Popup {
    fn default() -> Self {
        Self {
            open: false,
            anchor: Rect::default(),
            size: Size::new(0.0, 0.0),
            align: RectAlign::BOTTOM_START,
            gap: 4.0,
            close_behavior: PopupCloseBehavior::default(),
        }
    }
}

impl Popup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// Flip the open flag; returns the new state.
    pub fn toggle(&mut self) -> bool {
        self.open = !self.open;
        self.open
    }

    /// Record the anchor rect (the opener widget's bounds, in the coordinate
    /// space the popup is placed and hit-tested in).
    pub fn set_anchor(&mut self, anchor: Rect) {
        self.anchor = anchor;
    }

    pub fn anchor(&self) -> Rect {
        self.anchor
    }

    /// Record the measured content size used for placement.
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// The placed, viewport-clamped popup rect.
    pub fn rect(&self, viewport: Size) -> Rect {
        clamp_rect(
            self.align.place_child(self.anchor, self.size, self.gap),
            viewport,
        )
    }

    /// Whether `pos` falls inside the placed popup rect.
    pub fn contains(&self, pos: Point, viewport: Size) -> bool {
        let r = self.rect(viewport);
        pos.x >= r.x && pos.x <= r.x + r.width && pos.y >= r.y && pos.y <= r.y + r.height
    }

    /// Apply the close behavior to a left mouse-down at `pos`.
    ///
    /// Returns the [`PopupClickOutcome`] so the host knows whether the popup
    /// closed and whether to consume the event. A no-op (returns all-false)
    /// when the popup is already closed.
    pub fn on_mouse_down(&mut self, pos: Point, viewport: Size) -> PopupClickOutcome {
        if !self.open {
            return PopupClickOutcome::default();
        }
        let inside = self.contains(pos, viewport);
        let closed = match self.close_behavior {
            PopupCloseBehavior::CloseOnClick => true,
            PopupCloseBehavior::CloseOnClickOutside => !inside,
            PopupCloseBehavior::IgnoreClicks => false,
        };
        if closed {
            self.open = false;
        }
        // Swallow the event whenever the click was inside the popup (so it
        // doesn't fall through to whatever is behind it) or whenever it caused
        // the popup to close.
        let consumed = inside || closed;
        PopupClickOutcome {
            inside,
            closed,
            consumed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup(behavior: PopupCloseBehavior) -> Popup {
        let mut p = Popup::new();
        p.close_behavior = behavior;
        p.align = RectAlign::BOTTOM_START;
        p.gap = 4.0;
        p.set_anchor(Rect::new(100.0, 200.0, 40.0, 20.0));
        p.set_size(Size::new(80.0, 60.0));
        p.open();
        p
    }

    const VIEWPORT: Size = Size {
        width: 800.0,
        height: 600.0,
    };

    fn inside_point(p: &Popup) -> Point {
        let r = p.rect(VIEWPORT);
        Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
    }

    #[test]
    fn close_on_click_closes_from_inside_or_outside() {
        let mut p = popup(PopupCloseBehavior::CloseOnClick);
        let inside = inside_point(&p);
        let out = p.on_mouse_down(inside, VIEWPORT);
        assert!(out.inside && out.closed && out.consumed);
        assert!(!p.is_open());

        let mut p = popup(PopupCloseBehavior::CloseOnClick);
        let out = p.on_mouse_down(Point::new(5.0, 5.0), VIEWPORT);
        assert!(!out.inside && out.closed && out.consumed);
        assert!(!p.is_open());
    }

    #[test]
    fn close_on_click_outside_keeps_open_when_inside() {
        let mut p = popup(PopupCloseBehavior::CloseOnClickOutside);
        let inside = inside_point(&p);
        let out = p.on_mouse_down(inside, VIEWPORT);
        assert!(out.inside && !out.closed && out.consumed);
        assert!(p.is_open(), "click inside keeps a CloseOnClickOutside popup open");

        let out = p.on_mouse_down(Point::new(5.0, 5.0), VIEWPORT);
        assert!(!out.inside && out.closed && out.consumed);
        assert!(!p.is_open());
    }

    #[test]
    fn ignore_clicks_never_closes_but_swallows_inside_clicks() {
        let mut p = popup(PopupCloseBehavior::IgnoreClicks);
        let inside = inside_point(&p);
        let out = p.on_mouse_down(inside, VIEWPORT);
        assert!(out.inside && !out.closed && out.consumed);
        assert!(p.is_open());

        // Outside click falls through and leaves it open.
        let out = p.on_mouse_down(Point::new(5.0, 5.0), VIEWPORT);
        assert!(!out.inside && !out.closed && !out.consumed);
        assert!(p.is_open());
    }

    #[test]
    fn closed_popup_ignores_clicks() {
        let mut p = popup(PopupCloseBehavior::CloseOnClick);
        p.close();
        let out = p.on_mouse_down(Point::new(120.0, 190.0), VIEWPORT);
        assert_eq!(out, PopupClickOutcome::default());
    }

    #[test]
    fn behavior_table_indices_round_trip() {
        for (i, (b, _, _)) in PopupCloseBehavior::ALL.iter().enumerate() {
            assert_eq!(b.all_index(), i);
        }
    }
}
