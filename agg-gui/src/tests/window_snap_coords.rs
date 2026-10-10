//! Snap-coordinate tests for a modal `Window` nested in an OFFSET host slot.
//!
//! Reproduces the bug where a modal dialog nested in an overlay slot snapped
//! against sibling windows in the wrong coordinate space: the drag snap pass and
//! the snap-target registration both fed SLOT-LOCAL bounds into the shared,
//! canvas-absolute snap registry. The fix caches the slot's world offset at
//! modal paint time and folds it into both the snap candidate and the registered
//! target rect (see `widgets/window/snap_glue.rs`, `paint.rs`, `widget_impl.rs`).
//!
//! Companion coverage: the modal paint clamp (`clamp_modal_into_viewport`) must
//! keep the dialog fully on-screen when dragged toward BOTH the top and bottom
//! viewport edges — a guard against a Y-up/Y-down flip in that clamp.
//!
//! The `lifted_*` cases repeat both under the on-screen keyboard's lift `L`
//! (contract in `keyboard_lift_harness.rs`): canvas space — the snap registry,
//! the snap candidate and `Window::world_offset` — is the UNLIFTED root space
//! that layout bounds and pointer events live in, while the paint clamp keeps
//! the dialog on SCREEN, where root `(x, y)` shows at `(x, y + L)`.

use super::keyboard_lift_harness::LiftGuard;
use super::*;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widget::active_modal_path;
use crate::widgets::window::{ClickAwayAction, Window};
use crate::Stack;
use std::sync::Arc;

fn unit_scale() {
    crate::device_scale::set_device_scale(1.0);
    crate::ux_scale::set_ux_scale(1.0);
}

const VP_W: f64 = 420.0;
const VP_H: f64 = 520.0;

