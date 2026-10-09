//! Tooltips while the on-screen keyboard lifts the tree.
//!
//! The lift contract (see `keyboard_lift_harness.rs` for the full statement):
//! root logical space is the UNLIFTED layout space, the tooltip queue takes
//! root-space anchors, and `App::paint` drains it under the lift `L` exactly
//! once — so a tip anchored at root `(x, y)` shows on screen relative to
//! `(x, y + L)`. Edge decisions (flip above, clamp into the viewport) are made
//! against the on-screen viewport: with `L = 60` in a 200-tall viewport the
//! on-screen part of root space is y ∈ [−60, 140], not [0, 200].
//!
//! Every test drives the production path — `App` layout, a hover at the
//! widget's ON-SCREEN position, the virtual tooltip clock, paint into a
//! framebuffer that is the screen — and finds the tip panel by its grey body
//! against a red clear:
//!
//! * C — a widget-anchored lightweight tip (`at_widget`) hangs just below its
//!   widget on screen, centred on it;
//! * D — edge placement: a tip whose widget / pointer has room below ON
//!   SCREEN (but not in root space) stays below, and a tip too tall for either
//!   side stays fully inside the on-screen viewport — for widget anchors,
//!   pointer anchors, and the central controller (`WidgetBase` tooltips);
//! * E — an interactive tip decides flip / shift from its on-screen position
//!   (a regression guard: it is consistent on screen today).

use super::keyboard_lift_harness::{
    bbox_logical, elapse_tooltip_delay, is_grey, is_pure_green, near, paint_frame, screen_phys,
    Block, LiftGuard, Place, ScaleGuard, SystemFontGuard, TooltipGuard,
};
use super::*;

use crate::geometry::{Point, Rect};
use crate::layout_props::WidgetBase;
use crate::text::Font;
use crate::{DrawCtx, Event, EventResult, Tooltip};
use std::sync::Arc;

/// Logical viewport for every test here.
const VP: Size = Size {
    width: 300.0,
    height: 200.0,
};
/// The pinned keyboard lift.
const L: f64 = 60.0;
/// One tooltip text line (12 px × 1.45) plus 6 px padding top and bottom.
const ONE_LINE_PANEL_H: f64 = 12.0 * 1.45 + 12.0;
/// `SCREEN_MARGIN` in `widgets/tooltip`.
const MARGIN: f64 = 4.0;

const RED: Color = Color::rgba(1.0, 0.0, 0.0, 1.0);

/// Build an app whose root places `tip` at root rect `slot`, lay it out at
/// `device × ux`, hover on-screen `hover`, let the tooltip delay elapse, paint
/// and return the screen with the scale it was painted at.
fn hover_and_paint(
    tip: Box<dyn Widget>,
    slot: Rect,
    hover: Point,
    device: f64,
    ux: f64,
) -> (Framebuffer, f64) {
    let s = device * ux;
    let mut app = App::new(Box::new(Place::new().at(slot, tip)));
    let phys = Size::new(VP.width * s, VP.height * s);
    app.layout(phys);
    let (px, py) = screen_phys(s, VP.height, hover);
    app.on_mouse_move(px, py);
    elapse_tooltip_delay();
    (paint_frame(&mut app, phys, RED), s)
}

// ---------------------------------------------------------------------------
// C. Widget-anchored lightweight tooltip
// ---------------------------------------------------------------------------

/// A widget at root (40, 40, 60, 20) is on screen at x ∈ [40, 100],
/// y ∈ [100, 120] under L = 60. Its `at_widget` tip anchors at the widget's
/// bottom centre, so on screen the panel's top sits `TOOLTIP_GAP` (4) below
/// y = 100, at 96, centred on x = 70. Today the anchor is taken through the
/// lifted paint transform and then drained under the lift again: the panel
/// top lands at 156, one lift too high.
fn assert_lifted_widget_anchored_tip_below_widget(device: f64, ux: f64) {
    let _scales = ScaleGuard::set(device, ux);
    let _lift = LiftGuard::set(L);
    let _tips = TooltipGuard::new();

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let tip = Tooltip::new(Box::new(Block::empty()), "Tip", font).at_widget();
    let (fb, s) = hover_and_paint(
        Box::new(tip),
        Rect::new(40.0, 40.0, 60.0, 20.0),
        Point::new(70.0, 110.0),
        device,
        ux,
    );

    let bbox = bbox_logical(&fb, s, is_grey).expect("the widget-anchored tip never painted");
    let (x0, _y0, x1, y1) = bbox;
    let centre_x = (x0 + x1) * 0.5;
    assert!(
        near(y1, 96.0, 1.0) && near(centre_x, 70.0, 1.0),
        "at device {device} × UX {ux} under lift {L} the tip panel must hang just \
         below the widget's on-screen bottom (top at y = 96, centred on x = 70); it \
         painted with its top at y = {y1:.1}, centre x = {centre_x:.1} (bbox {bbox:?})"
    );
}

#[test]
fn lifted_widget_anchored_tooltip_paints_below_its_widget_at_scale_1() {
    assert_lifted_widget_anchored_tip_below_widget(1.0, 1.0);
}

