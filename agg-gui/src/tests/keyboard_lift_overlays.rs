//! Root-space overlays while the on-screen keyboard lifts the tree.
//!
//! The lift contract (see `keyboard_lift_harness.rs` for the full statement):
//! root logical space is the UNLIFTED layout space — layout bounds, pointer
//! events, `event_root_transform`, `logical_root_transform(ctx)` and every
//! overlay request queue live in it — and `App::paint` applies the lift `L`
//! exactly once, so root `(x, y)` shows on screen at `(x, y + L)`. Edge
//! decisions are made against the on-screen viewport, i.e. root `(0, −L, w, h)`.
//!
//! Covered here, each through the production `App` layout / event / paint
//! path into a framebuffer that is the screen:
//!
//! * the contract itself: a widget's origin through `logical_root_transform`
//!   at paint equals its origin through `event_root_transform` at event time,
//!   and both equal its laid-out root position;
//! * the open `ComboBox` popup paints adjacent to the closed box on screen
//!   and opens in the direction the on-screen room allows;
//! * the `InspectorPanel` hover highlight (fed root-space layout rects) lands
//!   on the hovered widget's on-screen position;
//! * `overlay_insets::for_paint_ctx` measures a reserved bottom strip against
//!   the widget's on-screen rect.
//!
//! Tooltips live in `keyboard_lift_tooltips.rs`; menus and the colour picker
//! popup in `keyboard_lift_popups.rs`; modal-window clamp / snap in
//! `window_snap_coords.rs`.

use super::keyboard_lift_harness::{
    column_extent, is_grey, near, paint_frame, sample_logical, screen_phys, Block, LiftGuard,
    Place, ScaleGuard,
};
use super::*;

use crate::geometry::{Point, Rect};
use crate::layout_props::Insets;
use crate::text::Font;
use crate::widget::InspectorOverlay;
use crate::{DrawCtx, Event, EventResult, InspectorPanel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// Logical viewport for every test here.
const VP: Size = Size {
    width: 300.0,
    height: 200.0,
};
/// The pinned keyboard lift.
const L: f64 = 60.0;

// ---------------------------------------------------------------------------
// A. The contract: paint-time and event-time root transforms agree
// ---------------------------------------------------------------------------

/// Records its local origin in root space through `logical_root_transform`
/// while painting and through `event_root_transform` while handling a press.
struct OriginProbe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    paint_origin: Rc<Cell<Option<Point>>>,
    event_origin: Rc<Cell<Option<Point>>>,
}

