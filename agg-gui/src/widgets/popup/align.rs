//! Anchor-relative placement math for popups — this framework's answer to
//! egui's `RectAlign`.
//!
//! A popup attaches to an *anchor* rectangle (usually the bounds of the widget
//! that opened it). [`RectAlign`] names a point on that anchor (`parent`) and a
//! point on the popup (`child`); [`RectAlign::place_child`] positions the popup
//! so its `child` point coincides with the anchor's `parent` point, then pushes
//! it `gap` pixels outward.
//!
//! ## Y-up note
//!
//! Unlike egui (Y-down), this framework is first-quadrant / **Y-up**: `y` grows
//! upward and the origin is the bottom-left. Consequently `Align::Max` on the Y
//! axis is the **top** edge and `Align::Min` is the **bottom** edge. The named
//! presets below (`TOP`, `BOTTOM`, …) are expressed so they land on the same
//! *visual* side a user expects, not the numeric side egui's constants use.
//!
//! This module is the one genuinely new primitive the Popup API adds on top of
//! the existing menu system (`super::super::menu`), which can only anchor a
//! popup left-aligned and extending straight up or down. The placement is a
//! pure function so it is unit-tested directly against production code.

use crate::geometry::{Rect, Size};

/// Viewport margin kept clear when clamping a popup on-screen. Matches the
/// menu system's `MARGIN` so popups and menus hug the viewport edge the same.
const MARGIN: f64 = 4.0;

/// One-axis alignment: the fraction of an extent an anchor sits at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Start of the axis (0.0). On Y this is the **bottom** edge (Y-up).
    Min,
    /// Middle of the axis (0.5).
    Center,
    /// End of the axis (1.0). On Y this is the **top** edge (Y-up).
    Max,
}

impl Align {
    /// Fraction of the extent this alignment sits at: 0.0 / 0.5 / 1.0.
    pub fn frac(self) -> f64 {
        match self {
            Align::Min => 0.0,
            Align::Center => 0.5,
            Align::Max => 1.0,
        }
    }

    /// Outward sign relative to the center: -1 at `Min`, 0 at `Center`,
    /// +1 at `Max`. Used to push the popup away from the anchor by the gap.
    pub fn sign(self) -> f64 {
        match self {
            Align::Min => -1.0,
            Align::Center => 0.0,
            Align::Max => 1.0,
        }
    }

    /// Mirror this alignment: `Min` ↔ `Max`, `Center` stays.
    /// Matches egui's `Align::flip` (emath/src/align.rs).
    pub fn flip(self) -> Self {
        match self {
            Align::Min => Align::Max,
            Align::Center => Align::Center,
            Align::Max => Align::Min,
        }
    }
}

/// A 2D anchor point within a rectangle (an `x` and a `y` [`Align`]).
///
/// Mirrors egui's `Align2`. Remember the Y-up convention: `y: Max` is the top
/// edge, `y: Min` the bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Align2 {
    pub x: Align,
    pub y: Align,
}

impl Align2 {
    pub const LEFT_TOP: Self = Self {
        x: Align::Min,
        y: Align::Max,
    };
    pub const CENTER_TOP: Self = Self {
        x: Align::Center,
        y: Align::Max,
    };
    pub const RIGHT_TOP: Self = Self {
        x: Align::Max,
        y: Align::Max,
    };
    pub const LEFT_CENTER: Self = Self {
        x: Align::Min,
        y: Align::Center,
    };
    pub const CENTER: Self = Self {
        x: Align::Center,
        y: Align::Center,
    };
    pub const RIGHT_CENTER: Self = Self {
        x: Align::Max,
        y: Align::Center,
    };
    pub const LEFT_BOTTOM: Self = Self {
        x: Align::Min,
        y: Align::Min,
    };
    pub const CENTER_BOTTOM: Self = Self {
        x: Align::Center,
        y: Align::Min,
    };
    pub const RIGHT_BOTTOM: Self = Self {
        x: Align::Max,
        y: Align::Min,
    };

