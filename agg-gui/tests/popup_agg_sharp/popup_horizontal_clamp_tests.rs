//! agg-sharp `PopupHorizontalClampTests`
//! (Tests/Agg.Tests/Agg.UI/PopupHorizontalClampTests.cs), converted to
//! behaviour tests against [`RectAlign::place_child_clamped`].
//!
//! C# opens the popup through `SystemWindow.ShowPopup` with a `MatePoint`
//! whose `AltMate` equals its `Mate`, so the mate flip is a no-op and only the
//! horizontal clamp is under test. agg-gui has no `MatePoint`/`ShowPopup`; its
//! placement is a [`RectAlign`] (anchor edge on the anchor, popup edge on the
//! popup), and the clamp without any flip is `place_child_clamped`, which is
//! what these tests drive. `MateEdge` pairs map to `Align2` x components; the
//! anchor's bottom mates the popup's top, with no gap (C#'s `ShowPopup` adds
//! none horizontally).
//!
//! CONVERTED: agg-gui keeps every popup and menu a 4 px gutter clear of the
//! viewport edge (`MARGIN` in `popup/align.rs` and `menu/geometry.rs`), where
//! agg-sharp's `BestPopupPosition` clamps flush to it. The clamped positions
//! below are therefore 4 px further in than C#'s (C# 400 → 396, 0 → 4); the
//! case that fits keeps its exact mate-aligned position, as in C#.

use agg_gui::{Align, Align2, Rect, RectAlign, Size};

const WINDOW_WIDTH: f64 = 600.0;
const WINDOW_HEIGHT: f64 = 400.0;

/// agg-gui's viewport gutter (`MARGIN` in `popup/align.rs`).
const MARGIN: f64 = 4.0;

/// Places a popup of `popup_width` x 100 against a 50 x 20 anchor at
/// (`anchor_x`, 200): the anchor's `anchor_edge`/bottom mates the popup's
/// `popup_edge`/top, then the result is clamped into the window.
fn show_popup(anchor_x: f64, popup_width: f64, anchor_edge: Align, popup_edge: Align) -> Rect {
    let anchor = Rect::new(anchor_x, 200.0, 50.0, 20.0);
    let align = RectAlign {
        parent: Align2 {
            x: anchor_edge,
            y: Align::Min,
        },
        child: Align2 {
            x: popup_edge,
            y: Align::Max,
        },
    };
    align.place_child_clamped(
        anchor,
        Size::new(popup_width, 100.0),
        0.0,
        Size::new(WINDOW_WIDTH, WINDOW_HEIGHT),
    )
}

/// A popup anchored hard against the right edge is pulled back until its
/// right edge is inside the window's.
#[test]
fn popup_at_the_right_edge_is_clamped_inside_the_window() {
    let popup = show_popup(550.0, 200.0, Align::Min, Align::Min);
    // Without the clamp this is 550, hanging 150 pixels past the right edge
    assert_eq!(popup.x, WINDOW_WIDTH - popup.width - MARGIN);
    assert!(popup.x + popup.width <= WINDOW_WIDTH);
    assert!(popup.x >= 0.0);
}

/// The mirror case: a popup right aligned to an anchor near the left edge
/// would start at a negative X, and is pushed back to the window's left edge.
#[test]
fn popup_at_the_left_edge_is_clamped_inside_the_window() {
    // Right edge of the popup mated to the left edge of the anchor, so the
    // popup extends left
    let popup = show_popup(10.0, 200.0, Align::Min, Align::Max);
    // Without the clamp this is -190, with almost the whole popup off the
    // left of the window
    assert_eq!(popup.x, MARGIN);
    assert!(popup.x + popup.width <= WINDOW_WIDTH);
}

/// The clamp must not become a general alignment rule: a popup that already
/// fits lands exactly where the mate put it, to the pixel.
#[test]
fn a_popup_that_fits_keeps_its_exact_mate_aligned_position() {
    // The anchor is 50 wide at x = 200, well away from either edge, and the
    // popup is 150 wide, so every one of the four mate combinations lands
    // entirely inside the window
    for (anchor_edge, popup_edge, expected_x) in [
        (Align::Min, Align::Min, 200.0),
        (Align::Max, Align::Min, 250.0),
        (Align::Min, Align::Max, 50.0),
        (Align::Max, Align::Max, 100.0),
    ] {
        let popup = show_popup(200.0, 150.0, anchor_edge, popup_edge);
        assert_eq!(popup.x, expected_x, "{anchor_edge:?} / {popup_edge:?}");
    }
}

/// Nothing can show all of a popup wider than the window. It is left aligned
/// rather than right aligned so the left edge - where menu icons and the start
/// of every label live - is the part that survives.
#[test]
fn a_popup_wider_than_the_window_is_left_aligned() {
    let popup = show_popup(100.0, WINDOW_WIDTH + 200.0, Align::Min, Align::Min);
    assert_eq!(popup.x, MARGIN);
}
