//! Guards for a modal `Window` holding the focused field under the
//! on-screen-keyboard lift, driven through real `App::layout` / `App::paint`
//! frames.
//!
//! Two pieces of production code interact here:
//!
//! - `widget/keyboard_scroll.rs` lifts the WHOLE widget tree (and its global
//!   overlays, which is where modal windows paint) upward by a tweened amount
//!   so the focused text field clears the keyboard panel. The lift is a pure
//!   paint-time translate: layout bounds, hit-testing and the lift computation
//!   itself (`relift_after_layout`, every `App::layout`) all live in the
//!   UN-lifted root frame.
//! - `widgets/window/paint.rs::clamp_modal_into_viewport` runs during the modal
//!   window's global-overlay paint. It reads the window origin through
//!   `logical_root_transform` (which takes the lift out), clamps the dialog
//!   against `visible_root_rect`, folds any correction into `window.bounds`,
//!   and caches `Window::world_offset` for snap registration.
//!
//! Pinned: a dialog that fits above the keyboard is lifted fully on screen,
//! the lift settles, the dialog keeps its bounds and the field clears the
//! keyboard (`short_modal_with_keyboard_lift_stays_on_screen`); and a lifted
//! modal registers its UN-lifted rect as its snap target, at unit scale and at
//! device 2 × UX 1.7, where the lift must come out in LOGICAL units.
//!
//! The host viewport is PHYSICAL (`VP × effective_scale`, see [`phys`]) so the
//! logical viewport is always `VP_W × VP_H`, and every assertion is in logical
//! units. Frames run on the virtual clock so the 0.22 s lift tween finishes
//! deterministically.

use super::*;
use crate::geometry::Rect;
use crate::input_profile::{set_input_profile, InputProfile};
use crate::text::Font;
use crate::widget::active_modal_path;
use crate::widget::keyboard_scroll::{self, SAFETY_MARGIN};
use crate::widgets::on_screen_keyboard;
use crate::widgets::window::{ClickAwayAction, Window};
use crate::Stack;
use std::sync::Arc;
use std::time::Duration;

const VP_W: f64 = 400.0;
const VP_H: f64 = 600.0;
const FIELD_ID: u64 = 4242;
/// Dialog placement: left edge 20, bottom edge on the viewport bottom so the
/// field near the dialog's bottom edge sits under the keyboard panel.
const DLG_X: f64 = 20.0;
const DLG_Y: f64 = 0.0;
const DLG_W: f64 = 300.0;
/// Frames driven after focus; 40 × 16 ms = 640 ms ≫ the 220 ms lift tween.
const FRAMES: usize = 40;
const FRAME_DT: Duration = Duration::from_millis(16);
/// Trailing frames that must be perfectly settled.
const SETTLED_FRAMES: usize = 10;
/// Minimum distance from the feasibility boundary each geometry must keep, so
/// the infeasible / feasible split can't be an off-by-a-pixel accident.
const FEASIBILITY_MARGIN: f64 = 20.0;
const TOL: f64 = 0.5;