    /// All nine anchors with the display names egui's Popups demo uses, in
    /// the same order its align combo box lists them (left column, center
    /// column, right column). The array index is stable so UI (e.g. a combo
    /// box) can map a selected index straight back to an `Align2`.
    pub const ALL: [(Self, &'static str); 9] = [
        (Self::LEFT_TOP, "LEFT_TOP"),
        (Self::LEFT_CENTER, "LEFT_CENTER"),
        (Self::LEFT_BOTTOM, "LEFT_BOTTOM"),
        (Self::CENTER_TOP, "CENTER_TOP"),
        (Self::CENTER, "CENTER_CENTER"),
        (Self::CENTER_BOTTOM, "CENTER_BOTTOM"),
        (Self::RIGHT_TOP, "RIGHT_TOP"),
        (Self::RIGHT_CENTER, "RIGHT_CENTER"),
        (Self::RIGHT_BOTTOM, "RIGHT_BOTTOM"),
    ];

    /// Index of this anchor within [`Align2::ALL`], or 0 if somehow absent.
    pub fn all_index(self) -> usize {
        Self::ALL.iter().position(|(a, _)| *a == self).unwrap_or(0)
    }

    /// The absolute point this anchor names inside `rect`.
    pub fn point_in(self, rect: Rect) -> (f64, f64) {
        (
            rect.x + rect.width * self.x.frac(),
            rect.y + rect.height * self.y.frac(),
        )
    }

    /// Mirror on the x-axis (e.g. `LEFT_TOP` → `RIGHT_TOP`).
    pub fn flip_x(self) -> Self {
        Self {
            x: self.x.flip(),
            y: self.y,
        }
    }

    /// Mirror on the y-axis (e.g. `LEFT_TOP` → `LEFT_BOTTOM`).
    pub fn flip_y(self) -> Self {
        Self {
            x: self.x,
            y: self.y.flip(),
        }
    }

    /// Mirror on both axes (e.g. `LEFT_TOP` → `RIGHT_BOTTOM`).
    pub fn flip(self) -> Self {
        Self {
            x: self.x.flip(),
            y: self.y.flip(),
        }
    }
}

/// A parent/child anchor pair describing how a popup attaches to its anchoring
/// widget — the placement half of the Popup API (egui's `RectAlign`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectAlign {
    /// Anchor point on the parent (the widget the popup hangs off).
    pub parent: Align2,
    /// Anchor point on the popup itself.
    pub child: Align2,
}

impl RectAlign {
    /// Below the widget, left edges aligned.
    pub const BOTTOM_START: Self = Self {
        parent: Align2::LEFT_BOTTOM,
        child: Align2::LEFT_TOP,
    };
    /// Below the widget, horizontally centered.
    pub const BOTTOM: Self = Self {
        parent: Align2::CENTER_BOTTOM,
        child: Align2::CENTER_TOP,
    };
    /// Below the widget, right edges aligned.
    pub const BOTTOM_END: Self = Self {
        parent: Align2::RIGHT_BOTTOM,
        child: Align2::RIGHT_TOP,
    };
    /// Above the widget, left edges aligned.
    pub const TOP_START: Self = Self {
        parent: Align2::LEFT_TOP,
        child: Align2::LEFT_BOTTOM,
    };
    /// Above the widget, horizontally centered.
    pub const TOP: Self = Self {
        parent: Align2::CENTER_TOP,
        child: Align2::CENTER_BOTTOM,
    };
    /// Above the widget, right edges aligned.
    pub const TOP_END: Self = Self {
        parent: Align2::RIGHT_TOP,
        child: Align2::RIGHT_BOTTOM,
    };
    /// To the right of the widget, top edges aligned.
    pub const RIGHT_START: Self = Self {
        parent: Align2::RIGHT_TOP,
        child: Align2::LEFT_TOP,
    };
    /// To the right of the widget, vertically centered.
    pub const RIGHT: Self = Self {
        parent: Align2::RIGHT_CENTER,
        child: Align2::LEFT_CENTER,
    };
    /// To the right of the widget, bottom edges aligned.
    pub const RIGHT_END: Self = Self {
        parent: Align2::RIGHT_BOTTOM,
        child: Align2::LEFT_BOTTOM,
    };
    /// To the left of the widget, top edges aligned.
    pub const LEFT_START: Self = Self {
        parent: Align2::LEFT_TOP,
        child: Align2::RIGHT_TOP,
    };
    /// To the left of the widget, vertically centered.
    pub const LEFT: Self = Self {
        parent: Align2::LEFT_CENTER,
        child: Align2::RIGHT_CENTER,
    };
    /// To the left of the widget, bottom edges aligned.
    pub const LEFT_END: Self = Self {
        parent: Align2::LEFT_BOTTOM,
        child: Align2::RIGHT_BOTTOM,
    };

