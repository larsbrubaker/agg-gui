//! Widget-hosted popups while the on-screen keyboard lifts the tree.
//!
//! The lift contract (see `keyboard_lift_harness.rs` for the full statement):
//! root logical space is the UNLIFTED layout space — pointer events,
//! `current_mouse_world`, `logical_root_transform(ctx)` and `PopupMenu` root
//! coordinates all live in it — and `App::paint` applies the lift `L` exactly
//! once, so root `(x, y)` shows on screen at `(x, y + L)`. Viewport clamps and
//! flips must keep popups on SCREEN: while lifted the on-screen part of root
//! space is `(0, −L, w, h)`.
//!
//! Covered here, through the production `App` path:
//!
//! * H — a `PopupMenu` hosted the widget-local way `menu/widget/popup_local.rs`
//!   documents (`sync_root_origin` in `paint`, `open_at_local`,
//!   `handle_local_event`, `paint_local` from `paint_global_overlay`, modal +
//!   overlay hit-test while open): hovering a row on screen offers THAT row's
//!   tooltip to the central controller, and a menu with room below its anchor
//!   on screen (but not in root space) hangs below it, fully on screen;
//! * I — the round-swatch `ColorPicker` popup opens below its swatch when
//!   there is room on screen (but not in root space), and the placement chosen
//!   when it opens is the one it paints with, so the first hit tests match the
//!   painted panel.

use super::keyboard_lift_harness::{
    bbox_logical, elapse_tooltip_delay, is_grey, near, paint_frame, sample_logical, screen_phys,
    LiftGuard, Place, ScaleGuard, SystemFontGuard, TooltipGuard,
};
use super::*;

use crate::geometry::{Point, Rect};
use crate::text::Font;
use crate::widgets::tooltip::controller;
use crate::{ColorPicker, DrawCtx, Event, EventResult, MenuEntry, MenuItem, PopupMenu};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// Logical viewport for every test here (tall enough for a menu / colour
/// panel to fit on screen below a lifted anchor).
const VP: Size = Size {
    width: 300.0,
    height: 400.0,
};

const RED: Color = Color::rgba(1.0, 0.0, 0.0, 1.0);

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

// ---------------------------------------------------------------------------
// H. PopupMenu hosted through the widget-local pattern
// ---------------------------------------------------------------------------

/// A dropdown-style owner of a `PopupMenu`, wired exactly as
/// `menu/widget/popup_local.rs` documents.
struct MenuHost {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    menu: Rc<RefCell<PopupMenu>>,
    font: Arc<Font>,
}

impl Widget for MenuHost {
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
        self.menu.borrow_mut().sync_root_origin(ctx);
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        let viewport = crate::widget::current_viewport();
        self.menu.borrow_mut().handle_local_event(event, viewport).0
    }
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let viewport = crate::widget::current_viewport();
        let font = Arc::clone(&self.font);
        self.menu
            .borrow_mut()
            .paint_local(ctx, font, 14.0, viewport);
    }
    fn has_active_modal(&self) -> bool {
        self.menu.borrow().is_open()
    }
    fn hit_test_global_overlay(&self, _local_pos: Point) -> bool {
        self.menu.borrow().is_open()
    }
}

/// Desktop menu metrics (the input profile is process-wide, hence the lock),
/// unit scale, a clean tooltip controller on the virtual clock and a system
/// font for the controller's tip painter. Restores all of it on drop; the
/// profile lock is declared last so it is released last.
struct MenuGuard {
    prev_profile: crate::input_profile::InputProfile,
    _font: SystemFontGuard,
    _tips: TooltipGuard,
    _scales: ScaleGuard,
    _profile: std::sync::MutexGuard<'static, ()>,
}

impl MenuGuard {
    fn new() -> Self {
        let profile = crate::input_profile::profile_test_lock();
        let prev_profile = crate::input_profile::current_input_profile();
        crate::input_profile::set_input_profile(crate::input_profile::InputProfile::Desktop);
        crate::touch_state::clear_last_touch_event_for_testing();
        MenuGuard {
            prev_profile,
            _scales: ScaleGuard::set(1.0, 1.0),
            _tips: TooltipGuard::new(),
            _font: SystemFontGuard::set(font()),
            _profile: profile,
        }
    }
}

impl Drop for MenuGuard {
    fn drop(&mut self) {
        // Still holding the profile lock (fields drop after this).
        crate::input_profile::set_input_profile(self.prev_profile);
    }
}