/// Pins every piece of global / thread-local state these tests depend on and
/// restores it on drop — including on a panic, so a failing assertion can't
/// leak an enabled keyboard, a lift, a non-unit scale or a non-Desktop
/// profile into a sibling.
struct Fixture {
    snap_was_enabled: bool,
    // Field order matters for drop: the clock guard restores real time before
    // the profile lock is released.
    _clock: crate::clock::ClockGuard,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    /// Pin the device (DPR) and UX scales; the logical viewport stays
    /// `VP_W × VP_H` at any scale because the host size is [`phys`].
    fn new(device_scale: f64, ux_scale: f64) -> Self {
        // The keyboard panel height depends on the process-global input
        // profile, which other tests flip in parallel — hold the shared lock
        // for the whole test and pin Desktop explicitly.
        let lock = crate::input_profile::profile_test_lock();
        set_input_profile(InputProfile::Desktop);
        crate::device_scale::set_device_scale(device_scale);
        crate::ux_scale::set_ux_scale(ux_scale);
        // Same clean slate as `on_screen_keyboard.rs::fresh_state`.
        on_screen_keyboard::dismiss();
        on_screen_keyboard::test_hook::reset();
        on_screen_keyboard::events::clear();
        keyboard_scroll::reset_lift_for_test();
        // Snap stays OFF unless a test opts in, so it can't perturb geometry.
        let snap_was_enabled = crate::snap::is_enabled();
        crate::snap::set_enabled(false);
        clear_snap_registry();
        // Virtual clock BEFORE focus so the lift tween's start time is virtual.
        let clock = crate::clock::scoped_virtual(None);
        on_screen_keyboard::set_enabled(true);
        Self {
            snap_was_enabled,
            _clock: clock,
            _lock: lock,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        on_screen_keyboard::set_enabled(false);
        on_screen_keyboard::test_hook::reset();
        keyboard_scroll::reset_lift_for_test();
        clear_snap_registry();
        crate::snap::set_enabled(self.snap_was_enabled);
        crate::device_scale::set_device_scale(1.0);
        crate::ux_scale::set_ux_scale(1.0);
        set_input_profile(InputProfile::Desktop);
    }
}

/// Drop every entry from the thread-local snap registry (public API only).
/// Mirrors the helper in `window_snap_coords.rs`.
fn clear_snap_registry() {
    for (id, _) in crate::snap::targets_snapshot() {
        crate::snap::unregister_target(id);
    }
}

/// Keyboard panel height the lift must clear, from the live keyboard layout.
fn panel_height() -> f64 {
    let h = on_screen_keyboard::target_panel_height(VP_W);
    assert!(
        h > 0.0,
        "precondition: the enabled keyboard must report a real panel height"
    );
    h
}

/// Host (PHYSICAL) viewport size: `App::layout` divides by the effective
/// device × UX scale, so this keeps the logical viewport at `VP_W × VP_H`.
fn phys() -> Size {
    let s = crate::ux_scale::effective_scale();
    Size::new(VP_W * s, VP_H * s)
}

/// Root `Stack` holding one modal dialog at `(DLG_X, DLG_Y, DLG_W, dlg_h)`
/// whose content is a flex spacer above a `TextField`, so the field sits at
/// the dialog's bottom edge.
fn build_app(dlg_h: f64) -> App {
    let font = Arc::new(Font::from_slice(TEST_FONT).unwrap());
    let content = FlexColumn::new()
        .add_flex(Box::new(SizedBox::new()), 1.0)
        .add(Box::new(
            TextField::new(Arc::clone(&font))
                .with_font_size(14.0)
                .with_focus_id(FIELD_ID),
        ));
    let dialog = Window::new("Dlg", Arc::clone(&font), Box::new(content))
        .with_bounds(Rect::new(DLG_X, DLG_Y, DLG_W, dlg_h))
        .with_auto_size(false)
        .with_resizable(false)
        .with_constrain(true)
        .with_gl_backbuffer(false)
        .with_modal(true)
        .with_click_away(ClickAwayAction::None);
    let root = Stack::new()
        .with_hit_children_only(false)
        .add(Box::new(dialog));
    App::new(Box::new(root))
}

/// Un-lifted world rect of the open modal dialog (active-modal-path walk
/// summing child bounds offsets — the frame layout and pointer events use).
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

/// Un-lifted world rect of the focused widget (the same walk the lift uses).
fn focused_field_world(app: &App) -> Rect {
    let path = app.focused_path().expect("a widget must be focused");
    keyboard_scroll::focused_widget_screen_bounds(app.root(), path)
        .expect("focus path must resolve to a widget")
}

fn layout_and_paint(app: &mut App, fb: &mut Framebuffer) {
    app.layout(phys());
    let mut ctx = GfxCtx::new(fb);
    ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
    app.paint(&mut ctx);
}

fn rect_eq(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < TOL
        && (a.y - b.y).abs() < TOL
        && (a.width - b.width).abs() < TOL
        && (a.height - b.height).abs() < TOL
}

/// One frame's observed state, captured after `layout` + `paint`.
#[derive(Clone, Copy)]
struct Frame {
    dialog: Rect,
    target: f64,
    lift: f64,
    field: Rect,
    animating: bool,
}

/// Everything a scenario run measured.
struct Run {
    app: App,
    panel_h: f64,
    dlg_h: f64,
    /// Field bottom's offset above the dialog bottom (`f`).
    field_offset: f64,
    pre_focus_dialog: Rect,
    frames: Vec<Frame>,
}

/// Build the dialog, paint it once unfocused, focus the field, then drive
/// `FRAMES` frames of `advance → layout → paint`, recording each frame.
fn run_scenario(label: &str, dlg_h: f64) -> Run {
    let mut app = build_app(dlg_h);
    let host = phys();
    let mut fb = Framebuffer::new(host.width.round() as u32, host.height.round() as u32);

    layout_and_paint(&mut app, &mut fb);
    let pre_focus_dialog = modal_window_world(&app);
    assert!(
        rect_eq(pre_focus_dialog, Rect::new(DLG_X, DLG_Y, DLG_W, dlg_h)),
        "precondition: the unfocused dialog fits the viewport, so nothing may \
         move it; got {pre_focus_dialog:?}"
    );

    crate::focus::request_focus(FIELD_ID);
    app.layout(phys());
    assert_eq!(
        app.focused_widget_type_name(),
        Some("TextField"),
        "precondition: the dialog's field took focus"
    );
    // Read AFTER focus: the auto-cap layer switch happens on focus.
    let panel_h = panel_height();
    let field0 = focused_field_world(&app);
    let field_offset = field0.y - pre_focus_dialog.y;
    assert!(
        field0.y < panel_h + SAFETY_MARGIN - FEASIBILITY_MARGIN,
        "precondition: the field (bottom y = {}) must start under the keyboard \
         (needs y >= {}) so a positive lift is required",
        field0.y,
        panel_h + SAFETY_MARGIN
    );
    assert!(
        keyboard_scroll::lift_target_for_test() > 0.0,
        "precondition: focusing the covered field must request a positive lift"
    );

    let mut frames = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        crate::clock::advance(FRAME_DT);
        layout_and_paint(&mut app, &mut fb);
        frames.push(Frame {
            dialog: modal_window_world(&app),
            target: keyboard_scroll::lift_target_for_test(),
            lift: keyboard_scroll::current_lift(),
            field: focused_field_world(&app),
            animating: keyboard_scroll::is_lift_animating(),
        });
    }