    /// The named presets with the display names and ordering of egui's Popups
    /// demo preset combo box (egui_demo_lib/src/demo/popups.rs).
    pub const PRESETS: [(Self, &'static str); 12] = [
        (Self::TOP_START, "TOP_START"),
        (Self::TOP, "TOP"),
        (Self::TOP_END, "TOP_END"),
        (Self::RIGHT_START, "RIGHT_START"),
        (Self::RIGHT, "RIGHT"),
        (Self::RIGHT_END, "RIGHT_END"),
        (Self::BOTTOM_START, "BOTTOM_START"),
        (Self::BOTTOM, "BOTTOM"),
        (Self::BOTTOM_END, "BOTTOM_END"),
        (Self::LEFT_START, "LEFT_START"),
        (Self::LEFT, "LEFT"),
        (Self::LEFT_END, "LEFT_END"),
    ];

    /// The 12 common menu positions in egui's fallback-preference order
    /// (emath's `RectAlign::MENU_ALIGNS`): corner placements first, centered
    /// ones last, for use with [`RectAlign::find_best_align`].
    pub const MENU_ALIGNS: [Self; 12] = [
        Self::BOTTOM_START,
        Self::BOTTOM_END,
        Self::TOP_START,
        Self::TOP_END,
        Self::RIGHT_END,
        Self::RIGHT_START,
        Self::LEFT_END,
        Self::LEFT_START,
        // These come last on purpose, we prefer the corner ones.
        Self::TOP,
        Self::RIGHT,
        Self::BOTTOM,
        Self::LEFT,
    ];

    /// The preset label matching this alignment exactly, or `None` for an
    /// arbitrary (combo-box-composed) pair that isn't a named preset.
    pub fn preset_label(self) -> Option<&'static str> {
        Self::PRESETS
            .iter()
            .find(|(a, _)| *a == self)
            .map(|(_, label)| *label)
    }

    /// Mirror the placement on the x-axis (e.g. `RIGHT` → `LEFT`).
    pub fn flip_x(self) -> Self {
        Self {
            parent: self.parent.flip_x(),
            child: self.child.flip_x(),
        }
    }

    /// Mirror the placement on the y-axis (e.g. `BOTTOM_START` → `TOP_START`).
    pub fn flip_y(self) -> Self {
        Self {
            parent: self.parent.flip_y(),
            child: self.child.flip_y(),
        }
    }

    /// Mirror the placement on both axes.
    pub fn flip(self) -> Self {
        Self {
            parent: self.parent.flip(),
            child: self.child.flip(),
        }
    }

    /// The 3 alternative placements flipped in various ways, for use with
    /// [`RectAlign::find_best_align`] (mirrors egui's `RectAlign::symmetries`).
    pub fn symmetries(self) -> [Self; 3] {
        [self.flip_x(), self.flip_y(), self.flip()]
    }

    /// Look for the first candidate placement whose popup rect fits entirely
    /// inside `content_rect` — egui's overflow-flip (`find_best_align`).
    ///
    /// If no candidate fits, the FIRST candidate is returned (so the caller's
    /// preferred alignment wins and downstream clamping takes over). Returns
    /// `None` only when the iterator is empty.
    pub fn find_best_align(
        values_to_try: impl Iterator<Item = Self>,
        content_rect: Rect,
        parent_rect: Rect,
        gap: f64,
        expected_size: Size,
    ) -> Option<Self> {
        let mut first_choice = None;
        for align in values_to_try {
            first_choice = first_choice.or(Some(align));
            let suggested = align.place_child(parent_rect, expected_size, gap);
            if contains_rect(content_rect, suggested) {
                return Some(align);
            }
        }
        first_choice
    }

    /// Place a popup of `size` relative to `parent`, applying `gap` pixels of
    /// separation.
    ///
    /// The gap is applied only along axes where the parent and child anchors
    /// *differ* — i.e. the side the popup extends toward — so an edge-attached
    /// popup (e.g. [`RectAlign::BOTTOM_START`]) gets vertical separation without
    /// being nudged sideways.
    pub fn place_child(self, parent: Rect, size: Size, gap: f64) -> Rect {
        let (px, py) = self.parent.point_in(parent);
        let gx = if self.parent.x != self.child.x {
            self.parent.x.sign()
        } else {
            0.0
        };
        let gy = if self.parent.y != self.child.y {
            self.parent.y.sign()
        } else {
            0.0
        };
        let ax = px + gap * gx;
        let ay = py + gap * gy;
        let cx = ax - size.width * self.child.x.frac();
        let cy = ay - size.height * self.child.y.frac();
        Rect::new(cx, cy, size.width, size.height)
    }

    /// [`place_child`](Self::place_child) then clamp the result fully inside
    /// `viewport`, leaving a small margin — so a popup near an edge stays
    /// on-screen, matching how the menu system clamps its panels.
    pub fn place_child_clamped(self, parent: Rect, size: Size, gap: f64, viewport: Size) -> Rect {
        clamp_rect(self.place_child(parent, size, gap), viewport)
    }
}

