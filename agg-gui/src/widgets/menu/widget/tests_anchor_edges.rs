//! agg-sharp `PopupMenuConformanceTests.GetYAnchorPutsThePopupEdgeOnTheAnchorEdge`
//! and `GetXAnchorPutsThePopupEdgeOnTheAnchorEdge` (Tests/Agg.Tests/Agg.UI),
//! as behaviour tests on agg-gui's anchor placement.
//!
//! C# pins `PopupMenu.GetYAnchor` / `GetXAnchor`, which turn a pair of
//! `MateOptions` into the offset from the anchor's bottom-left. agg-gui's
//! equivalent is [`RectAlign::place_child`] (`widgets/popup/align.rs`): the
//! anchor edge is `parent`, the popup edge is `child`, and with no gap the
//! popup lands with the named edges on each other. Each `[Arguments]` row of
//! the C# test is one case of the loop, with C#'s values and its 0.001
//! tolerance; edges are Y-up as in agg-sharp (`Align::Max` is the top).

use crate::geometry::{Rect, Size};
use crate::widgets::{Align, Align2, RectAlign};

/// C#'s anchor bounds: bottom-left at the origin, 60 x 20.
const ANCHOR: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 60.0,
    height: 20.0,
};

/// C#'s `new GuiWidget(150, 90)`.
const POPUP: Size = Size {
    width: 150.0,
    height: 90.0,
};

/// The coordinate of `edge` on the axis whose start is `start` and extent
/// `extent` (`Min` is the left / bottom, `Max` the right / top).
fn edge_at(edge: Align, start: f64, extent: f64) -> f64 {
    match edge {
        Align::Max => start + extent,
        _ => start,
    }
}

#[test]
fn get_y_anchor_puts_the_popup_edge_on_the_anchor_edge() {
    // (anchor edge, popup edge): Top/Bottom, Top/Top, Bottom/Top, Bottom/Bottom
    let cases = [
        (Align::Max, Align::Min),
        (Align::Max, Align::Max),
        (Align::Min, Align::Max),
        (Align::Min, Align::Min),
    ];
    for (anchor_edge, popup_edge) in cases {
        let align = RectAlign {
            parent: Align2 {
                x: Align::Min,
                y: anchor_edge,
            },
            child: Align2 {
                x: Align::Min,
                y: popup_edge,
            },
        };
        let popup = align.place_child(ANCHOR, POPUP, 0.0);

        let anchor_y = edge_at(anchor_edge, ANCHOR.y, ANCHOR.height);
        let popup_y = edge_at(popup_edge, popup.y, popup.height);
        assert!(
            (popup_y - anchor_y).abs() <= 0.001,
            "anchor {anchor_edge:?} / popup {popup_edge:?}: {popup_y} vs {anchor_y}"
        );
    }
}

#[test]
fn get_x_anchor_puts_the_popup_edge_on_the_anchor_edge() {
    // (anchor edge, popup edge): Left/Left, Left/Right, Right/Left, Right/Right
    let cases = [
        (Align::Min, Align::Min),
        (Align::Min, Align::Max),
        (Align::Max, Align::Min),
        (Align::Max, Align::Max),
    ];
    for (anchor_edge, popup_edge) in cases {
        let align = RectAlign {
            parent: Align2 {
                x: anchor_edge,
                y: Align::Min,
            },
            child: Align2 {
                x: popup_edge,
                y: Align::Min,
            },
        };
        let popup = align.place_child(ANCHOR, POPUP, 0.0);

        let anchor_x = edge_at(anchor_edge, ANCHOR.x, ANCHOR.width);
        let popup_x = edge_at(popup_edge, popup.x, popup.width);
        assert!(
            (popup_x - anchor_x).abs() <= 0.001,
            "anchor {anchor_edge:?} / popup {popup_edge:?}: {popup_x} vs {anchor_x}"
        );
        assert_eq!(popup.width, POPUP.width, "the popup keeps its size");
    }
}