impl Widget for OriginProbe {
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
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (mut x, mut y) = (0.0, 0.0);
        crate::widget::logical_root_transform(ctx).transform(&mut x, &mut y);
        self.paint_origin.set(Some(Point::new(x, y)));
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown { .. } = event {
            if let Some(t) = crate::widget::event_root_transform() {
                let (mut x, mut y) = (0.0, 0.0);
                t.transform(&mut x, &mut y);
                self.event_origin.set(Some(Point::new(x, y)));
            }
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

/// A probe laid out at root (40, 50, 60, 30) — on screen at y ∈ [110, 140]
/// under L = 60 — is pressed at its on-screen centre (70, 125) and painted.
/// Both the event-time and the paint-time transform must put its origin at
/// its laid-out root position (40, 50). Today the paint-time one carries the
/// lift: (40, 110).
fn assert_paint_and_event_root_origins_agree(device: f64, ux: f64) {
    let _scales = ScaleGuard::set(device, ux);
    let _lift = LiftGuard::set(L);
    let s = device * ux;

    let paint_origin = Rc::new(Cell::new(None));
    let event_origin = Rc::new(Cell::new(None));
    let probe = OriginProbe {
        bounds: Rect::default(),
        children: Vec::new(),
        paint_origin: Rc::clone(&paint_origin),
        event_origin: Rc::clone(&event_origin),
    };
    let laid_out = Rect::new(40.0, 50.0, 60.0, 30.0);
    let mut app = App::new(Box::new(Place::new().at(laid_out, Box::new(probe))));
    let phys = Size::new(VP.width * s, VP.height * s);
    app.layout(phys);

    let (px, py) = screen_phys(s, VP.height, Point::new(70.0, 125.0));
    app.on_mouse_down(px, py, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(px, py, MouseButton::Left, Modifiers::default());
    let _fb = paint_frame(&mut app, phys, Color::black());

    let want = Point::new(laid_out.x, laid_out.y);
    let ev = event_origin
        .get()
        .expect("the on-screen press must reach the probe");
    let pt = paint_origin.get().expect("the probe must paint");
    let close = |p: Point| near(p.x, want.x, 1e-6) && near(p.y, want.y, 1e-6);
    assert!(
        close(ev),
        "at device {device} × UX {ux} under lift {L}, event_root_transform must map \
         the probe's origin to its laid-out root position {want:?}; got {ev:?}"
    );
    assert!(
        close(pt),
        "at device {device} × UX {ux} under lift {L}, logical_root_transform(ctx) \
         must map the probe's origin to its laid-out (unlifted) root position \
         {want:?} — the same as the event-time transform {ev:?}; got {pt:?}"
    );
}

#[test]
fn lifted_paint_and_event_root_transforms_agree_at_scale_1() {
    assert_paint_and_event_root_origins_agree(1.0, 1.0);
}

#[test]
fn lifted_paint_and_event_root_transforms_agree_at_device_2_ux_scale() {
    assert_paint_and_event_root_origins_agree(2.0, 1.5);
}

// ---------------------------------------------------------------------------
// B. ComboBox popup
// ---------------------------------------------------------------------------

/// A 3-item combo laid out at root (30, 20, 150, 24) — on screen at
/// x ∈ [30, 180], y ∈ [80, 104] under L = 60 — is clicked open at its
/// on-screen position and painted.
///
/// On screen there are 80 − 4 = 76 px below the box for the 3 × 22 = 66 px
/// popup, so it must open DOWN, adjacent to the box: x ∈ [30, 180],
/// y ∈ [14, 80]. (Measured in root space there would be only 20 − 4 = 16 px
/// below, which would wrongly flip it up.) Nothing may paint above the box.
/// Today the request is submitted at the lifted origin and drained under the
/// lift again, so the popup lands one lift too high, at y ∈ [74, 140].
fn assert_lifted_combo_popup_adjacent_on_screen(device: f64, ux: f64) {
    let _scales = ScaleGuard::set(device, ux);
    let lift = LiftGuard::set(L);
    let s = device * ux;

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let combo = ComboBox::new(vec!["Zero", "One", "Two"], 0, font);
    let root = Place::new().at(Rect::new(30.0, 20.0, 150.0, 24.0), Box::new(combo));
    let mut app = App::new(Box::new(root));
    let phys = Size::new(VP.width * s, VP.height * s);
    app.layout(phys);

    // Click the box's on-screen point (50, 92) → root (50, 32).
    let (px, py) = screen_phys(s, VP.height, Point::new(50.0, 92.0));
    app.on_mouse_down(px, py, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(px, py, MouseButton::Left, Modifiers::default());
    // The click focused the combo, which asks the lift to slide back to 0.
    lift.repin();
    assert_eq!(
        app.root().children()[0]
            .properties()
            .into_iter()
            .find(|(k, _)| *k == "open")
            .map(|(_, v)| v),
        Some("true".to_string()),
        "the click at the box's on-screen position must open the combo"
    );

    let fb = paint_frame(&mut app, phys, Color::rgba(1.0, 0.0, 0.0, 1.0));
    // Box + popup together, as painted in the column x = 37: [14, 104] when
    // the popup hangs below the box.
    let painted = column_extent(&fb, s, 37.0, |p| !is_red(p));

    // Just below the box, and just inside the popup's bottom-left.
    for (x, y) in [(100.0, 77.0), (37.0, 20.0)] {
        let p = sample_logical(&fb, s, x, y);
        assert!(
            !is_red(p),
            "at device {device} × UX {ux} under lift {L}, the open popup must hang \
             below the on-screen box (x ∈ [30, 180], y ∈ [14, 80]) and cover \
             on-screen ({x}, {y}); the pixel there is the red clear colour {p:?}. \
             Box + popup painted at y ∈ {painted:?} in column x = 37 (expected \
             [14, 104])"
        );
    }
    // Well above the box: the popup must not be there.
    let above = sample_logical(&fb, s, 100.0, 115.0);
    assert!(
        is_red(above),
        "at device {device} × UX {ux} under lift {L}, nothing may paint above the \
         on-screen box at (100, 115) — the popup opens down (on-screen room \
         below) and sits adjacent to the box; got {above:?}. Box + popup painted \
         at y ∈ {painted:?} in column x = 37 (expected [14, 104])"
    );
}

#[test]
fn lifted_combo_popup_paints_adjacent_to_the_box_at_scale_1() {
    assert_lifted_combo_popup_adjacent_on_screen(1.0, 1.0);
}

#[test]
fn lifted_combo_popup_paints_adjacent_to_the_box_at_device_2_ux_scale() {
    assert_lifted_combo_popup_adjacent_on_screen(2.0, 1.5);
}

/// A 10-item combo with per-item tips laid out at root (30, 76, 100, 24) is on
/// screen at y ∈ [136, 160] under L = 60. On screen there are 136 − 4 = 132 px
/// below it and only 200 − 160 − 4 = 36 px above, so it opens down —
/// unambiguously — showing floor(132 / 22) = 6 rows that fill on-screen
/// y ∈ [4, 136], the last visible row (5) at [4, 26]. Hovering row 5 puts its
/// 28 px tip beside the popup (x from 30 + 100 + 6 = 136) centred on the row
/// — 3 px past the screen bottom — so the clamp must hold it at on-screen
/// y ∈ [4, 32]: fully on screen and still beside the hovered row. A clamp
/// against root (0, 0, w, h) instead lifts it to on-screen [64, 92], far
/// from the row.
#[test]
fn lifted_combo_item_tooltip_stays_on_screen_beside_the_hovered_row() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let lift = LiftGuard::set(L);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let items = vec![
        "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    let tips: Vec<String> = items.iter().map(|i| format!("Tip {i}")).collect();
    let combo = ComboBox::new(items, 0, font).with_item_tooltips(tips);
    let root = Place::new().at(Rect::new(30.0, 76.0, 100.0, 24.0), Box::new(combo));
    let mut app = App::new(Box::new(root));
    app.layout(VP);

    // Open with a click on the box (on screen (50, 148) → root (50, 88)).
    let (px, py) = screen_phys(1.0, VP.height, Point::new(50.0, 148.0));
    app.on_mouse_down(px, py, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(px, py, MouseButton::Left, Modifiers::default());
    lift.repin();
    let _ = paint_frame(&mut app, VP, Color::rgba(1.0, 0.0, 0.0, 1.0));
    let prop = |k: &str| {
        app.root().children()[0]
            .properties()
            .into_iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| v)
    };
    assert_eq!(
        (
            prop("open"),
            prop("popup_opens_up"),
            prop("popup_visible_count")
        ),
        (
            Some("true".to_string()),
            Some("false".to_string()),
            Some("6".to_string())
        ),
        "precondition: the combo is open and its popup opens down showing 6 rows \
         (on screen y ∈ [4, 136])"
    );

    // Hover the last visible row (5) at on-screen (60, 15).
    let (hx, hy) = screen_phys(1.0, VP.height, Point::new(60.0, 15.0));
    app.on_mouse_move(hx, hy);
    let fb = paint_frame(&mut app, VP, Color::rgba(1.0, 0.0, 0.0, 1.0));

    // The tip's grey body in a column right of the popup (x ∈ [30, 130]).
    let tip = column_extent(&fb, 1.0, 140.0, is_grey)
        .expect("the hovered row's tip must paint beside the popup (column x = 140)");
    let (y0, y1) = tip;
    let centre = (y0 + y1) * 0.5;
    assert!(
        y0 >= 4.0 - 1.0 && y1 <= VP.height - 4.0 + 1.0,
        "under lift {L} the item tip must stay fully on screen (y ∈ [4, 196]); it \
         painted at y ∈ [{y0:.1}, {y1:.1}] (expected [4, 32])"
    );
    assert!(
        (4.0..=26.0).contains(&centre),
        "under lift {L} the item tip must sit beside the hovered row (on screen \
         y ∈ [4, 26]); it painted at y ∈ [{y0:.1}, {y1:.1}] (expected [4, 32])"
    );
}

// ---------------------------------------------------------------------------
// F. InspectorPanel hover highlight
// ---------------------------------------------------------------------------

/// The inspector's hovered bounds are the nodes' `screen_bounds` — root
/// (unlifted) layout rects. A target at root (20, 30, 60, 40) shows on screen
/// at (20, 90, 60, 40) under L = 60, so the highlight must cover its on-screen
/// centre (50, 110) and leave its root-space position (50, 50) — where nothing
/// is on screen — dark. Today the panel subtracts its lifted origin and
/// paints under the lift, so the highlight lands at the unlifted rect, L too
/// low.
#[test]
fn lifted_inspector_hover_highlight_lands_on_the_widget_on_screen() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);

    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let hovered = Rc::new(RefCell::new(None));
    let panel = InspectorPanel::new(font, Rc::new(RefCell::new(Vec::new())), Rc::clone(&hovered));
    let target_rect = Rect::new(20.0, 30.0, 60.0, 40.0);
    let root = Place::new()
        .at(target_rect, Box::new(Block::empty()))
        .at(Rect::new(160.0, 10.0, 130.0, 180.0), Box::new(panel));
    let mut app = App::new(Box::new(root));
    app.layout(VP);

    let target = app
        .collect_inspector_nodes()
        .into_iter()
        .find(|n| n.type_name == "Block")
        .expect("the target is in the inspector snapshot");
    assert_eq!(
        target.screen_bounds, target_rect,
        "inspector screen_bounds are root (unlifted) layout rects"
    );
    *hovered.borrow_mut() = Some(InspectorOverlay {
        bounds: target.screen_bounds,
        margin: Insets::ZERO,
        padding: Insets::ZERO,
    });

    // Black backdrop: the highlight is a faint blue wash.
    let fb = paint_frame(&mut app, VP, Color::rgba(0.0, 0.0, 0.0, 1.0));
    let is_blue = |p: [u8; 4]| p[2] > 40 && p[2] > p[0];
    // Where the highlight actually painted, in the column through the target.
    let painted = column_extent(&fb, 1.0, 50.0, is_blue);

    let on = sample_logical(&fb, 1.0, 50.0, 110.0);
    assert!(
        is_blue(on),
        "under lift {L} the blue hover highlight must cover the hovered widget's \
         on-screen centre (50, 110) (root (50, 50) + lift); the pixel there is \
         {on:?}. The highlight painted at y ∈ {painted:?} in column x = 50 \
         (expected the widget's on-screen [90, 130])"
    );
    let unlifted = sample_logical(&fb, 1.0, 50.0, 50.0);
    assert!(
        is_dark(unlifted),
        "under lift {L} the highlight must not paint at the widget's unlifted root \
         position, on-screen (50, 50) — nothing is on screen there; got {unlifted:?}. \
         The highlight painted at y ∈ {painted:?} in column x = 50"
    );
}

// ---------------------------------------------------------------------------
// J. overlay_insets::for_paint_ctx
// ---------------------------------------------------------------------------

/// Reserves a bottom strip during layout (as the on-screen keyboard or a
/// `ReserveInset` tray would) and records its paint-time local insets.
struct InsetProbe {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    strip: f64,
    seen: Rc<Cell<Option<Insets>>>,
}

impl Widget for InsetProbe {
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
        crate::overlay_insets::reserve(Insets {
            bottom: self.strip,
            ..Insets::default()
        });
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let size = Size::new(self.bounds.width, self.bounds.height);
        self.seen
            .set(Some(crate::overlay_insets::for_paint_ctx(ctx, size)));
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Clears this frame's reserved strips on drop, so the probe's reservation
/// can't leak into a later test on a reused thread.
struct OverlayInsetsGuard;

impl Drop for OverlayInsetsGuard {
    fn drop(&mut self) {
        crate::overlay_insets::begin_frame();
    }
}

/// GUARD. A widget at root (20, 30, 100, 50) is on screen at y ∈ [90, 140]
/// under L = 60. A 120-tall bottom strip — screen-glued, like the keyboard
/// panel — intrudes 120 − 90 = 30 into it on screen (measured in root space it
/// would be 120 − 30 = 90). Its top (140) is below the viewport top (200), so
/// no top inset.
#[test]
fn lifted_overlay_insets_measure_the_widget_on_screen() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let _lift = LiftGuard::set(L);
    let _insets = OverlayInsetsGuard;

    let seen = Rc::new(Cell::new(None));
    let probe = InsetProbe {
        bounds: Rect::default(),
        children: Vec::new(),
        strip: 120.0,
        seen: Rc::clone(&seen),
    };
    let mut app = App::new(Box::new(
        Place::new().at(Rect::new(20.0, 30.0, 100.0, 50.0), Box::new(probe)),
    ));
    app.layout(VP);
    let _fb = paint_frame(&mut app, VP, Color::black());

    let ins = seen.get().expect("the probe must paint");
    assert!(
        near(ins.bottom, 30.0, 1e-6)
            && near(ins.top, 0.0, 1e-6)
            && near(ins.left, 0.0, 1e-6)
            && near(ins.right, 0.0, 1e-6),
        "under lift {L} a 120-tall bottom strip must intrude 30 into the widget's \
         on-screen rect (y ∈ [90, 140]); got local insets {ins:?}"
    );
}
