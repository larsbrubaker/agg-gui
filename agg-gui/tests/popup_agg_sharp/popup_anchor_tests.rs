//! agg-sharp `PopupAnchorTests` (Tests/Agg.Tests/Agg.UI/PopupAnchorTests.cs),
//! as behaviour tests on agg-gui's [`Popup`] placed against a real [`Button`]
//! laid out in a window-sized [`AbsoluteLayout`].
//!
//! C# mates a popup to a button through `SystemWindow.ShowPopup` with a pair
//! of `MatePoint`s, each an edge pair (`MateOptions(horizontal, vertical)`)
//! plus an `AltMate` tried when the first does not fit. agg-gui's equivalent
//! is a [`RectAlign`]: the anchor's mate is `parent`, the popup's mate is
//! `child` (`Left`/`Bottom` are [`Align::Min`], `Right`/`Top` [`Align::Max`]),
//! with no gap. The window is the [`Popup::rect`] viewport and the button's
//! laid-out rect (root space, the root at the origin) is its anchor, so each
//! case runs the same edge mating through the same layout C# does.
//!
//! Behaviour conversions, both deliberate agg-gui placement rules:
//! - agg-gui has no explicit `AltMate`. A popup that does not fit tries the
//!   mirror images of its own alignment ([`RectAlign::symmetries`], egui's
//!   overflow flip). Every C# vertical alt mate in this class is the mirror
//!   of its mate, so the vertical redirects land where C# puts them. Two
//!   horizontal legs differ: `left_right_redirect_test` case 0 (popup right
//!   edge on the button's left, no room) flips to the button's *right* edge
//!   where C#'s fixed alt puts it left-aligned with the button, and
//!   `right_left_redirect_test` case 1 (left-aligned, no room) flips to
//!   right-aligned where C#'s alt hangs it off the button's left. Both stay
//!   mated to a button edge and fully on screen.
//! - agg-gui clamps popups 4 px inside the window (`MARGIN`); C# clamps at 0.
//!   The `left_right_redirect_test` button sits 2 px from the window's left,
//!   so a left-aligned popup there lands at 4, not 2.
//!
//! C#'s `WindowTest` only clicks a button that shows a popup; its port checks
//! that the opened popup sits at the button with the default mates.

use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::AbsoluteLayout;
use agg_gui::{
    Align, Align2, Button, HAnchor, Insets, Popup, Rect, RectAlign, Size, VAnchor, Widget,
};

static REGULAR: &[u8] = include_bytes!("../../../demo/assets/Nunito_Regular.ttf");

/// agg-gui's popup clamp gutter (`widgets/popup/align.rs` `MARGIN`).
const MARGIN: f64 = 4.0;

/// C#'s `new GuiWidget(180d, 100d)` popup.
const POPUP: Size = Size {
    width: 180.0,
    height: 100.0,
};

/// A C# `MateEdge`.
#[derive(Clone, Copy)]
enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

fn axis(edge: Edge) -> Align {
    match edge {
        Edge::Left | Edge::Bottom => Align::Min,
        Edge::Right | Edge::Top => Align::Max,
    }
}

/// C# `MateOptions(horizontal, vertical)`.
fn mate(h: Edge, v: Edge) -> Align2 {
    Align2 {
        x: axis(h),
        y: axis(v),
    }
}

/// C#'s `BorderDouble(l, b, r, t)`.
fn bd(left: f64, bottom: f64, right: f64, top: f64) -> Insets {
    Insets::from_sides(left, right, top, bottom)
}

/// C#'s `PopupsTestWindow` holding the `buttonA` row: a window of `size`
/// with `padding`, and the button placed by the row's margin and anchors (a
/// fit row of one button is the button). Returns the button's window rect.
fn button_in_window(size: Size, padding: Insets, margin: Insets, h: HAnchor, v: VAnchor) -> Rect {
    let font = Arc::new(Font::from_slice(REGULAR).expect("demo font"));
    let mut button = Button::new("Popup", font);
    {
        let base = button
            .widget_base_mut()
            .expect("button embeds a WidgetBase");
        base.margin = margin;
        base.h_anchor = h;
        base.v_anchor = v;
    }
    let mut window = AbsoluteLayout::new()
        .with_padding(padding)
        .add(Box::new(button));
    let got = window.layout(size);
    window.set_bounds(Rect::new(0.0, 0.0, got.width, got.height));
    window.children()[0].bounds()
}

