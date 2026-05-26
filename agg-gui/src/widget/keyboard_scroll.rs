//! Keyboard-aware focus auto-scroll.
//!
//! When the on-screen software keyboard slides up under a focused text
//! field, the field can end up hidden behind the panel — typing then
//! happens in pixels the user can't see.  This module is the App-side
//! glue that prevents that: on every focus change (and after the
//! keyboard's panel height is known), we walk the focus path, compute
//! the focused widget's global viewport-space rect, and ask each
//! ancestor [`ScrollView`](crate::widgets::ScrollView) on the path to
//! lift its contents upward by however much of the deficit it can
//! absorb.  Nothing scrolls if the field is already visible above the
//! keyboard.
//!
//! ## Why a separate file
//!
//! `widget/app.rs` is the project's largest file by a margin and is
//! already close to the 800-line cap.  Putting the auto-scroll logic
//! here keeps the file small and the algorithm independently
//! testable (the public function takes an `App` by `&mut self` via a
//! method on `App` defined in `app.rs`).
//!
//! ## Algorithm
//!
//! 1. Get the focused widget's path.  If nothing is focused (or the
//!    focused widget doesn't accept text input), do nothing.
//! 2. Walk from root to the focused widget summing each ancestor's
//!    `bounds().origin()` — this yields the focused widget's bottom-
//!    left corner in Y-up viewport coordinates (its bottom edge sits
//!    at the accumulated `y`).
//! 3. Ask the keyboard for the panel height it WILL occupy when
//!    fully open (independent of the current slide-animation
//!    fraction — see
//!    [`target_panel_height`](crate::widgets::on_screen_keyboard::target_panel_height)).
//!    The first focus event after launch happens before the keyboard
//!    has ever painted, but the layout is deterministic so we get a
//!    real number anyway.
//! 4. Required clearance: bottom of the field must sit at least
//!    `panel_height + SAFETY_MARGIN` above `y = 0` in Y-up.
//! 5. If the field is short, scroll upward (positive lift) by the
//!    deficit, walking UP the focus path and calling
//!    `try_scroll_to_lift` on each ancestor.  The innermost
//!    [`ScrollView`](crate::widgets::ScrollView) absorbs as much as
//!    it can; if it caps out (max-scroll reached), the next
//!    `ScrollView` outward picks up the remainder.

use std::cell::RefCell;

use crate::animation::Tween;
use crate::geometry::Rect;
use crate::widget::Widget;

/// Pixels of clearance kept between the focused field's bottom edge
/// and the keyboard panel's top edge.  Small but non-zero so the
/// field's outline isn't kissing the keyboard chrome.
pub(crate) const SAFETY_MARGIN: f64 = 8.0;

/// Slide duration for the global keyboard-driven lift, in seconds.
/// Matches the keyboard's own slide animation so the lift and the
/// keyboard panel travel together — no visible "the field jumps,
/// THEN the keyboard slides" stagger.
const LIFT_DURATION_SECS: f64 = 0.22;

thread_local! {
    /// Active "lift" applied to the entire widget tree (Y-up pixels)
    /// to keep the focused field above the on-screen keyboard panel.
    /// Animated via [`Tween`] so the raise / lower reads as a smooth
    /// slide rather than a snap.  Updated by [`request_lift`] when
    /// focus changes and ticked by `App::paint`.
    static LIFT: RefCell<Tween> = RefCell::new(Tween::new(0.0, LIFT_DURATION_SECS));
}

/// Set the lift target.  No-op when the new target matches the
/// current target (Tween's `set_target` already guards this); when it
/// differs the animation re-anchors at the current interpolated
/// value, so a focus change that walks the target up then back down
/// reverses smoothly.
pub fn request_lift(target: f64) {
    LIFT.with(|c| c.borrow_mut().set_target(target.max(0.0)));
    crate::animation::request_draw();
}

/// Current interpolated lift in Y-up pixels, without advancing the
/// tween.  Mouse handlers call this to translate screen pixels into
/// widget-tree world coordinates; the keyboard panel itself sits in
/// screen space so its hit-tests use the un-lifted position.
pub fn current_lift() -> f64 {
    LIFT.with(|c| c.borrow().value())
}

/// Advance the animation by one frame and return the new lift.  Call
/// exactly once per paint — Tween auto-requests another draw while
/// the animation is in flight.
pub fn tick_lift() -> f64 {
    LIFT.with(|c| c.borrow_mut().tick())
}

/// `true` while the lift tween is still travelling toward its target.
/// `App::wants_draw` honours this so the host event loop keeps
/// pumping frames until the slide completes.
pub fn is_lift_animating() -> bool {
    LIFT.with(|c| c.borrow().is_animating())
}

