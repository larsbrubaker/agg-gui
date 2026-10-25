//! Long press: a finger held still opens the right-click (context) menu.
//!
//! The gesture itself lives in [`super::TouchMouseEmu`]: a primary finger
//! that stays within [`LONG_PRESS_SLOP`] of where it went down for
//! [`LONG_PRESS_DELAY`], with no second finger and no drag, becomes a right
//! press and release at the touch-down point, so every widget that answers a
//! right click answers a long press without touch-specific code. Afterwards
//! the finger is spent: its moves and its lift emit nothing.
//!
//! This module holds what the rest of the program sees of it:
//!
//!   - the timings ([`LONG_PRESS_DELAY`], [`LONG_PRESS_SLOP`],
//!     [`LONG_PRESS_CUE_DELAY`]);
//!   - the pending cue ([`long_press_cue`]): where the finger is held and
//!     how far the hold has come, which `App` publishes every frame and,
//!     unless an app turns it off ([`set_long_press_ring_drawn`]), draws as
//!     a thin ring filling around the finger ([`paint_ring`]);
//!   - whether the latest press came from a finger
//!     ([`last_press_was_touch`]), so a menu opened by it gets
//!     finger-sized rows and a menu the mouse opens does not;
//!   - listeners told when a long press fires ([`on_long_press`]), which is
//!     where a shell plays a haptic tick;
//!   - the claim a widget makes on the finger while it handles the right
//!     press ([`claim_long_press_drag`]), so the press stays down and the
//!     finger's slide reaches it (a text field extends a word selection).
//!
//! `App` drives it (`widget/app/touch.rs`): it polls the emulator at the
//! start of every layout, keeps a wake-up armed for the deadline so an idle
//! app still fires on time, and paints the ring after the widget tree.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;

/// How long a finger must stay still before it becomes a right click.
pub const LONG_PRESS_DELAY: Duration = Duration::from_millis(500);

/// How far (logical points, scaled to the physical pixels touches arrive
/// in by [`crate::device_scale`]) a held finger may drift and still long
/// press. It is the tap tolerance too ([`super::TOUCH_SCROLL_THRESHOLD`]):
/// anything that moves less is a tap or a hold, anything that moves more is
/// a drag, so a hold that drifts never starts a scroll.
pub const LONG_PRESS_SLOP: f64 = super::TOUCH_SCROLL_THRESHOLD;

/// How long into a hold the cue appears. A quick tap never shows it; a
/// deliberate hold sees the ring start filling well before it fires.
pub const LONG_PRESS_CUE_DELAY: Duration = Duration::from_millis(150);

/// Radius (logical points) of the default ring: just outside a fingertip,
/// so the part not under the finger stays visible.
pub const LONG_PRESS_RING_RADIUS: f64 = 30.0;

/// A long press in progress, for drawing feedback around the finger.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LongPressCue {
    /// The touch-down point in logical, Y-up viewport coordinates (the
    /// coordinates `App::paint` draws the tree in).
    pub x: f64,
    pub y: f64,
    /// How far the hold has come, 0 at [`LONG_PRESS_CUE_DELAY`] and 1 when it
    /// fires.
    pub progress: f64,
}

/// Callbacks told when a long press fires ([`on_long_press`]).
type Listeners = Vec<Box<dyn Fn(LongPressCue)>>;

thread_local! {
    static CUE: Cell<Option<LongPressCue>> = const { Cell::new(None) };
    static RING_DRAWN: Cell<bool> = const { Cell::new(true) };
    static EMULATING: Cell<bool> = const { Cell::new(false) };
    static LAST_PRESS_FROM_TOUCH: Cell<bool> = const { Cell::new(false) };
    static LISTENERS: RefCell<Listeners> = RefCell::new(Vec::new());
    static DRAG_CLAIMED: Cell<bool> = const { Cell::new(false) };
}

/// Called by a widget while it handles a long press's right press (a
/// `MouseDown` with [`MouseButton::Right`](crate::MouseButton::Right) for
/// which [`last_press_was_touch`] is true) to keep the finger: instead of
/// releasing the right button at once, `App` holds it down, the finger's
/// later moves arrive as `MouseMove`s at the widget (it holds the capture
/// because it consumed the press), and the lift is a right `MouseUp` where
/// the finger let go. A second finger or a platform cancel ends it with
/// `MouseCaptureLost`. Outside a long press it does nothing.
pub fn claim_long_press_drag() {
    DRAG_CLAIMED.with(|c| c.set(true));
}