/// C#'s `ShowPopup` with `anchor_mate` on the button and `popup_mate` on the
/// popup: the placed popup rect in the window.
fn show_popup(button: Rect, window: Size, anchor_mate: Align2, popup_mate: Align2) -> Rect {
    let mut popup = Popup::new();
    popup.set_anchor(button);
    popup.set_size(POPUP);
    popup.gap = 0.0;
    popup.align = RectAlign {
        parent: anchor_mate,
        child: popup_mate,
    };
    popup.open();
    assert!(popup.is_open());
    popup.rect(window)
}

fn left(r: Rect) -> f64 {
    r.x
}
fn right(r: Rect) -> f64 {
    r.x + r.width
}
fn bottom(r: Rect) -> f64 {
    r.y
}
fn top(r: Rect) -> f64 {
    r.y + r.height
}

/// C#'s `AnchorTests` loop: the four horizontal edge pairs it walks
/// (anchor, popup).
const CASES: [(Edge, Edge); 4] = [
    (Edge::Left, Edge::Right),
    (Edge::Left, Edge::Left),
    (Edge::Right, Edge::Right),
    (Edge::Right, Edge::Left),
];

const WINDOW: Size = Size {
    width: 800.0,
    height: 600.0,
};

/// `PopupsTestWindow`'s padding.
fn window_padding() -> Insets {
    bd(120.0, 10.0, 10.0, 10.0)
}

/// The row C#'s `AnchorTests` adds: centred vertically, left, 120 in.
fn centred_button() -> Rect {
    button_in_window(
        WINDOW,
        window_padding(),
        bd(120.0, 0.0, 0.0, 0.0),
        HAnchor::LEFT | HAnchor::FIT,
        VAnchor::CENTER | VAnchor::FIT,
    )
}

/// The row with `VAnchor = v | Fit` (the redirect tests' row adjuster).
fn row_at(v: VAnchor) -> Rect {
    button_in_window(
        WINDOW,
        window_padding(),
        bd(120.0, 0.0, 0.0, 0.0),
        HAnchor::LEFT | HAnchor::FIT,
        v | VAnchor::FIT,
    )
}

/// Run the four horizontal cases with the vertical mates `anchor_v` /
/// `popup_v`, checking each placed popup with `check(case, button, popup)`.
fn anchor_tests(button: Rect, anchor_v: Edge, popup_v: Edge, check: impl Fn(usize, Rect, Rect)) {
    for (i, (anchor_h, popup_h)) in CASES.into_iter().enumerate() {
        let popup = show_popup(
            button,
            WINDOW,
            mate(anchor_h, anchor_v),
            mate(popup_h, popup_v),
        );
        check(i, button, popup);
    }
}

#[test]
fn window_test() {
    let window = Size::new(700.0, 300.0);
    let button = button_in_window(
        window,
        bd(120.0, 180.0, 10.0, 10.0),
        Insets::ZERO,
        HAnchor::LEFT,
        VAnchor::BOTTOM,
    );
    // C#'s default `MateOptions` on both sides: left / bottom to left / bottom.
    let popup = show_popup(
        button,
        window,
        mate(Edge::Left, Edge::Bottom),
        mate(Edge::Left, Edge::Bottom),
    );
    assert_eq!(left(popup), left(button));
    assert_eq!(bottom(popup), bottom(button));
    assert_eq!((popup.width, popup.height), (180.0, 100.0));
}

#[test]
fn top_bottom_popup_test() {
    anchor_tests(centred_button(), Edge::Top, Edge::Bottom, |i, b, p| {
        assert_eq!(bottom(p), top(b), "case {i}");
    });
}

#[test]
fn top_top_popup_test() {
    anchor_tests(centred_button(), Edge::Top, Edge::Top, |i, b, p| {
        assert_eq!(top(p), top(b), "case {i}");
    });
}