#[test]
fn lifted_widget_anchored_tooltip_paints_below_its_widget_at_device_2_ux_scale() {
    assert_lifted_widget_anchored_tip_below_widget(2.0, 1.5);
}

// ---------------------------------------------------------------------------
// D. Edge placement under lift
// ---------------------------------------------------------------------------

/// D1. A widget at root y ∈ [10, 30] is on screen at y ∈ [70, 90] under
/// L = 60: there is room below it on screen (66 − 29.4 ≥ 4), so its tip must
/// hang below — top at 66, centred on x = 70 — and must NOT flip above it.
/// (In root space there is no room below y = 10, which would flip it.)
#[test]
fn lifted_widget_anchored_tip_near_the_bottom_stays_below_on_screen() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);
    let _tips = TooltipGuard::new();

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let tip = Tooltip::new(Box::new(Block::empty()), "Tip", font).at_widget();
    let (fb, s) = hover_and_paint(
        Box::new(tip),
        Rect::new(40.0, 10.0, 60.0, 20.0),
        Point::new(70.0, 80.0),
        1.0,
        1.0,
    );

    let bbox = bbox_logical(&fb, s, is_grey).expect("the widget-anchored tip never painted");
    let (x0, _y0, x1, y1) = bbox;
    let centre_x = (x0 + x1) * 0.5;
    assert!(
        near(y1, 66.0, 1.0) && near(centre_x, 70.0, 1.0),
        "under lift {L} a widget on screen at y ∈ [70, 90] has room below, so its \
         tip must hang below it (top at y = 66, centred on x = 70), not flip above; \
         it painted with its top at y = {y1:.1}, centre x = {centre_x:.1} (bbox {bbox:?})"
    );
}

/// An 8-line tip: 8 × 17.4 + 12 = 151.2 tall — too tall to fit either below
/// or above a widget in the middle of a 200-tall viewport.
const TALL_TIP: &str =
    "Line one\nLine two\nLine three\nLine four\nLine five\nLine six\nLine seven\nLine eight";

/// The tall tip's panel must end up fully inside the on-screen viewport
/// (y ∈ [4, 196], x ∈ [4, 296]) — not off the top by the lift.
fn assert_tall_tip_inside_screen(fb: &Framebuffer, what: &str) {
    let bbox = bbox_logical(fb, 1.0, is_grey).expect("the tall tip never painted");
    let (x0, y0, x1, y1) = bbox;
    let (top_max, right_max) = (VP.height - MARGIN, VP.width - MARGIN);
    assert!(
        y0 >= MARGIN - 1.0 && y1 <= top_max + 1.0 && x0 >= MARGIN - 1.0 && x1 <= right_max + 1.0,
        "under lift {L} the tall {what} tip must sit fully inside the on-screen \
         viewport (y ∈ [{MARGIN}, {top_max}], x ∈ [{MARGIN}, {right_max}]); it painted \
         at x ∈ [{x0:.1}, {x1:.1}], y ∈ [{y0:.1}, {y1:.1}] (bbox {bbox:?}; a top \
         pinned at {} is the framebuffer edge — the panel ran off screen)",
        VP.height
    );
}

/// D2 (widget anchor). The widget is on screen at y ∈ [90, 110]: 151.2 fits
/// neither below (86) nor above (90 left) it, so the clamp must keep the
/// panel inside the on-screen viewport.
#[test]
fn lifted_tall_widget_anchored_tip_stays_inside_the_screen() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);
    let _tips = TooltipGuard::new();

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let tip = Tooltip::new(Box::new(Block::empty()), TALL_TIP, font).at_widget();
    let (fb, _) = hover_and_paint(
        Box::new(tip),
        Rect::new(40.0, 30.0, 60.0, 20.0),
        Point::new(70.0, 100.0),
        1.0,
        1.0,
    );
    assert_tall_tip_inside_screen(&fb, "widget-anchored");
}

/// D2 (pointer anchor). Same widget, the default pointer-anchored tip with
/// the pointer on screen at (70, 100).
#[test]
fn lifted_tall_pointer_anchored_tip_stays_inside_the_screen() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);
    let _tips = TooltipGuard::new();

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let tip = Tooltip::new(Box::new(Block::empty()), TALL_TIP, font);
    let (fb, _) = hover_and_paint(
        Box::new(tip),
        Rect::new(40.0, 30.0, 60.0, 20.0),
        Point::new(70.0, 100.0),
        1.0,
        1.0,
    );
    assert_tall_tip_inside_screen(&fb, "pointer-anchored");
}

