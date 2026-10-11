//! agg-sharp `AnchoredPopupTests` (Tests/Agg.Tests/Agg.UI/AnchoredPopupTests.cs),
//! which was itself written after agg-gui's `popup/align.rs` and
//! `popup/behavior.rs` unit tests, ported back against [`RectAlign`] and
//! [`Popup`] (C#'s `AnchoredPopup`).
//!
//! C# rectangles are `(left, bottom, right, top)`; agg-gui's [`Rect`] is
//! `(x, y, width, height)`, both Y-up, so `RectangleDouble(100, 66, 160, 96)`
//! is `Rect::new(100, 66, 60, 30)`. C#'s `PresetIndex == -1` (no named preset)
//! is agg-gui's `preset_label() == None`.

use agg_gui::{Align2, Point, Popup, PopupCloseBehavior, Rect, RectAlign, Size};

/// The anchor: bottom 100, top 120 (C# `RectangleDouble(100, 100, 140, 120)`).
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

/// C#'s `RectAlign.ViewportMargin`, the gutter `clamp_rect_in` keeps.
const VIEWPORT_MARGIN: f64 = 4.0;

fn popup(anchor: Rect, size: Size, behavior: PopupCloseBehavior) -> Popup {
    let mut popup = Popup::new();
    popup.set_anchor(anchor);
    popup.set_size(size);
    popup.close_behavior = behavior;
    popup.open();
    popup
}

#[test]
fn presets_place_against_the_named_side() {
    // BottomStart hangs below, left aligned.
    let below = RectAlign::BOTTOM_START.place_child(PARENT, SIZE, GAP);
    assert_eq!(below, Rect::new(100.0, 66.0, 60.0, 30.0));

    // Top is centred above.
    let above = RectAlign::TOP.place_child(PARENT, SIZE, GAP);
    assert_eq!(above.y, PARENT.y + PARENT.height + GAP);
    assert_eq!(above.x + above.width * 0.5, PARENT.x + PARENT.width * 0.5);

    // Right and Left are centred beside it.
    let right = RectAlign::RIGHT.place_child(PARENT, SIZE, GAP);
    assert_eq!(right.x, PARENT.x + PARENT.width + GAP);
    assert_eq!(right.y + right.height * 0.5, PARENT.y + PARENT.height * 0.5);
    let left = RectAlign::LEFT.place_child(PARENT, SIZE, GAP);
    assert_eq!(left.x + left.width, PARENT.x - GAP);
}

#[test]
fn flips_mirror_the_named_presets_and_labels_round_trip() {
    assert_eq!(RectAlign::BOTTOM_START.flip_y(), RectAlign::TOP_START);
    assert_eq!(RectAlign::LEFT.flip_x(), RectAlign::RIGHT);
    assert_eq!(RectAlign::BOTTOM_START.flip(), RectAlign::TOP_END);
    assert_eq!(RectAlign::BOTTOM.preset_label(), Some("BOTTOM"));
    let centred = RectAlign {
        parent: Align2::CENTER,
        child: Align2::CENTER,
    };
    assert_eq!(centred.preset_label(), None);
    for (i, (align, _)) in Align2::ALL.iter().enumerate() {
        assert_eq!(align.all_index(), i);
    }
}

#[test]
fn overflow_flips_to_a_mirror_image_then_clamps() {
    // Anchored near the bottom of a 400 x 300 viewport, BottomStart does not
    // fit, so it flips above.
    let mut popup = popup(
        Rect::new(100.0, 10.0, 40.0, 20.0),
        SIZE,
        PopupCloseBehavior::CloseOnClick,
    );
    popup.gap = GAP;
    popup.align = RectAlign::BOTTOM_START;
    let viewport = Size::new(400.0, 300.0);
    assert_eq!(popup.effective_align(viewport), RectAlign::TOP_START);
    assert_eq!(popup.rect(viewport).y, 34.0);

    // Bigger than the viewport nothing fits; it is clamped in from the margin.
    popup.set_size(Size::new(500.0, 30.0));
    assert_eq!(popup.rect(viewport).x, VIEWPORT_MARGIN);
}

#[test]
fn close_behaviors_decide_which_clicks_close() {
    let viewport = Size::new(400.0, 300.0);
    let inside = Point::new(110.0, 80.0);
    let outside = Point::new(300.0, 250.0);

    let mut p = popup(PARENT, SIZE, PopupCloseBehavior::CloseOnClick);
    let click = p.on_mouse_down(inside, viewport);
    assert!(click.inside);
    assert!(click.closed);

    let mut p = popup(PARENT, SIZE, PopupCloseBehavior::CloseOnClickOutside);
    assert!(!p.on_mouse_down(inside, viewport).closed);
    assert!(p.is_open());
    let click = p.on_mouse_down(outside, viewport);
    assert!(click.closed);
    assert!(click.consumed);

    let mut p = popup(PARENT, SIZE, PopupCloseBehavior::IgnoreClicks);
    let click = p.on_mouse_down(outside, viewport);
    assert!(!click.consumed);
    assert!(p.is_open());

    // Escape closes even an IgnoreClicks popup, once.
    assert!(p.on_escape());
    assert!(!p.on_escape());
}