#[test]
fn bottom_top_popup_test() {
    anchor_tests(centred_button(), Edge::Bottom, Edge::Top, |i, b, p| {
        assert_eq!(top(p), bottom(b), "case {i}");
    });
}

#[test]
fn bottom_bottom_popup_test() {
    anchor_tests(centred_button(), Edge::Bottom, Edge::Bottom, |i, b, p| {
        assert_eq!(bottom(p), bottom(b), "case {i}");
    });
}

#[test]
fn bottom_top_up_redirect_test() {
    // Hanging below a button on the window's bottom does not fit: it goes
    // above, bottom on the button's top.
    let button = row_at(VAnchor::BOTTOM);
    anchor_tests(button, Edge::Bottom, Edge::Top, |i, b, p| {
        assert_eq!(bottom(p), top(b), "case {i}");
    });
}

#[test]
fn top_top_up_redirect_test() {
    let button = row_at(VAnchor::BOTTOM);
    anchor_tests(button, Edge::Top, Edge::Top, |i, b, p| {
        assert_eq!(bottom(p), bottom(b), "case {i}");
    });
}

#[test]
fn bottom_top_down_redirect_test() {
    // Standing on a button at the window's top does not fit: it hangs below.
    let button = row_at(VAnchor::TOP);
    anchor_tests(button, Edge::Top, Edge::Bottom, |i, b, p| {
        assert_eq!(top(p), bottom(b), "case {i}");
    });
}

#[test]
fn top_top_down_redirect_test() {
    let button = row_at(VAnchor::TOP);
    anchor_tests(button, Edge::Bottom, Edge::Bottom, |i, b, p| {
        assert_eq!(top(p), top(b), "case {i}");
    });
}

#[test]
fn left_right_redirect_test() {
    // No window padding on the left and a 2 px row margin, so a popup
    // reaching left of the button has no room.
    let button = button_in_window(
        WINDOW,
        bd(0.0, 10.0, 10.0, 10.0),
        Insets::all(2.0),
        HAnchor::LEFT | HAnchor::FIT,
        VAnchor::CENTER | VAnchor::FIT,
    );
    assert_eq!(left(button), 2.0);
    anchor_tests(button, Edge::Bottom, Edge::Top, |i, b, p| {
        let expected = match i {
            // Right edge on the button's left has no room: mirrored, the
            // left edge goes on the button's right (C#'s alt: left-aligned).
            0 => right(b),
            // Left-aligned, clamped to agg-gui's 4 px gutter (C#: 2).
            1 => left(b).max(MARGIN),
            // Right-aligned has no room: mirrored to left-aligned, clamped.
            2 => left(b).max(MARGIN),
            _ => right(b),
        };
        assert_eq!(left(p), expected, "case {i}");
        assert_eq!(top(p), bottom(b), "case {i}");
    });
}

#[test]
fn right_left_redirect_test() {
    // The row is right-anchored, so the button ends 10 px from the
    // window's right and a popup reaching right of it has no room.
    let button = button_in_window(
        WINDOW,
        window_padding(),
        bd(120.0, 0.0, 0.0, 0.0),
        HAnchor::RIGHT | HAnchor::FIT,
        VAnchor::CENTER | VAnchor::FIT,
    );
    // The button's origin is rounded to a whole pixel, as C#'s is.
    assert!((right(button) - 790.0).abs() < 1.0, "{button:?}");
    anchor_tests(button, Edge::Bottom, Edge::Bottom, |i, b, p| {
        let expected = match i {
            // Right edge on the button's left fits.
            0 => left(b),
            // Left-aligned has no room: mirrored to right-aligned (C#'s alt
            // hangs it off the button's left).
            1 => right(b),
            // Right-aligned fits.
            2 => right(b),
            // Left edge on the button's right has no room: mirrored, the
            // right edge goes on the button's left.
            _ => left(b),
        };
        assert_eq!(right(p), expected, "case {i}");
        assert_eq!(bottom(p), bottom(b), "case {i}");
    });
}