    let excess = dlg_h + panel_h + SAFETY_MARGIN - field_offset - VP_H;
    eprintln!(
        "[{label}] scale={:.2} (host {:.0}x{:.0}) VP={VP_W}x{VP_H} panel_h={panel_h:.2} SAFETY_MARGIN={SAFETY_MARGIN} \
         H={dlg_h:.2} f={field_offset:.2} excess(H+panel+margin-f-VP_H)={excess:.2} \
         initial dialog={pre_focus_dialog:?}",
        crate::ux_scale::effective_scale(),
        host.width,
        host.height
    );
    eprintln!("[{label}] frame | dialog.y | lift_target | current_lift | field.y | animating");
    for (i, f) in frames.iter().enumerate() {
        eprintln!(
            "[{label}] {i:>5} | {:>8.2} | {:>11.2} | {:>12.2} | {:>7.2} | {}",
            f.dialog.y, f.target, f.lift, f.field.y, f.animating
        );
    }

    Run {
        app,
        panel_h,
        dlg_h,
        field_offset,
        pre_focus_dialog,
        frames,
    }
}

/// `excess > 0` means "field clears the keyboard" and "lifted dialog top stays
/// on-screen" cannot both hold.
fn excess(run: &Run) -> f64 {
    run.dlg_h + run.panel_h + SAFETY_MARGIN - run.field_offset - VP_H
}

/// (a) The last `SETTLED_FRAMES` frames are identical and the tween is idle.
fn check_converged(run: &Run) -> Option<String> {
    let tail = &run.frames[run.frames.len() - SETTLED_FRAMES..];
    for (k, pair) in tail.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if !rect_eq(a.dialog, b.dialog)
            || (a.target - b.target).abs() >= TOL
            || (a.lift - b.lift).abs() >= TOL
        {
            let i = run.frames.len() - SETTLED_FRAMES + k;
            return Some(format!(
                "not converged: frame {i} → {}: dialog.y {:.2} → {:.2}, lift target \
                 {:.2} → {:.2}, current lift {:.2} → {:.2}",
                i + 1,
                a.dialog.y,
                b.dialog.y,
                a.target,
                b.target,
                a.lift,
                b.lift
            ));
        }
    }
    if keyboard_scroll::is_lift_animating() {
        return Some("not converged: the lift tween is still animating".into());
    }
    None
}

/// (b) On screen, the focused field sits above the keyboard panel.
fn check_field_visible(run: &Run) -> Option<String> {
    let last = run.frames[run.frames.len() - 1];
    let on_screen = last.field.y + last.lift;
    let need = run.panel_h + SAFETY_MARGIN;
    (on_screen < need - TOL).then(|| {
        format!(
            "field hidden: on-screen field bottom {on_screen:.2} (world {:.2} + lift \
             {:.2}) is below the keyboard clearance {need:.2}",
            last.field.y, last.lift
        )
    })
}

/// (c) The transient keyboard lift was not persisted into the dialog bounds.
fn check_dialog_unmoved(run: &Run) -> Option<String> {
    let last = run.frames[run.frames.len() - 1];
    (!rect_eq(last.dialog, run.pre_focus_dialog)).then(|| {
        format!(
            "dialog moved: world rect {:?} differs from its pre-focus rect {:?} — the \
             keyboard lift was folded into the dialog's bounds",
            last.dialog, run.pre_focus_dialog
        )
    })
}