/// Forget any claim, before `App` dispatches a long press's right press.
pub(crate) fn clear_drag_claim() {
    DRAG_CLAIMED.with(|c| c.set(false));
}

/// Whether the widget that handled the right press claimed the finger
/// ([`claim_long_press_drag`]); clears the claim.
pub(crate) fn take_drag_claim() -> bool {
    DRAG_CLAIMED.with(|c| c.replace(false))
}

/// The long press now under way, once its cue has appeared; `None` when no
/// finger is being held. Refreshed by `App` every frame.
pub fn long_press_cue() -> Option<LongPressCue> {
    CUE.with(|c| c.get())
}

/// Publish (or clear) the cue; `App` calls this every paint.
pub(crate) fn publish_cue(cue: Option<LongPressCue>) {
    CUE.with(|c| c.set(cue));
}

/// Whether `App` draws its default ring for the cue (on unless turned off).
/// An app that draws its own feedback from [`long_press_cue`] turns it off.
pub fn set_long_press_ring_drawn(drawn: bool) {
    RING_DRAWN.with(|c| c.set(drawn));
}

/// Whether the default ring is drawn ([`set_long_press_ring_drawn`]).
pub fn long_press_ring_drawn() -> bool {
    RING_DRAWN.with(|c| c.get())
}

/// Whether the most recent mouse press was made by a finger: a tap, a
/// touch drag's press or a long press's right click. A popup menu opened
/// from that press lays itself out for fingers (taller rows,
/// [`crate::widgets::menu::geometry::popup_touch_sized`]). A real mouse
/// press clears it.
pub fn last_press_was_touch() -> bool {
    LAST_PRESS_FROM_TOUCH.with(|c| c.get())
}

/// Run `f` with presses marked as coming from a finger; `App` replays the
/// emulator's commands inside it.
pub(crate) fn while_emulating<R>(f: impl FnOnce() -> R) -> R {
    let previous = EMULATING.with(|c| c.replace(true));
    let result = f();
    EMULATING.with(|c| c.set(previous));
    result
}

/// Record where the press now being dispatched came from (`App` calls this
/// for every press).
pub(crate) fn note_press() {
    let from_touch = EMULATING.with(|c| c.get());
    LAST_PRESS_FROM_TOUCH.with(|c| c.set(from_touch));
}

/// Mark the latest press as a finger's or not, as `App` does for each press
/// it dispatches, for tests that open a menu without an `App`.
#[cfg(test)]
pub(crate) fn set_last_press_was_touch_for_testing(from_touch: bool) {
    LAST_PRESS_FROM_TOUCH.with(|c| c.set(from_touch));
}

/// Call `listener` each time a long press fires, with the touch-down point
/// (logical, Y-up) and `progress` 1. Shells use it for a haptic tick; an app
/// can add its own. Listeners live for the rest of the thread.
pub fn on_long_press(listener: impl Fn(LongPressCue) + 'static) {
    LISTENERS.with(|l| l.borrow_mut().push(Box::new(listener)));
}

/// Tell every listener a long press fired at `at`.
pub(crate) fn notify(at: LongPressCue) {
    // Take the list out while calling, so a listener that registers another
    // listener cannot hit a double borrow.
    let listeners = LISTENERS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    for listener in &listeners {
        listener(at);
    }
    LISTENERS.with(|l| {
        let mut current = l.borrow_mut();
        let added = std::mem::take(&mut *current);
        *current = listeners;
        current.extend(added);
    });
}

/// Draw the default cue: a faint full circle with an arc in the theme's
/// accent colour filling clockwise from the top as the hold progresses.
/// `ctx` is in the logical, Y-up coordinates the cue is given in.
pub(crate) fn paint_ring(ctx: &mut dyn DrawCtx, cue: LongPressCue) {
    let accent: Color = crate::theme::current_visuals().accent;
    let r = LONG_PRESS_RING_RADIUS;
    ctx.save();
    ctx.set_line_width(3.0);
    ctx.set_stroke_color(accent.with_alpha(0.25));
    ctx.begin_path();
    ctx.circle(cue.x, cue.y, r);
    ctx.stroke();
    let progress = cue.progress.clamp(0.0, 1.0);
    if progress > 0.0 {
        let top = std::f64::consts::FRAC_PI_2;
        let sweep = progress * std::f64::consts::TAU;
        ctx.set_stroke_color(accent.with_alpha(0.9));
        ctx.begin_path();
        // Y-up: clockwise runs towards decreasing angle.
        ctx.arc_to(cue.x, cue.y, r, top, top - sweep, false);
        ctx.stroke();
    }
    ctx.restore();
}