/// `true` when `inner` lies entirely within `outer` (edges may touch).
fn contains_rect(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

/// Clamp `rect` inside `viewport` with a [`MARGIN`] gutter. If the rect is
/// larger than the viewport it is pinned to the low edge rather than pushed
/// off the high one.
pub fn clamp_rect(rect: Rect, viewport: Size) -> Rect {
    let max_x = (viewport.width - rect.width - MARGIN).max(MARGIN);
    let max_y = (viewport.height - rect.height - MARGIN).max(MARGIN);
    Rect::new(
        rect.x.clamp(MARGIN, max_x),
        rect.y.clamp(MARGIN, max_y),
        rect.width,
        rect.height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARENT: Rect = Rect {
        x: 100.0,
        y: 100.0,
        width: 40.0,
        height: 20.0,
    };
    const SIZE: Size = Size {
        width: 60.0,
        height: 30.0,
    };
    const GAP: f64 = 4.0;

    #[test]
    fn bottom_start_hangs_below_left_aligned() {
        // Y-up: "below" means the popup sits at SMALLER y, its top edge one
        // gap under the parent's bottom edge, left edges flush.
        let r = RectAlign::BOTTOM_START.place_child(PARENT, SIZE, GAP);
        assert_eq!(r, Rect::new(100.0, 66.0, 60.0, 30.0));
        // Left edges aligned.
        assert_eq!(r.x, PARENT.x);
        // Top of popup is exactly `gap` below the parent's bottom.
        assert_eq!(r.y + r.height, PARENT.y - GAP);
    }

    #[test]
    fn top_is_centered_above() {
        let r = RectAlign::TOP.place_child(PARENT, SIZE, GAP);
        // Bottom of popup one gap above parent top.
        assert_eq!(r.y, PARENT.y + PARENT.height + GAP);
        // Horizontal centers coincide.
        let popup_center_x = r.x + r.width * 0.5;
        let parent_center_x = PARENT.x + PARENT.width * 0.5;
        assert!((popup_center_x - parent_center_x).abs() < 1e-9);
    }

    #[test]
    fn right_is_centered_to_the_right() {
        let r = RectAlign::RIGHT.place_child(PARENT, SIZE, GAP);
        // Left edge of popup one gap right of parent's right edge.
        assert_eq!(r.x, PARENT.x + PARENT.width + GAP);
        // Vertical centers coincide.
        let popup_center_y = r.y + r.height * 0.5;
        let parent_center_y = PARENT.y + PARENT.height * 0.5;
        assert!((popup_center_y - parent_center_y).abs() < 1e-9);
    }

    #[test]
    fn left_is_centered_to_the_left() {
        let r = RectAlign::LEFT.place_child(PARENT, SIZE, GAP);
        // Right edge of popup one gap left of parent's left edge.
        assert_eq!(r.x + r.width, PARENT.x - GAP);
        let popup_center_y = r.y + r.height * 0.5;
        let parent_center_y = PARENT.y + PARENT.height * 0.5;
        assert!((popup_center_y - parent_center_y).abs() < 1e-9);
    }

    #[test]
    fn zero_gap_puts_child_anchor_on_parent_anchor() {
        // With no gap the child's anchor point should sit exactly on the
        // parent's anchor point for an arbitrary pair.
        let align = RectAlign {
            parent: Align2::RIGHT_TOP,
            child: Align2::LEFT_BOTTOM,
        };
        let r = align.place_child(PARENT, SIZE, 0.0);
        let (px, py) = Align2::RIGHT_TOP.point_in(PARENT);
        let (cx, cy) = Align2::LEFT_BOTTOM.point_in(r);
        assert!((px - cx).abs() < 1e-9);
        assert!((py - cy).abs() < 1e-9);
    }

    #[test]
    fn clamp_keeps_popup_on_screen() {
        // A bottom-start popup off the left/bottom edge is clamped back in.
        let parent = Rect::new(2.0, 6.0, 40.0, 20.0);
        let viewport = Size::new(400.0, 300.0);
        let r = RectAlign::BOTTOM_START.place_child_clamped(parent, SIZE, GAP, viewport);
        assert!(r.x >= 4.0);
        assert!(r.y >= 4.0);
        assert!(r.x + r.width <= viewport.width);
        assert!(r.y + r.height <= viewport.height);
    }

    #[test]
    fn presets_and_anchor_tables_round_trip() {
        // Every preset is discoverable by label, and every Align2 maps back to
        // its own index — the demo relies on both.
        assert_eq!(RectAlign::BOTTOM.preset_label(), Some("BOTTOM"));
        for (i, (a, _)) in Align2::ALL.iter().enumerate() {
            assert_eq!(a.all_index(), i);
        }
    }

    #[test]
    fn flips_mirror_the_named_presets() {
        // Same pairings egui documents on Align2::flip_x/flip_y/flip.
        assert_eq!(RectAlign::BOTTOM_START.flip_y(), RectAlign::TOP_START);
        assert_eq!(RectAlign::TOP_END.flip_y(), RectAlign::BOTTOM_END);
        assert_eq!(RectAlign::LEFT.flip_x(), RectAlign::RIGHT);
        assert_eq!(RectAlign::RIGHT_START.flip_x(), RectAlign::LEFT_START);
        assert_eq!(RectAlign::BOTTOM_START.flip(), RectAlign::TOP_END);
        // symmetries() = [flip_x, flip_y, flip], in that order.
        assert_eq!(
            RectAlign::BOTTOM_START.symmetries(),
            [
                RectAlign::BOTTOM_END,
                RectAlign::TOP_START,
                RectAlign::TOP_END
            ]
        );
    }

    /// Candidate order egui's `Popup::get_best_align` tries: the configured
    /// align, then its symmetries, then the full MENU_ALIGNS table.
    fn candidates(align: RectAlign) -> impl Iterator<Item = RectAlign> {
        std::iter::once(align)
            .chain(align.symmetries())
            .chain(RectAlign::MENU_ALIGNS)
    }

    #[test]
    fn find_best_align_flips_at_each_screen_edge() {
        let screen = Rect::new(0.0, 0.0, 400.0, 300.0);
        let size = Size::new(100.0, 80.0);

        // Anchor near the BOTTOM edge (Y-up: small y): BOTTOM_START overflows
        // downward, flip_y (TOP_START) survives.
        let parent = Rect::new(150.0, 10.0, 40.0, 20.0);
        let best = RectAlign::find_best_align(
            candidates(RectAlign::BOTTOM_START),
            screen,
            parent,
            GAP,
            size,
        );
        assert_eq!(best, Some(RectAlign::TOP_START));

        // Anchor near the TOP edge: TOP_START overflows upward, flips back down.
        let parent = Rect::new(150.0, 270.0, 40.0, 20.0);
        let best =
            RectAlign::find_best_align(candidates(RectAlign::TOP_START), screen, parent, GAP, size);
        assert_eq!(best, Some(RectAlign::BOTTOM_START));

        // Anchor near the RIGHT edge: RIGHT_START overflows, LEFT_START survives.
        let parent = Rect::new(340.0, 100.0, 40.0, 20.0);
        let best = RectAlign::find_best_align(
            candidates(RectAlign::RIGHT_START),
            screen,
            parent,
            GAP,
            size,
        );
        assert_eq!(best, Some(RectAlign::LEFT_START));

        // Anchor near the LEFT edge: LEFT_START overflows, RIGHT_START survives.
        let parent = Rect::new(10.0, 100.0, 40.0, 20.0);
        let best = RectAlign::find_best_align(
            candidates(RectAlign::LEFT_START),
            screen,
            parent,
            GAP,
            size,
        );
        assert_eq!(best, Some(RectAlign::RIGHT_START));
    }

    #[test]
    fn find_best_align_keeps_preferred_align_when_it_fits() {
        let screen = Rect::new(0.0, 0.0, 400.0, 300.0);
        let parent = Rect::new(150.0, 150.0, 40.0, 20.0);
        let best = RectAlign::find_best_align(
            candidates(RectAlign::BOTTOM_START),
            screen,
            parent,
            GAP,
            Size::new(100.0, 80.0),
        );
        assert_eq!(best, Some(RectAlign::BOTTOM_START));
    }

    #[test]
    fn find_best_align_falls_back_to_first_candidate_when_nothing_fits() {
        // Popup bigger than the screen: nothing fits, first candidate returned
        // (the caller's preferred align), None only for an empty iterator.
        let screen = Rect::new(0.0, 0.0, 100.0, 100.0);
        let parent = Rect::new(40.0, 40.0, 20.0, 10.0);
        let big = Size::new(500.0, 500.0);
        let best = RectAlign::find_best_align(
            candidates(RectAlign::BOTTOM_START),
            screen,
            parent,
            GAP,
            big,
        );
        assert_eq!(best, Some(RectAlign::BOTTOM_START));
        assert_eq!(
            RectAlign::find_best_align(std::iter::empty(), screen, parent, GAP, big),
            None
        );
    }
}