/// Print every check's verdict, then fail with ALL violations at once so one
/// run shows the full picture.
fn assert_checks(label: &str, checks: &[(&str, Option<String>)]) {
    let mut failures = Vec::new();
    for (name, result) in checks {
        match result {
            None => eprintln!("[{label}] {name}: ok"),
            Some(msg) => {
                eprintln!("[{label}] {name}: FAIL — {msg}");
                failures.push(format!("{name}: {msg}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "[{label}] {} check(s) failed:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// FEASIBLE geometry guard: a short dialog lifted clear of the keyboard stays
/// fully on-screen, settles, and keeps its bounds.
#[test]
fn short_modal_with_keyboard_lift_stays_on_screen() {
    let _fx = Fixture::new(1.0, 1.0);
    let panel_h = panel_height();
    let dlg_h = feasible_height(panel_h);
    let run = run_scenario("short", dlg_h);
    assert!(
        excess(&run) < -FEASIBILITY_MARGIN,
        "precondition: geometry must be feasible by a clear margin; excess = {:.2}",
        excess(&run)
    );

    let last = run.frames[run.frames.len() - 1];
    let lifted_bottom = last.dialog.y + last.lift;
    let lifted_top = lifted_bottom + last.dialog.height;
    let on_screen = (lifted_bottom < -TOL || lifted_top > VP_H + TOL).then(|| {
        format!(
            "lifted dialog spans y {lifted_bottom:.2}..{lifted_top:.2}, outside the \
             viewport 0..{VP_H}"
        )
    });
    assert_checks(
        "short",
        &[
            ("(a) converged", check_converged(&run)),
            ("(b) field visible", check_field_visible(&run)),
            ("(c) dialog unmoved", check_dialog_unmoved(&run)),
            ("(d) lifted dialog on screen", on_screen),
        ],
    );
}

/// The snap registry is in the UN-lifted world frame (every other window and
/// pointer event uses it), so a lifted modal must register its un-lifted rect.
fn snap_scenario(label: &str, device_scale: f64, ux_scale: f64) {
    let _fx = Fixture::new(device_scale, ux_scale);
    crate::snap::set_enabled(true);
    clear_snap_registry();
    let panel_h = panel_height();
    let dlg_h = feasible_height(panel_h);
    let mut run = run_scenario(label, dlg_h);
    assert!(
        excess(&run) < -FEASIBILITY_MARGIN,
        "precondition: geometry must be feasible by a clear margin; excess = {:.2}",
        excess(&run)
    );
    let lift = keyboard_scroll::current_lift();
    assert!(
        lift > FEASIBILITY_MARGIN,
        "precondition: the dialog must be lifted (lift = {lift:.2})"
    );
    // Judge settling and bounds BEFORE the extra layout below, which may
    // retarget the lift. Reported alongside the snap checks (not as a bare
    // precondition) so a wrong-frame clamp that drags the dialog shows why.
    let converged = check_converged(&run);
    let unmoved = check_dialog_unmoved(&run);

    // Re-register from fresh layout (paint has cached the modal's offset).
    run.app.layout(phys());
    let want = modal_window_world(&run.app);
    let targets = crate::snap::targets_snapshot();
    eprintln!("[{label}] lift={lift:.2} want={want:?} registry={targets:?}");
    // Exactly one entry (the modal), so a stale duplicate can't mask a wrong
    // rect.
    let single = (targets.len() != 1)
        .then(|| format!("the registry must hold only the modal's snap target; got {targets:?}"));
    let unlifted = (!targets.iter().any(|(_, r)| rect_eq(*r, want))).then(|| {
        format!(
            "a lifted modal must register its UN-lifted world rect {want:?} as its \
             snap target (current lift {lift:.2}); registry held {targets:?}"
        )
    });
    assert_checks(
        label,
        &[
            ("(a) converged", converged),
            ("(c) dialog unmoved", unmoved),
            ("(e) one snap target", single),
            ("(f) un-lifted snap target", unlifted),
        ],
    );
}

#[test]
fn lifted_modal_registers_unlifted_snap_target() {
    snap_scenario("snap", 1.0, 1.0);
}

/// Snap registration at HiDPI × mobile UX zoom (effective scale 3.4): the
/// cached `world_offset` must drop the lift in LOGICAL units.
#[test]
fn lifted_modal_registers_unlifted_snap_target_at_hidpi_ux_scale() {
    snap_scenario("snap@dpr2x1.7", 2.0, 1.7);
}

/// A dialog height comfortably on the feasible side: half the space left
/// above the keyboard clearance, but never below the window's minimum.
fn feasible_height(panel_h: f64) -> f64 {
    ((VP_H - panel_h - SAFETY_MARGIN) * 0.5).round().max(120.0)
}