/// Four rows: two with tips, two without.
fn menu_items() -> Vec<MenuEntry> {
    vec![
        MenuItem::action("Recent A", "open.a")
            .tooltip("tip for A")
            .into(),
        MenuItem::action("Recent B", "open.b")
            .tooltip("tip for B")
            .into(),
        MenuItem::action("Plain", "plain").into(),
        MenuItem::action("Plain 2", "plain.2").into(),
    ]
}

/// An app whose root places a `MenuHost` at root rect `slot`, laid out and
/// painted once (so the host has synced its root origin), then opened with
/// its top-left at the host's local (0, 0) and painted again.
fn open_hosted_menu(slot: Rect) -> (App, Rc<RefCell<PopupMenu>>) {
    let menu = Rc::new(RefCell::new(PopupMenu::new(menu_items())));
    let host = MenuHost {
        bounds: Rect::default(),
        children: Vec::new(),
        menu: Rc::clone(&menu),
        font: font(),
    };
    let mut app = App::new(Box::new(Place::new().at(slot, Box::new(host))));
    app.layout(VP);
    let _ = paint_frame(&mut app, VP, RED);
    menu.borrow_mut().open_at_local(Point::new(0.0, 0.0));
    assert!(menu.borrow().is_open(), "precondition: the menu opened");
    let _ = paint_frame(&mut app, VP, RED);
    (app, menu)
}

/// H1. The host sits at root (40, 200, 100, 24) — on screen at y = 260 under
/// L = 60 — and its menu hangs from the host's bottom-left: on screen from
/// y = 260 down, rows 24 tall, nowhere near an edge. Hovering row 0 at its
/// on-screen centre must show row 0's tip. Today the menu itself works in
/// on-screen coordinates (hover lands on row 0), but the row-tip offer
/// hit-tests the unlifted `current_mouse_world` against those on-screen rows,
/// L = 60 (2.5 rows) too low, so it never matches the hovered row.
#[test]
fn lifted_hosted_menu_row_offers_its_own_tooltip() {
    let _guard = MenuGuard::new();
    let lift = 60.0;
    let _lift = LiftGuard::set(lift);
    let host_root = Rect::new(40.0, 200.0, 100.0, 24.0);
    let (mut app, menu) = open_hosted_menu(host_root);

    // Row 0's centre relative to the menu's anchor (translation-invariant:
    // no clamp engages here in either space), placed at the anchor's ON-SCREEN
    // position: the host's local (0, 0) at root (40, 200) → screen (40, 260).
    let offset = {
        let m = menu.borrow();
        let layouts = m.state.layouts(&m.items, VP);
        let row = layouts[0].rows[0].rect;
        Point::new(
            row.x + row.width * 0.5 - m.state.anchor.x,
            row.y + row.height * 0.5 - m.state.anchor.y,
        )
    };
    let row0 = Point::new(host_root.x + offset.x, host_root.y + lift + offset.y);
    let (px, py) = screen_phys(1.0, VP.height, row0);
    app.on_mouse_move(px, py);
    assert_eq!(
        menu.borrow().state.hover_path,
        Some(vec![0]),
        "precondition: hovering on-screen {row0:?} puts the menu's hover on row 0"
    );

    let _ = paint_frame(&mut app, VP, RED);
    elapse_tooltip_delay();
    let _ = paint_frame(&mut app, VP, RED);

    assert_eq!(
        controller::visible_text().as_deref(),
        Some("tip for A"),
        "under lift {lift}, hovering row 0 of a widget-hosted menu at its on-screen \
         centre {row0:?} must show row 0's tooltip"
    );
}

/// H2 (GUARD). The host sits at root (40, 40, 100, 24) — on screen at
/// y = 190 under L = 150. The 4 × 24 = 96 tall menu fits below its anchor ON
/// SCREEN (190 − 96 = 94 ≥ 4), so it must hang from y = 190 down to 94,
/// x ∈ [40, 264] (224 wide), fully on screen. Measured in root space there is
/// only 40 px below the anchor, which would shove it up over its host.
#[test]
fn lifted_hosted_menu_hangs_below_its_anchor_on_screen() {
    let _guard = MenuGuard::new();
    let lift = 150.0;
    let _lift = LiftGuard::set(lift);
    let (mut app, _menu) = open_hosted_menu(Rect::new(40.0, 40.0, 100.0, 24.0));

    let fb = paint_frame(&mut app, VP, RED);
    let bbox = bbox_logical(&fb, 1.0, is_grey).expect("the menu never painted");
    let (x0, y0, x1, y1) = bbox;
    assert!(
        near(y1, 190.0, 2.0) && near(y0, 94.0, 2.0) && near(x0, 40.0, 2.0) && near(x1, 264.0, 2.0),
        "under lift {lift}, a menu with room below its on-screen anchor (y = 190) must \
         hang below it at x ∈ [40, 264], y ∈ [94, 190]; it painted at x ∈ \
         [{x0:.1}, {x1:.1}], y ∈ [{y0:.1}, {y1:.1}] (bbox {bbox:?})"
    );
}