/// Reset the lift state — for tests that need a clean slate.
#[cfg(test)]
pub fn reset_lift_for_test() {
    LIFT.with(|c| *c.borrow_mut() = Tween::new(0.0, LIFT_DURATION_SECS));
}

/// Where the lift tween is travelling toward (last value passed to
/// `request_lift`).  Useful for asserting that focus loss did in fact
/// retarget the lift back to 0 without waiting on the animation.
#[cfg(test)]
pub fn lift_target_for_test() -> f64 {
    LIFT.with(|c| c.borrow().target())
}

/// Drop a screen-space position into the lifted widget-tree frame
/// (subtract the active keyboard-driven lift).  Cheap no-op when the
/// keyboard isn't lifting anything.  Mouse handlers call this AFTER
/// the on-screen-keyboard panel hit-test so the panel itself still
/// sits in unlifted screen space.
#[inline]
pub fn lift_to_world(screen_pos: crate::geometry::Point) -> crate::geometry::Point {
    let lift = current_lift();
    if lift.abs() < 0.001 {
        screen_pos
    } else {
        crate::geometry::Point::new(screen_pos.x, screen_pos.y - lift)
    }
}

/// Paint the widget tree and its non-keyboard global overlays (combo
/// popups, tooltips, modal layers) with the active keyboard-driven
/// lift translate applied.  The keyboard panel itself paints OUTSIDE
/// this lift (in `App::paint` after this returns) so it stays glued
/// to the viewport bottom regardless of how much the tree shifts up.
pub(crate) fn paint_lifted_tree(
    root: &mut dyn Widget,
    ctx: &mut dyn crate::draw_ctx::DrawCtx,
    viewport: crate::geometry::Size,
    lift: f64,
) {
    use crate::widget::paint::{paint_global_overlays, paint_subtree};
    let lifted = lift.abs() > 0.001;
    if lifted {
        ctx.save();
        ctx.translate(0.0, lift);
    }
    paint_subtree(root, ctx);
    crate::widgets::combo_box::paint_global_combo_popups(ctx);
    crate::widgets::tooltip::paint_global_tooltips(ctx, viewport);
    paint_global_overlays(root, ctx);
    // Modal/global overlays can contain ComboBox widgets. They submit
    // their popups while `paint_global_overlays` runs, so drain once
    // more to draw those popups above the modal body.
    crate::widgets::combo_box::paint_global_combo_popups(ctx);
    if lifted {
        ctx.restore();
    }
}

/// Combined "focus changed" hook called from `App::set_focus`.
/// Pushes the newly-focused widget's text-input affordance into the
/// on-screen keyboard (so it slides up / down) and immediately runs
/// the auto-scroll so the field clears the keyboard panel.  Doing
/// both in one place keeps `app.rs` tiny — the dependency chain
/// (read affordance / set keyboard / lift scroll) is the same every
/// time, so factoring it out also prevents future drift.
pub(crate) fn notify_focus_change(
    new_path: Option<&[usize]>,
    viewport_width: f64,
    root: &mut dyn Widget,
) {
    use crate::widgets::on_screen_keyboard::{set_text_input_focused, KeyboardInputMode};
    // Read the affordance bundle from the focused widget — text input
    // flag, current text (for the sentence-start auto-cap heuristic),
    // and the preferred keyboard mode (Numeric fields open on digits).
    let (accepts, existing_text, mode) = match new_path {
        Some(p) => {
            let w = mutable_widget_at_path(root, p);
            (w.accepts_text_input(), w.text_input_value(), w.text_input_mode())
        }
        None => (false, None, KeyboardInputMode::Text),
    };
    set_text_input_focused(accepts, existing_text.as_deref(), mode);
    if accepts {
        ensure_focused_visible_above_keyboard(new_path, viewport_width, root);
    } else {
        // Focus left a text-input widget — slide the global lift back
        // to zero so the tree settles into its natural layout as the
        // keyboard slides down.
        request_lift(0.0);
    }
}