/// A host that places its single child subtree at a fixed OFFSET origin within
/// the canvas. Unlike the `Slot` in `color_clickaway.rs` (whose own bounds fill
/// the viewport, so it adds no offset), this host keeps its own `bounds` at a
/// non-zero origin — so the accumulated paint transform down to the nested
/// window is offset from the canvas origin, reproducing the toolbar-overlay slot
/// the real colour dialog nests in.
struct OffsetHost {
    origin: Rect,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for OffsetHost {
    fn type_name(&self) -> &'static str {
        "OffsetHost"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    // Ignore the parent's set_bounds so we keep our offset origin (mirrors how a
    // Window keeps its own with_bounds; the parent Stack would otherwise reset us
    // to a full-viewport (0,0) rect and erase the offset).
    fn set_bounds(&mut self, _b: Rect) {}
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, _available: Size) -> Size {
        self.bounds = self.origin;
        let o = self.origin;
        let child = &mut self.children[0];
        child.layout(Size::new(o.width, o.height));
        Size::new(self.origin.width, self.origin.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// World-space rect of the currently-open modal window, found by walking the
/// active-modal path and summing child-bounds offsets down to it.
fn modal_window_world(app: &App) -> Rect {
    let path = active_modal_path(app.root()).expect("a modal dialog must be open");
    let mut widget: &dyn Widget = app.root();
    let (mut ox, mut oy) = (0.0, 0.0);
    for &idx in &path {
        let child: &dyn Widget = widget.children()[idx].as_ref();
        let b = child.bounds();
        ox += b.x;
        oy += b.y;
        widget = child;
    }
    let b = widget.bounds();
    Rect::new(ox, oy, b.width, b.height)
}

fn drag(app: &mut App, from: Point, to: Point) {
    app.on_mouse_down(
        from.x,
        VP_H - from.y,
        MouseButton::Left,
        Modifiers::default(),
    );
    app.on_mouse_move(to.x, VP_H - to.y);
}

fn end_drag(app: &mut App, at: Point) {
    app.on_mouse_up(at.x, VP_H - at.y, MouseButton::Left, Modifiers::default());
}

fn paint_once(app: &mut App) {
    app.layout(Size::new(VP_W, VP_H));
    let mut fb = Framebuffer::new(VP_W as u32, VP_H as u32);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
    app.paint(&mut ctx);
}

/// Drop every entry from the shared thread-local snap registry so a sibling
/// test's leftover targets can't perturb our snap computation. Uses only the
/// public API (`targets_snapshot` + `unregister_target`).
fn clear_snap_registry() {
    for (id, _) in crate::snap::targets_snapshot() {
        crate::snap::unregister_target(id);
    }
}

fn approx_eq(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

/// Build an app with a top-level target Window at a known canvas rect plus a
/// modal dialog nested in an offset host at `host_origin`, with dialog-local
/// bounds `dlg_local`.
fn build(target: Rect, host_origin: Rect, dlg_local: Rect) -> App {
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());

    let target_content = SizedBox::new()
        .with_width(target.width)
        .with_height(target.height);
    let target_win = Window::new("Target", Arc::clone(&font), Box::new(target_content))
        .with_bounds(target)
        .with_auto_size(false)
        .with_resizable(false)
        .with_gl_backbuffer(false);

    let dlg_content = SizedBox::new()
        .with_width(dlg_local.width)
        .with_height(dlg_local.height);
    let dialog = Window::new("Dlg", Arc::clone(&font), Box::new(dlg_content))
        .with_bounds(dlg_local)
        .with_auto_size(false)
        .with_resizable(false)
        .with_constrain(true)
        .with_gl_backbuffer(false)
        .with_modal(true)
        .with_click_away(ClickAwayAction::None);
    let host = OffsetHost {
        origin: host_origin,
        bounds: Rect::default(),
        children: vec![Box::new(dialog)],
    };

    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(target_win))
        .add(Box::new(host));
    App::new(Box::new(root))
}

#[test]
fn nested_modal_snaps_in_canvas_absolute_space() {
    unit_scale();
    crate::snap::set_enabled(true);
    clear_snap_registry();

    // Target window: canvas rect (200, 200, 120, 100) → left edge at x=200.
    let target = Rect::new(200.0, 200.0, 120.0, 100.0);
    // Dialog nested in a host offset by (30, 40); local bounds (20, 20, 120, 150)
    // → canvas-absolute origin (50, 60).
    let host_origin = Rect::new(30.0, 40.0, 200.0, 200.0);
    let dlg_local = Rect::new(20.0, 20.0, 120.0, 150.0);
    let mut app = build(target, host_origin, dlg_local);

    // Paint once so the dialog caches its slot world offset at modal-paint time.
    paint_once(&mut app);

    let w0 = modal_window_world(&app);
    assert!(
        approx_eq(w0.x, 50.0) && approx_eq(w0.y, 60.0),
        "dialog starts at canvas (50,60), got {w0:?}"
    );

    // Grab the title bar and drag so the dialog's canvas-absolute LEFT edge lands
    // 3 px shy of the target's left edge (x=200) — within DEFAULT_THRESHOLD (8).
    let grab = Point::new(w0.x + w0.width * 0.5, w0.y + w0.height - 8.0);
    // Aim canvas-left = 203 (raw), i.e. move right by (203 - 50) = 153.
    let to = Point::new(grab.x + 153.0, grab.y);
    drag(&mut app, grab, to);

    let w1 = modal_window_world(&app);
    assert!(
        approx_eq(w1.x, 200.0),
        "the nested dialog's canvas-absolute LEFT edge must snap EXACTLY to the \
         target window's left edge (x=200); got {w1:?}. Before the coordinate-space \
         fix the snap runs in slot-local space and never engages."
    );
    end_drag(&mut app, to);
}

#[test]
fn nested_modal_registers_canvas_absolute_target() {
    unit_scale();
    crate::snap::set_enabled(true);
    clear_snap_registry();

    let target = Rect::new(200.0, 200.0, 120.0, 100.0);
    let host_origin = Rect::new(30.0, 40.0, 200.0, 200.0);
    let dlg_local = Rect::new(20.0, 20.0, 120.0, 150.0);
    let mut app = build(target, host_origin, dlg_local);

    // layout → paint (caches the offset) → layout (re-registers with the offset).
    paint_once(&mut app);
    app.layout(Size::new(VP_W, VP_H));

    let want = modal_window_world(&app); // canvas-absolute (50, 60, 120, 150)
    let targets = crate::snap::targets_snapshot();
    let found = targets.iter().any(|(_, r)| {
        approx_eq(r.x, want.x)
            && approx_eq(r.y, want.y)
            && approx_eq(r.width, want.width)
            && approx_eq(r.height, want.height)
    });
    assert!(
        found,
        "the nested dialog must register its CANVAS-ABSOLUTE rect {want:?} as a snap \
         target; registry held {targets:?}. Before the fix it registered its \
         slot-local rect, corrupting snap targets for every other window."
    );
}

#[test]
fn modal_clamp_keeps_dialog_inside_at_both_vertical_edges() {
    // Drag the nested dialog past BOTH the bottom and top viewport edges and
    // assert the paint clamp keeps it fully on-screen. Guards against a
    // Y-up/Y-down flip in `clamp_modal_into_viewport`.
    unit_scale();
    crate::snap::set_enabled(false); // isolate the clamp from any snap pull
    clear_snap_registry();

    let host_origin = Rect::new(30.0, 40.0, 200.0, 200.0);
    let dlg_local = Rect::new(20.0, 20.0, 120.0, 150.0);
    // No competing target window needed for the clamp check.
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let dlg_content = SizedBox::new().with_width(120.0).with_height(150.0);
    let dialog = Window::new("Dlg", Arc::clone(&font), Box::new(dlg_content))
        .with_bounds(dlg_local)
        .with_auto_size(false)
        .with_resizable(false)
        .with_constrain(true)
        .with_gl_backbuffer(false)
        .with_modal(true)
        .with_click_away(ClickAwayAction::None);
    let host = OffsetHost {
        origin: host_origin,
        bounds: Rect::default(),
        children: vec![Box::new(dialog)],
    };
    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(host));
    let mut app = App::new(Box::new(root));
    paint_once(&mut app);

    // ── Drag far BELOW the viewport (world y → negative) ────────────────
    let w = modal_window_world(&app);
    let grab = Point::new(w.x + w.width * 0.5, w.y + w.height - 8.0);
    drag(&mut app, grab, Point::new(grab.x, -600.0));
    end_drag(&mut app, Point::new(grab.x, -600.0));
    paint_once(&mut app);
    let wb = modal_window_world(&app);
    assert!(
        wb.y >= -0.5,
        "dragging past the BOTTOM edge must clamp the dialog fully on-screen \
         (bottom edge y >= 0); got {wb:?}"
    );
    assert!(
        wb.y + wb.height <= VP_H + 0.5,
        "the bottom-clamped dialog must not overshoot the top edge; got {wb:?}"
    );

    // ── Drag far ABOVE the viewport (world y → large) ───────────────────
    let w = modal_window_world(&app);
    let grab = Point::new(w.x + w.width * 0.5, w.y + w.height - 8.0);
    drag(&mut app, grab, Point::new(grab.x, 1400.0));
    end_drag(&mut app, Point::new(grab.x, 1400.0));
    paint_once(&mut app);
    let wt = modal_window_world(&app);
    assert!(
        wt.y + wt.height <= VP_H + 0.5,
        "dragging past the TOP edge must clamp the dialog fully on-screen \
         (top edge y + height <= viewport height); got {wt:?}"
    );
    assert!(
        wt.y >= -0.5,
        "the top-clamped dialog must not overshoot the bottom edge; got {wt:?}"
    );
}

// ---------------------------------------------------------------------------
// Under the on-screen keyboard's lift
// ---------------------------------------------------------------------------

/// The pinned keyboard lift for the `lifted_*` cases.
const LIFT: f64 = 60.0;

/// Press at ROOT point `from` and move to root `to` while the tree is lifted:
/// a root point shows on screen at `y + L`, and physical input is Y-down.
fn drag_lifted(app: &mut App, lift: &LiftGuard, from: Point, to: Point) {
    let l = lift.value();
    app.on_mouse_down(
        from.x,
        VP_H - (from.y + l),
        MouseButton::Left,
        Modifiers::default(),
    );
    // A press that moves focus asks the lift to slide back to 0.
    lift.repin();
    app.on_mouse_move(to.x, VP_H - (to.y + l));
}

fn end_drag_lifted(app: &mut App, lift: &LiftGuard, at: Point) {
    let l = lift.value();
    app.on_mouse_up(
        at.x,
        VP_H - (at.y + l),
        MouseButton::Left,
        Modifiers::default(),
    );
}

/// `nested_modal_snaps_in_canvas_absolute_space` under lift. Snapping runs in
/// unlifted canvas space, so the dialog's LEFT edge snaps onto the target's
/// (x = 200) exactly as without a lift, and it must NOT move vertically: its
/// canvas bottom 60, top 210 and centre 135 are all more than the 8 px
/// threshold from the target's 200 / 300 / 250. A candidate that carries the
/// lift (bottom 120, top 270, centre 195) instead snaps its centre onto the
/// target's bottom edge (200), dragging the dialog 5 px up.
#[test]
fn lifted_nested_modal_snaps_in_unlifted_canvas_space() {
    unit_scale();
    let lift = LiftGuard::set(LIFT);
    crate::snap::set_enabled(true);
    clear_snap_registry();

    let target = Rect::new(200.0, 200.0, 120.0, 100.0);
    let host_origin = Rect::new(30.0, 40.0, 200.0, 200.0);
    let dlg_local = Rect::new(20.0, 20.0, 120.0, 150.0);
    let mut app = build(target, host_origin, dlg_local);
    paint_once(&mut app);

    let w0 = modal_window_world(&app);
    assert!(
        approx_eq(w0.x, 50.0) && approx_eq(w0.y, 60.0),
        "dialog starts at canvas (50,60), got {w0:?}"
    );

    // Same drag as the unlifted case: aim canvas-left = 203 (3 px shy of 200).
    let grab = Point::new(w0.x + w0.width * 0.5, w0.y + w0.height - 8.0);
    let to = Point::new(grab.x + 153.0, grab.y);
    drag_lifted(&mut app, &lift, grab, to);

    let w1 = modal_window_world(&app);
    assert!(
        approx_eq(w1.x, 200.0),
        "under lift {LIFT} the nested dialog's canvas LEFT edge must still snap to the \
         target's left edge (x=200); got {w1:?}"
    );
    assert!(
        approx_eq(w1.y, 60.0),
        "under lift {LIFT} a horizontal drag must not move the dialog vertically: in \
         unlifted canvas space no vertical edge is within the snap threshold, so its \
         canvas y stays 60; got {w1:?} (a snap candidate carrying the lift snaps its \
         centre 195 onto the target's bottom edge 200)"
    );
    end_drag_lifted(&mut app, &lift, to);
}

/// The registered snap target of a nested dialog is its UNLIFTED canvas rect
/// (50, 60, 120, 150) — the same space its peers register in. With
/// `world_offset` cached from the lifted paint origin it registers L higher.
#[test]
fn lifted_nested_modal_registers_unlifted_canvas_target() {
    unit_scale();
    let _lift = LiftGuard::set(LIFT);
    crate::snap::set_enabled(true);
    clear_snap_registry();

    let target = Rect::new(200.0, 200.0, 120.0, 100.0);
    let host_origin = Rect::new(30.0, 40.0, 200.0, 200.0);
    let dlg_local = Rect::new(20.0, 20.0, 120.0, 150.0);
    let mut app = build(target, host_origin, dlg_local);

    // layout → paint (caches the offset) → layout (re-registers with it).
    paint_once(&mut app);
    app.layout(Size::new(VP_W, VP_H));

    let want = modal_window_world(&app); // unlifted canvas (50, 60, 120, 150)
    let targets = crate::snap::targets_snapshot();
    let found = targets.iter().any(|(_, r)| {
        approx_eq(r.x, want.x)
            && approx_eq(r.y, want.y)
            && approx_eq(r.width, want.width)
            && approx_eq(r.height, want.height)
    });
    assert!(
        found,
        "under lift {LIFT} the nested dialog must register its UNLIFTED canvas rect \
         {want:?} as a snap target; registry held {targets:?}"
    );
}

/// A vertical edge snap under lift. Target (250, 300, 120, 80): edges
/// y = 300 / 380, centre 340. Dragging the dialog (canvas (50, 60, 120, 150))
/// up by 237 puts its canvas bottom at 297 — 3 px shy of the target's bottom
/// edge — so it snaps to y = 300. A candidate carrying the lift sits at
/// bottom 357 / top 507 / centre 432, nowhere near the target, and never
/// snaps.
#[test]
fn lifted_nested_modal_snaps_vertically_in_unlifted_canvas_space() {
    unit_scale();
    let lift = LiftGuard::set(LIFT);
    crate::snap::set_enabled(true);
    clear_snap_registry();

    let target = Rect::new(250.0, 300.0, 120.0, 80.0);
    let host_origin = Rect::new(30.0, 40.0, 200.0, 200.0);
    let dlg_local = Rect::new(20.0, 20.0, 120.0, 150.0);
    let mut app = build(target, host_origin, dlg_local);
    paint_once(&mut app);

    let w0 = modal_window_world(&app);
    let grab = Point::new(w0.x + w0.width * 0.5, w0.y + w0.height - 8.0);
    let to = Point::new(grab.x, grab.y + 237.0);
    drag_lifted(&mut app, &lift, grab, to);

    let w1 = modal_window_world(&app);
    assert!(
        approx_eq(w1.y, 300.0) && approx_eq(w1.x, 50.0),
        "under lift {LIFT} the dialog's canvas bottom (dragged to 297) must snap onto \
         the target's bottom edge at canvas y = 300, x unchanged at 50; got {w1:?}"
    );
    end_drag_lifted(&mut app, &lift, to);
}

/// A modal dialog nested in an offset host, without a competing target.
fn build_dialog_only(host_origin: Rect, dlg_local: Rect) -> App {
    let font = Arc::new(crate::text::Font::from_slice(TEST_FONT).unwrap());
    let dlg_content = SizedBox::new()
        .with_width(dlg_local.width)
        .with_height(dlg_local.height);
    let dialog = Window::new("Dlg", Arc::clone(&font), Box::new(dlg_content))
        .with_bounds(dlg_local)
        .with_auto_size(false)
        .with_resizable(false)
        .with_constrain(true)
        .with_gl_backbuffer(false)
        .with_modal(true)
        .with_click_away(ClickAwayAction::None);
    let host = OffsetHost {
        origin: host_origin,
        bounds: Rect::default(),
        children: vec![Box::new(dialog)],
    };
    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(host));
    App::new(Box::new(root))
}

