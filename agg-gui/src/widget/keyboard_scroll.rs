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
            (
                w.accepts_text_input(),
                w.text_input_value(),
                w.text_input_mode(),
            )
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

/// Walk from `root` down `path` and compose each visited widget's
/// `bounds().origin()` offset **and** its optional
/// [`child_transform`](crate::widget::Widget::child_transform) into a single
/// affine, then return the focused widget's screen-space rect in Y-up
/// coordinates.
///
/// Honouring `child_transform` matters because a focusable widget can live
/// inside a pan/zoom container (a [`Scene`](crate::widgets::Scene)) — the demo
/// hosts a `TextField` inside a zoomed Scene — so the naive bounds-only walk
/// would report the field at its un-scaled scene position and lift it by the
/// wrong amount.  The composition mirrors the inspector's
/// (`collect_inspector_nodes`): descending from a widget applies
/// `translate(widget.bounds) · widget.child_transform`.
pub(crate) fn focused_widget_screen_bounds(root: &dyn Widget, path: &[usize]) -> Option<Rect> {
    // `to_screen` maps the CURRENT widget's parent-local coords → screen,
    // starting at identity (the root's parent frame — the inspector walk seeds
    // its `parent_to_screen` the same way with `translate(0, 0)`).  Each loop
    // iteration folds in the current widget's own bounds offset, so the root's
    // position is accounted for by the first iteration, not a separate seed.
    let mut to_screen = crate::TransAffine::new();
    let mut widget: &dyn Widget = root;
    for &idx in path.iter() {
        // Descend from `widget` into child `idx`: shift into the widget's own
        // frame, then apply its child transform (identity for most widgets).
        let b = widget.bounds();
        to_screen.translate(b.x, b.y);
        if let Some(ct) = widget.child_transform() {
            to_screen.premultiply(&ct);
        }
        let children = widget.children();
        if idx >= children.len() {
            return None;
        }
        widget = children[idx].as_ref();
    }
    // `to_screen` now maps the focused widget's parent-local coords → screen;
    // apply it to the leaf's own bounds rect and take the axis-aligned bound
    // (exact for translation + uniform scale, conservative otherwise).
    Some(transform_rect_aabb(&to_screen, widget.bounds()))
}

/// AABB of `rect`'s four corners after passing through `t`.  Mirrors the
/// inspector's helper of the same name — duplicated here (a couple of lines)
/// rather than made cross-module public to keep the two walks independent.
fn transform_rect_aabb(t: &crate::TransAffine, rect: Rect) -> Rect {
    let corners = [
        (rect.x, rect.y),
        (rect.x + rect.width, rect.y),
        (rect.x, rect.y + rect.height),
        (rect.x + rect.width, rect.y + rect.height),
    ];
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (mut x, mut y) in corners {
        t.transform(&mut x, &mut y);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Rect::new(min_x, min_y, (max_x - min_x).max(0.0), (max_y - min_y).max(0.0))
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

#[cfg(test)]
mod tests {
    use super::focused_widget_screen_bounds;
    use crate::draw_ctx::DrawCtx;
    use crate::event::{Event, EventResult};
    use crate::geometry::{Rect, Size};
    use crate::widget::Widget;
    use crate::TransAffine;

    /// Parent that scales+translates its children — stands in for a `Scene`.
    struct ScaleParent {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
        zoom: f64,
        offset: (f64, f64),
    }
    impl Widget for ScaleParent {
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
        fn layout(&mut self, a: Size) -> Size {
            a
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn child_transform(&self) -> Option<TransAffine> {
            let mut m = TransAffine::new_scaling_uniform(self.zoom);
            m.translate(self.offset.0, self.offset.1);
            Some(m)
        }
    }
    struct Leaf {
        bounds: Rect,
        children: Vec<Box<dyn Widget>>,
    }
    impl Widget for Leaf {
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
        fn layout(&mut self, _: Size) -> Size {
            Size::new(self.bounds.width, self.bounds.height)
        }
        fn paint(&mut self, _: &mut dyn DrawCtx) {}
        fn on_event(&mut self, _: &Event) -> EventResult {
            EventResult::Ignored
        }
    }

    /// A focused leaf hosted under a 2× scale+offset parent must report its
    /// screen bounds through that transform — otherwise the keyboard lift uses
    /// the un-scaled scene position and raises the field by the wrong amount.
    #[test]
    fn focused_bounds_honor_child_transform() {
        let leaf = Leaf {
            bounds: Rect::new(5.0, 5.0, 8.0, 4.0),
            children: vec![],
        };
        let parent = ScaleParent {
            bounds: Rect::new(0.0, 0.0, 400.0, 300.0),
            children: vec![Box::new(leaf)],
            zoom: 2.0,
            offset: (10.0, 20.0),
        };
        // Screen = 2*local + offset (parent at origin):
        //   x = 10 + 5*2 = 20, y = 20 + 5*2 = 30, w = 8*2 = 16, h = 4*2 = 8.
        let r = focused_widget_screen_bounds(&parent, &[0]).expect("bounds");
        assert!(
            (r.x - 20.0).abs() < 1e-6
                && (r.y - 30.0).abs() < 1e-6
                && (r.width - 16.0).abs() < 1e-6
                && (r.height - 8.0).abs() < 1e-6,
            "focused bounds must reflect the parent's child_transform; got {:?}",
            r
        );
    }

    /// Without a child transform the walk still reduces to plain bounds
    /// accumulation (regression guard for the rewrite).
    #[test]
    fn focused_bounds_plain_bounds_accumulate() {
        let leaf = Leaf {
            bounds: Rect::new(5.0, 7.0, 8.0, 4.0),
            children: vec![],
        };
        // ScaleParent with zoom 1, offset 0 == identity child transform.
        let parent = ScaleParent {
            bounds: Rect::new(30.0, 40.0, 400.0, 300.0),
            children: vec![Box::new(leaf)],
            zoom: 1.0,
            offset: (0.0, 0.0),
        };
        let r = focused_widget_screen_bounds(&parent, &[0]).expect("bounds");
        // 30 + 5 = 35, 40 + 7 = 47, size unchanged.
        assert!(
            (r.x - 35.0).abs() < 1e-6
                && (r.y - 47.0).abs() < 1e-6
                && (r.width - 8.0).abs() < 1e-6
                && (r.height - 4.0).abs() < 1e-6,
            "plain bounds accumulation regressed; got {:?}",
            r
        );
    }
}