/// Entry point invoked by `App::ensure_focused_visible_above_keyboard`.
/// Kept here (rather than as a method body in `app.rs`) so the
/// auto-scroll algorithm sits next to its helpers and the parent
/// file stays under the project's 800-line cap.  Takes the App's
/// fields directly rather than `&mut App` so it doesn't need a set
/// of accessor methods on the App side.
pub(crate) fn ensure_focused_visible_above_keyboard(
    focus: Option<&[usize]>,
    viewport_width: f64,
    root: &mut dyn Widget,
) {
    let Some(path) = focus else {
        return;
    };
    let Some(rect) = focused_widget_screen_bounds(&*root, path) else {
        return;
    };
    // Panel height we'll need to clear.  Uses the layout-derived
    // target so the very first focus event (before the keyboard has
    // ever painted) still gets a real number.
    let panel_h = crate::widgets::on_screen_keyboard::target_panel_height(viewport_width);
    if panel_h <= 0.0 {
        return;
    }
    // In Y-up the panel occupies [0, panel_h].  The field's bottom
    // edge is `rect.y` (Rect::y is the lowest Y in Y-up).  We want
    // `rect.y >= panel_h + SAFETY_MARGIN`.  Anything less is the
    // deficit we ask scroll containers to absorb.
    let required = panel_h + SAFETY_MARGIN;
    if rect.y >= required {
        // Field already clears the panel — slide any leftover lift
        // back to zero (e.g. previous focus needed lift; new one
        // doesn't).
        request_lift(0.0);
        return;
    }
    let deficit = required - rect.y;
    // First let any enclosing ScrollView absorb the deficit (free
    // scroll bandwidth is preferable to lifting the whole UI).
    let absorbed_by_scroll = apply_lift_along_path(root, path, deficit);
    let residual = (deficit - absorbed_by_scroll).max(0.0);
    // Whatever the scroll chain couldn't take, the global lift
    // covers — this is the path the demo's non-overflowing window
    // takes.  Animated via `Tween` so the raise / lower glides
    // alongside the keyboard's own slide.
    request_lift(residual);
}

/// Walk from `root` down `path` and accumulate each visited widget's
/// `bounds().origin()` (its position relative to its parent).  Returns
/// the focused widget's screen-space rect in Y-up coordinates.
///
/// Note: this only handles plain bounds-based positioning — widgets
/// that apply additional transforms via `inspector_child_transform`
/// (e.g. the node editor's pan/zoom canvas) aren't accounted for.
/// That's deliberate — text input inside a panned canvas isn't a
/// supported pattern for the software keyboard yet.
pub(crate) fn focused_widget_screen_bounds(
    root: &dyn Widget,
    path: &[usize],
) -> Option<Rect> {
    // Accumulate translation from root frame down to the focused leaf.
    // The root's own bounds.origin() is always (0,0) in practice
    // (App::layout sets it that way) but we include it for symmetry.
    let mut accum_x = root.bounds().x;
    let mut accum_y = root.bounds().y;
    let mut widget: &dyn Widget = root;
    for &idx in path.iter() {
        let children = widget.children();
        if idx >= children.len() {
            return None;
        }
        widget = children[idx].as_ref();
        let b = widget.bounds();
        accum_x += b.x;
        accum_y += b.y;
    }
    let b = widget.bounds();
    // accum_x/y is the focused widget's parent-frame origin already
    // composed with all ancestor offsets — i.e. its viewport-space
    // bottom-left (Y-up).  Size comes straight from the leaf.
    Some(Rect::new(accum_x, accum_y, b.width, b.height))
}

/// Walk UP the focus path (innermost ancestor first), giving each
/// ancestor a chance to absorb `deficit` pixels of lift via
/// `Widget::try_scroll_to_lift`.  Stops as soon as the deficit is
/// fully absorbed.  Returns the total amount applied.
pub(crate) fn apply_lift_along_path(
    root: &mut dyn Widget,
    path: &[usize],
    mut deficit: f64,
) -> f64 {
    if deficit.abs() < 0.5 {
        return 0.0;
    }
    let mut total_applied = 0.0;
    // The innermost ancestor of the focused widget sits at path[..len-1].
    // (path[..len] would be the focused widget itself — text fields
    // and friends never override `try_scroll_to_lift`, so there's
    // nothing to gain by asking them.)
    let n = path.len();
    if n == 0 {
        return 0.0;
    }
    for ancestor_depth in (0..n).rev() {
        let ancestor_path = &path[..ancestor_depth];
        let ancestor = mutable_widget_at_path(root, ancestor_path);
        let applied = ancestor.try_scroll_to_lift(deficit);
        total_applied += applied;
        deficit -= applied;
        if deficit.abs() < 0.5 {
            break;
        }
    }
    total_applied
}

fn mutable_widget_at_path<'a>(root: &'a mut dyn Widget, path: &[usize]) -> &'a mut dyn Widget {
    if path.is_empty() {
        return root;
    }
    let idx = path[0];
    let child = &mut root.children_mut()[idx];
    mutable_widget_at_path(child.as_mut(), &path[1..])
}