/// Expected on-screen panel for a one-line pointer-anchored tip with the
/// pointer on screen at (70, 75): it hangs below-right of the cursor, its top
/// `TOOLTIP_GAP` (4) + `POINTER_TOOLTIP_EXTRA_DROP` (10) below it at y = 61
/// (bottom 61 − 29.4 = 31.6 ≥ 4, so it fits), its left edge at x = 70.
fn assert_pointer_tip_below_cursor(fb: &Framebuffer, what: &str) {
    let bbox = bbox_logical(fb, 1.0, is_grey).expect("the pointer-anchored tip never painted");
    let (x0, y0, _x1, y1) = bbox;
    assert!(
        near(y1, 61.0, 1.0) && near(y0, 61.0 - ONE_LINE_PANEL_H, 1.0) && near(x0, 70.0, 1.0),
        "under lift {L} the {what} tip for a pointer on screen at (70, 75) has room \
         below it on screen, so it must hang below the cursor (top at y = 61, left \
         at x = 70), not flip above; it painted at x0 = {x0:.1}, y ∈ [{y0:.1}, \
         {y1:.1}] (bbox {bbox:?})"
    );
}

/// D3 (lightweight wrapper, pointer anchor). A widget at root (40, 5, 120, 30)
/// is on screen at y ∈ [65, 95]; the pointer hovers it on screen at (70, 75),
/// root (70, 15). In root space a tip below y = 15 would not fit and flips
/// above (painting at on-screen y ∈ [79, 108.4]); on screen it fits below.
#[test]
fn lifted_pointer_anchored_tip_near_the_bottom_stays_below_the_cursor() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);
    let _tips = TooltipGuard::new();

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let tip = Tooltip::new(Box::new(Block::empty()), "Tip", font);
    let (fb, _) = hover_and_paint(
        Box::new(tip),
        Rect::new(40.0, 5.0, 120.0, 30.0),
        Point::new(70.0, 75.0),
        1.0,
        1.0,
    );
    assert_pointer_tip_below_cursor(&fb, "pointer-anchored wrapper");
}

/// A leaf carrying its tip on its `WidgetBase` — the central controller path.
struct Tipped {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
}

impl Widget for Tipped {
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
}

/// D3 (central controller). The same geometry as the wrapper case, but the
/// tip comes from `WidgetBase::with_tooltip` through the App's central
/// controller (anchored at `current_mouse_world`, placed by the same
/// `place_panel`). The controller arms on the first frame and shows on the
/// first frame after the delay.
#[test]
fn lifted_controller_tip_near_the_bottom_stays_below_the_cursor() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);
    let _tips = TooltipGuard::new();
    let _font = SystemFontGuard::set(Arc::new(Font::from_slice(TEST_FONT).unwrap()));

    let tipped = Tipped {
        bounds: Rect::default(),
        children: Vec::new(),
        base: WidgetBase::new().with_tooltip("Tip"),
    };
    let mut app = App::new(Box::new(
        Place::new().at(Rect::new(40.0, 5.0, 120.0, 30.0), Box::new(tipped)),
    ));
    app.layout(VP);
    let (px, py) = screen_phys(1.0, VP.height, Point::new(70.0, 75.0));
    app.on_mouse_move(px, py);
    let _arm = paint_frame(&mut app, VP, RED);
    elapse_tooltip_delay();
    let fb = paint_frame(&mut app, VP, RED);

    assert!(
        crate::widgets::tooltip::controller::is_visible(),
        "precondition: the central controller shows the hovered widget's tip"
    );
    assert_pointer_tip_below_cursor(&fb, "central-controller");
}

// ---------------------------------------------------------------------------
// E. Interactive tooltip (regression guard)
// ---------------------------------------------------------------------------

/// GUARD. An interactive tip's anchor at root (150, 5, 40, 20) is on screen at
/// y ∈ [65, 85] under L = 60. Its 116 × 52 panel (100 × 40 content + 8 / 6 px
/// padding) fits below on screen (65 − 4 − 52 = 9 ≥ 4), so it must stay below
/// — content at y ∈ [15, 55] — where measuring from root y = 5 would flip it
/// above. The right edge 150 + 116 = 266 ≤ 296 needs no shift: content at
/// x ∈ [158, 258].
#[test]
fn lifted_interactive_tooltip_avoids_edges_from_its_on_screen_position() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);
    let _tips = TooltipGuard::new();

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = Block::solid(Size::new(100.0, 40.0), Color::rgba(0.0, 1.0, 0.0, 1.0));
    let tooltip = Tooltip::new(Box::new(Block::empty()), "unused", font)
        .with_interactive_content(Box::new(content));
    let (fb, s) = hover_and_paint(
        Box::new(tooltip),
        Rect::new(150.0, 5.0, 40.0, 20.0),
        Point::new(170.0, 75.0),
        1.0,
        1.0,
    );

    let bbox =
        bbox_logical(&fb, s, is_pure_green).expect("the interactive tip never painted its content");
    let (x0, y0, x1, y1) = bbox;
    assert!(
        near(y0, 15.0, 1.0) && near(y1, 55.0, 1.0),
        "under lift {L} the tip has room below its on-screen anchor and must stay \
         below it (content y ∈ [15, 55]); it painted at y ∈ [{y0:.1}, {y1:.1}] \
         (bbox {bbox:?})"
    );
    assert!(
        near(x0, 158.0, 1.0) && near(x1, 258.0, 1.0),
        "the tip fits on the right and must not shift (content x ∈ [158, 258]); \
         it painted at x ∈ [{x0:.1}, {x1:.1}] (bbox {bbox:?})"
    );
}