// ---------------------------------------------------------------------------
// I. ColorPicker round-swatch popup
// ---------------------------------------------------------------------------

/// The 228 × 258 colour panel's geometry for a 24 px swatch at root
/// (20, 100) — on screen at y ∈ [300, 324] under L = 200. On screen there are
/// 300 − 4 = 296 px below (≥ 258), so the panel must open DOWN: local
/// y ∈ [−262, −4], on screen y ∈ [38, 296], x ∈ [20, 248]. In root space there
/// are only 96 px below and 272 above, which would open it UP (local
/// y ∈ [28, 286]).
const SWATCH_ROOT: Rect = Rect {
    x: 20.0,
    y: 100.0,
    width: 24.0,
    height: 24.0,
};
const PICKER_LIFT: f64 = 200.0;
/// A local point inside the downward panel (on screen (130, 170)).
const IN_DOWN_PANEL: Point = Point {
    x: 110.0,
    y: -130.0,
};
/// A local point inside where an upward panel would be (on screen (130, 350)).
const IN_UP_PANEL: Point = Point { x: 110.0, y: 50.0 };

/// Lay out a popup-mode picker at `SWATCH_ROOT`, paint once (publishing the
/// viewport the popup clamps against), and click its on-screen centre to
/// open it. Returns before any paint after the open.
fn open_lifted_picker(lift: &LiftGuard) -> App {
    let cell = Rc::new(Cell::new(Color::rgb(0.0, 0.0, 1.0)));
    let picker = ColorPicker::new(cell, font()).with_round_popup_swatch(24.0);
    let mut app = App::new(Box::new(Place::new().at(SWATCH_ROOT, Box::new(picker))));
    app.layout(VP);
    let _ = paint_frame(&mut app, VP, RED);

    // The swatch centre, root (32, 112), is on screen at (32, 312).
    let centre = Point::new(32.0, 112.0 + PICKER_LIFT);
    let (px, py) = screen_phys(1.0, VP.height, centre);
    app.on_mouse_down(px, py, MouseButton::Left, Modifiers::default());
    app.on_mouse_up(px, py, MouseButton::Left, Modifiers::default());
    // The click focused the picker, which asks the lift to slide back to 0.
    lift.repin();
    app.layout(VP);
    assert!(
        picker_of(&app).is_open(),
        "precondition: clicking the swatch at its on-screen position opens the popup"
    );
    app
}

fn picker_of(app: &App) -> &ColorPicker {
    app.root().children()[0]
        .as_any()
        .and_then(|a| a.downcast_ref::<ColorPicker>())
        .expect("the root's first child is the ColorPicker")
}

/// I (GUARD). Under lift the open popup paints below its swatch on screen and
/// claims the pointer there.
#[test]
fn lifted_color_popup_opens_below_its_swatch_on_screen() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let lift = LiftGuard::set(PICKER_LIFT);
    let mut app = open_lifted_picker(&lift);

    let fb = paint_frame(&mut app, VP, RED);
    // The panel's left padding column, on screen (23, 150).
    let inside = sample_logical(&fb, 1.0, 23.0, 150.0);
    assert!(
        is_grey(inside),
        "under lift {PICKER_LIFT} the colour panel must open below its on-screen \
         swatch (on screen x ∈ [20, 248], y ∈ [38, 296]) and cover (23, 150); \
         the pixel there is {inside:?}"
    );
    let above = sample_logical(&fb, 1.0, 23.0, 350.0);
    assert!(
        is_red(above),
        "under lift {PICKER_LIFT} nothing may paint above the swatch at on-screen \
         (23, 350) — the panel opens down; got {above:?}"
    );
    assert!(
        picker_of(&app).hit_test_global_overlay(IN_DOWN_PANEL),
        "the painted (downward) panel must claim the pointer at local {IN_DOWN_PANEL:?}"
    );
}