/// GUARD. A dialog at canvas (50, 330, 120, 150) fits the viewport in root
/// space (top 480 ≤ 520) but shows on screen at y ∈ [390, 540] under L = 60 —
/// 20 px past the top. The paint clamp must pull it fully on screen: canvas
/// y = 520 − 150 − 60 = 310 (on screen [370, 520]).
#[test]
fn lifted_modal_clamp_pulls_an_overflowing_dialog_on_screen() {
    unit_scale();
    let _lift = LiftGuard::set(LIFT);
    crate::snap::set_enabled(false);
    clear_snap_registry();

    // Host slot large enough that the window's own first-layout canvas fit
    // leaves the dialog's local bounds alone.
    let mut app = build_dialog_only(
        Rect::new(30.0, 40.0, 390.0, 480.0),
        Rect::new(20.0, 290.0, 120.0, 150.0),
    );
    paint_once(&mut app);

    let w = modal_window_world(&app);
    let (bottom, top) = (w.y + LIFT, w.y + LIFT + w.height);
    assert!(
        approx_eq(w.y, 310.0) && top <= VP_H + 0.5 && bottom >= -0.5,
        "under lift {LIFT} a dialog whose on-screen top overflows must be clamped \
         fully on screen (canvas y = 310, on screen [370, 520]); got canvas {w:?}, \
         on screen y ∈ [{bottom}, {top}]"
    );
}

/// GUARD. A dialog at canvas (50, −40, 120, 150) is below the root-space
/// viewport bottom but shows on screen at y ∈ [20, 170] under L = 60 — fully
/// on screen — so the paint clamp must leave it where it is.
#[test]
fn lifted_modal_clamp_leaves_an_on_screen_dialog_alone() {
    unit_scale();
    let _lift = LiftGuard::set(LIFT);
    crate::snap::set_enabled(false);
    clear_snap_registry();

    let mut app = build_dialog_only(
        Rect::new(30.0, -40.0, 390.0, 480.0),
        Rect::new(20.0, 0.0, 120.0, 150.0),
    );
    paint_once(&mut app);

    let w = modal_window_world(&app);
    assert!(
        approx_eq(w.x, 50.0) && approx_eq(w.y, -40.0),
        "under lift {LIFT} a dialog fully on screen (on screen y ∈ [20, 170]) must not \
         be moved by the clamp: canvas (50, -40); got {w:?}"
    );
}