/// I. The placement chosen when the popup opens (from the pressing pointer's
/// root position) must be the one it paints with, so hit tests between the
/// open and the first paint match the panel the user then sees. Today the
/// open-time placement is measured in root space (opens UP) and the paint-time
/// one from the lifted paint transform (opens DOWN).
#[test]
fn lifted_color_popup_placement_at_open_matches_paint() {
    let _scales = ScaleGuard::set(1.0, 1.0);
    let lift = LiftGuard::set(PICKER_LIFT);
    let mut app = open_lifted_picker(&lift);

    let at_open = {
        let p = picker_of(&app);
        (
            p.hit_test_global_overlay(IN_DOWN_PANEL),
            p.hit_test_global_overlay(IN_UP_PANEL),
        )
    };
    let _ = paint_frame(&mut app, VP, RED);
    let after_paint = {
        let p = picker_of(&app);
        (
            p.hit_test_global_overlay(IN_DOWN_PANEL),
            p.hit_test_global_overlay(IN_UP_PANEL),
        )
    };
    assert_eq!(
        at_open,
        (true, false),
        "under lift {PICKER_LIFT} the popup has room below its on-screen swatch, so \
         the placement chosen at open must already be DOWN — claims (down-panel \
         point, up-panel point) = (true, false); at open it claimed {at_open:?}, \
         after the first paint {after_paint:?}"
    );
    assert_eq!(
        after_paint, at_open,
        "the first paint must keep the placement chosen at open"
    );
}

// ---------------------------------------------------------------------------
// P. `Popup` controller (widgets/popup) — clamps like the menu system
// ---------------------------------------------------------------------------

/// Viewport for the `Popup` cases.
const POPUP_VP: Size = Size {
    width: 300.0,
    height: 200.0,
};
/// The pinned lift for the `Popup` cases: on-screen part of root space is
/// `(0, −60, 300, 200)`.
const POPUP_LIFT: f64 = 60.0;

/// An 80 × 50 `BOTTOM_START` popup with a 4 px gap, open at `anchor` (root).
fn open_popup(anchor: Rect) -> crate::Popup {
    let mut p = crate::Popup::new();
    p.align = crate::RectAlign::BOTTOM_START;
    p.gap = 4.0;
    p.set_anchor(anchor);
    p.set_size(Size::new(80.0, 50.0));
    p.open();
    p
}

/// P1. The anchor at root (100, 10, 40, 20) is on screen at y ∈ [70, 90]
/// under L = 60. Hanging below it (top 4 px under the anchor: root
/// y ∈ [−44, 6], on screen [16, 66]) fits ON SCREEN, so the popup must keep
/// its configured `BOTTOM_START` — measured against root (0, 0, w, h) it
/// would not fit and flip above the anchor.
#[test]
fn lifted_popup_with_room_below_on_screen_keeps_its_below_alignment() {
    let _lift = LiftGuard::set(POPUP_LIFT);
    let p = open_popup(Rect::new(100.0, 10.0, 40.0, 20.0));

    let align = p.effective_align(POPUP_VP);
    let r = p.rect(POPUP_VP);
    assert_eq!(
        align,
        crate::RectAlign::BOTTOM_START,
        "under lift {POPUP_LIFT} the popup fits below its anchor on screen and must \
         keep BOTTOM_START; it chose {align:?} (rect {r:?})"
    );
    assert_eq!(
        r,
        Rect::new(100.0, -44.0, 80.0, 50.0),
        "the popup must hang 4 px below the anchor in root space (on screen \
         y ∈ [16, 66])"
    );
    assert!(
        p.contains(Point::new(140.0, -20.0), POPUP_VP),
        "hit-testing follows the placed rect"
    );
}

/// P2. An anchor near the root top, (100, 150, 40, 20), is above the screen
/// under L = 60 (the on-screen part of root space ends at y = 140). No
/// placement of the 80 × 50 popup fits there, so the final clamp must keep its
/// root rect inside the on-screen part of root space less the 4 px margin:
/// x ∈ [4, 296], y ∈ [−56, 136]. Clamped against root (0, 0, w, h) it would
/// sit at y ∈ [96, 146] — partly off screen.
#[test]
fn lifted_popup_near_the_root_top_is_clamped_on_screen() {
    let _lift = LiftGuard::set(POPUP_LIFT);
    let p = open_popup(Rect::new(100.0, 150.0, 40.0, 20.0));

    let r = p.rect(POPUP_VP);
    let (bottom, top) = (-POPUP_LIFT + 4.0, POPUP_VP.height - POPUP_LIFT - 4.0);
    assert!(
        r.x >= 4.0
            && r.x + r.width <= POPUP_VP.width - 4.0
            && r.y >= bottom
            && r.y + r.height <= top,
        "under lift {POPUP_LIFT} the popup's root rect must lie within the on-screen \
         part of root space less the margin (x ∈ [4, 296], y ∈ [{bottom}, {top}]); \
         got {r:?} (on screen y ∈ [{}, {}])",
        r.y + POPUP_LIFT,
        r.y + r.height + POPUP_LIFT
    );
}
