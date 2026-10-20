//! Touch → mouse emulation for the primary finger.
//!
//! Companion to [`crate::touch_state`]: while `TouchState` aggregates
//! two-plus-finger gestures into a per-frame [`crate::MultiTouchInfo`],
//! this module makes SINGLE-finger touches drive the existing mouse
//! event pipeline so every widget keeps working without touch-specific
//! code.  [`crate::widget::App`] owns a [`TouchMouseEmu`] and feeds it
//! from `App::on_touch_start/move/end/cancel`; the returned [`EmuCmd`]s
//! are replayed through `App::on_mouse_*`.  Platform shells (web JS,
//! native winit) therefore forward RAW touches only — they must not
//! synthesize mouse events themselves, or every event would fire twice.
//!
//! The gesture contract (ported from the original JS shell in
//! `demo/src/app.ts`):
//!
//!   - A tap (release before moving [`TOUCH_SCROLL_THRESHOLD`] points,
//!     that many logical units times [`crate::device_scale`] in the
//!     physical pixels touches arrive in) is a left click at the release
//!     point.
//!   - A drag past the threshold becomes a MIDDLE-button drag starting
//!     at the touch-down point — `ScrollView` pans on middle-drag, so a
//!     finger drag scrolls by the exact finger delta.  Canvas-style
//!     widgets that want to own finger drags (e.g. the lion demo)
//!     consume middle-drag instead of left-drag.  While that press is
//!     dispatched, [`touch_pan_travel`] reports the finger's travel so a
//!     `ScrollView` takes it only when it can scroll along the drag.
//!   - A drag nothing takes as a middle-drag is the widget's own drag (a
//!     slider, a 3D view): `App` calls [`TouchMouseEmu::press_instead`]
//!     and presses the LEFT button where the finger went down.  Such a
//!     press ends in [`EmuCmd::CancelPress`] (the capture is cancelled,
//!     not released), so a gesture past the tap tolerance never clicks.
//!   - The moment a second finger lands, single-finger emulation is
//!     suppressed until every finger lifts: an in-flight middle drag is
//!     released immediately (a widget's own left drag is cancelled), and
//!     the eventual release emits no click.  (The multi-finger gesture is
//!     reported via `MultiTouchInfo` instead.)  This also kills the
//!     phantom click a pinch used to fire when the first finger barely
//!     moved, and lets go of a press whose release a host dropped.
//!   - A finger held within [`LONG_PRESS_SLOP`] of where it went down for
//!     [`LONG_PRESS_DELAY`] (no drag, no second finger) is a long press: a
//!     RIGHT press and release at the touch-down point, so it opens the
//!     context menu wherever a right click does. The finger is then spent:
//!     its moves and its lift emit nothing. The emulator reads the UI clock
//!     ([`crate::clock`]); `App` polls it every frame ([`TouchMouseEmu::poll`])
//!     and every touch event checks the deadline first, so a late frame
//!     never turns a hold into a tap or a drag. The cue, the touch-origin
//!     flag and the listeners are in [`long_press`].
//!
//! All coordinates are the same screen-space (Y-down, physical-pixel)
//! units the `App::on_mouse_*` entry points accept.

use std::cell::Cell;
use std::collections::BTreeSet;

use web_time::Instant;

use crate::event::MouseButton;
use crate::touch_state::{TouchDeviceId, TouchId};

pub mod long_press;
pub use long_press::{
    last_press_was_touch, long_press_cue, long_press_ring_drawn, on_long_press,
    set_long_press_ring_drawn, LongPressCue, LONG_PRESS_CUE_DELAY, LONG_PRESS_DELAY,
    LONG_PRESS_RING_RADIUS, LONG_PRESS_SLOP,
};

/// Distance (logical points) the primary finger must travel before the
/// gesture stops being a potential tap and becomes a drag. Touches arrive
/// in physical pixels, so the emulator compares against this times
/// [`crate::device_scale`] (the monitor's pixels per point, not the UX
/// zoom): on a devicePixelRatio 3.5 phone a fixed 8 physical pixels is
/// about 2 CSS pixels, and ordinary finger jitter turned every tap into a
/// drag (agg-sharp `TouchTapToleranceTests`). It is also the long press's
/// slop ([`LONG_PRESS_SLOP`]): a finger held for a context menu drifts
/// more than a tapping one, and a hold that drifts must not start a scroll.
pub const TOUCH_SCROLL_THRESHOLD: f64 = 10.0;

thread_local! {
    static TOUCH_PAN_TRAVEL: Cell<Option<(f64, f64)>> = const { Cell::new(None) };
}

/// The finger's travel (logical units, Y-up: `x` right, `y` up) while
/// `App` dispatches the middle press of a touch drag, else `None`. A
/// `ScrollView` takes that press only when it can scroll along the
/// drag's main axis, leaving a drag it cannot scroll to its ancestors
/// and, when none can, to the widget under the finger as a left drag.
pub fn touch_pan_travel() -> Option<(f64, f64)> {
    TOUCH_PAN_TRAVEL.with(|t| t.get())
}

/// Set (or clear) [`touch_pan_travel`] around a dispatch.
pub(crate) fn set_touch_pan_travel(travel: Option<(f64, f64)>) {
    TOUCH_PAN_TRAVEL.with(|t| t.set(travel));
}

/// A mouse event the emulator wants replayed through `App::on_mouse_*`.
/// Coordinates are screen-space, matching those entry points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum EmuCmd {
    MouseMove(f64, f64),
    MouseDown(f64, f64, MouseButton),
    MouseUp(f64, f64, MouseButton),
    MouseLeave,
    /// End a widget's own left drag without a release: the capture holder
    /// gets `Event::MouseCaptureLost`, so a drag never clicks.
    CancelPress,
}

/// State machine tracking the primary finger.  Self-contained (keeps
/// its own active-finger set) so it can be unit-tested without an
/// `App`.
#[derive(Default)]
pub struct TouchMouseEmu {
    /// Every finger currently down, as reported through on_start/on_end.
    active: BTreeSet<(TouchDeviceId, TouchId)>,
    /// The finger driving mouse emulation, if any.
    primary: Option<(TouchDeviceId, TouchId)>,
    /// Primary's touch-down position — anchor for the tap-vs-drag
    /// threshold and the position of the synthetic middle MouseDown.
    start_pos: (f64, f64),
    /// Primary's most recent position — used for the tap click and the
    /// middle-button release (touch-end events carry no coordinates).
    last_pos: (f64, f64),
    /// True once the threshold was crossed and a middle-drag is in
    /// flight.
    scrolling: bool,
    /// True once the drag fell to the widget under the finger as a left
    /// press ([`Self::press_instead`]).
    pressed: bool,
    /// True once a second finger landed while this primary was active;
    /// no further mouse events are emitted until all fingers lift.
    suppressed: bool,
    /// When the primary finger went down, while it may still long press
    /// (no drag, no second finger, not fired yet).
    held_since: Option<Instant>,
    /// True once the long press fired: the finger's later moves and its
    /// lift emit nothing.
    spent: bool,
    /// The touch-down point of a long press that fired since the last
    /// [`Self::take_long_press`].
    fired: Option<(f64, f64)>,
}

impl TouchMouseEmu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_start(&mut self, device: TouchDeviceId, id: TouchId, x: f64, y: f64) -> Vec<EmuCmd> {
        self.active.insert((device, id));
        if self.primary.is_none() && self.active.len() == 1 {
            // First finger of a fresh gesture: becomes primary and moves
            // the hover position so the eventual tap hits the right
            // widget (matches the old JS shell's touchstart mouse-move).
            self.primary = Some((device, id));
            self.start_pos = (x, y);
            self.last_pos = (x, y);
            self.scrolling = false;
            self.pressed = false;
            self.suppressed = false;
            self.held_since = Some(crate::clock::now());
            self.spent = false;
            return vec![EmuCmd::MouseMove(x, y)];
        }
        // A hold that came due before this finger landed fires first.
        let mut cmds = self.poll();
        // Two fingers are a pinch or a pan, never a long press.
        self.held_since = None;
        // A second (or later) finger: hand the gesture over to the
        // multi-touch aggregate.  Release any in-flight middle drag so
        // ScrollView doesn't keep panning while the user pinches.
        if self.primary.is_some() && !self.suppressed {
            self.suppressed = true;
            if self.pressed {
                // A widget's own drag (or a press whose release a host
                // dropped) lets go without clicking.
                self.pressed = false;
                cmds.push(EmuCmd::CancelPress);
            } else if self.scrolling {
                self.scrolling = false;
                cmds.push(EmuCmd::MouseUp(
                    self.last_pos.0,
                    self.last_pos.1,
                    MouseButton::Middle,
                ));
            }
        }
        cmds
    }

    pub fn on_move(&mut self, device: TouchDeviceId, id: TouchId, x: f64, y: f64) -> Vec<EmuCmd> {
        if self.primary != Some((device, id)) || self.suppressed {
            return Vec::new();
        }
        // The deadline is checked against where the finger was until now,
        // before this move counts.
        let mut cmds = self.poll();
        if self.spent {
            return cmds;
        }
        if !self.scrolling && !self.pressed {
            let dx = x - self.start_pos.0;
            let dy = y - self.start_pos.1;
            let threshold = TOUCH_SCROLL_THRESHOLD * crate::device_scale();
            if (dx * dx + dy * dy).sqrt() >= threshold {
                // Threshold crossed: this is a drag, not a tap.  The
                // middle button goes down at the START point so the
                // scroll pan covers the full finger travel.
                self.scrolling = true;
                self.held_since = None;
                cmds.push(EmuCmd::MouseDown(
                    self.start_pos.0,
                    self.start_pos.1,
                    MouseButton::Middle,
                ));
            }
        }
        // The primary finger always drives the hover position, even
        // before the threshold — widgets show hover/pressed states.
        self.last_pos = (x, y);
        cmds.push(EmuCmd::MouseMove(x, y));
        cmds
    }

    /// The primary finger's travel from where it went down, in the
    /// screen-space (Y-down, physical-pixel) units of the commands.
    pub fn travel(&self) -> (f64, f64) {
        (
            self.last_pos.0 - self.start_pos.0,
            self.last_pos.1 - self.start_pos.1,
        )
    }

    /// Nothing took the drag's middle press: the gesture becomes the
    /// widget's own left drag, which `App` presses at the touch-down point.
    /// Later moves stay plain moves and the lift cancels the press
    /// ([`EmuCmd::CancelPress`]) instead of releasing it.
    pub fn press_instead(&mut self) {
        if self.scrolling {
            self.scrolling = false;
            self.pressed = true;
        }
    }

    /// The touch-down point of the primary finger (screen space).
    pub fn start_pos(&self) -> (f64, f64) {
        self.start_pos
    }

    pub fn on_end(&mut self, device: TouchDeviceId, id: TouchId) -> Vec<EmuCmd> {
        self.finish(device, id, /*allow_tap=*/ true)
    }

    /// Platform cancelled the touch (browser gesture hand-off, palm
    /// rejection, …).  Like `on_end` but a cancelled tap must NOT click.
    pub fn on_cancel(&mut self, device: TouchDeviceId, id: TouchId) -> Vec<EmuCmd> {
        self.finish(device, id, /*allow_tap=*/ false)
    }

    fn finish(&mut self, device: TouchDeviceId, id: TouchId, allow_tap: bool) -> Vec<EmuCmd> {
        let mut cmds = Vec::new();
        if self.primary == Some((device, id)) && allow_tap {
            // A hold that came due before the lift fires, not a tap.
            cmds = self.poll();
        }
        self.active.remove(&(device, id));
        if self.primary == Some((device, id)) {
            self.primary = None;
            self.held_since = None;
            let (x, y) = self.last_pos;
            if self.suppressed || self.spent {
                // Also the lift after a long press: its right click
                // already happened, so the lift only clears hover.
                // The gesture went multi-touch: the middle button was
                // already released when the second finger landed, so
                // only the hover state needs clearing — and crucially,
                // NO click fires.
                cmds.push(EmuCmd::MouseLeave);
            } else if self.pressed {
                self.pressed = false;
                cmds.push(EmuCmd::CancelPress);
                cmds.push(EmuCmd::MouseLeave);
            } else if self.scrolling {
                self.scrolling = false;
                cmds.push(EmuCmd::MouseUp(x, y, MouseButton::Middle));
                cmds.push(EmuCmd::MouseLeave);
            } else if allow_tap {
                cmds.push(EmuCmd::MouseDown(x, y, MouseButton::Left));
                cmds.push(EmuCmd::MouseUp(x, y, MouseButton::Left));
                cmds.push(EmuCmd::MouseLeave);
            } else {
                cmds.push(EmuCmd::MouseLeave);
            }
        }
        if self.active.is_empty() {
            // All fingers lifted: fully reset so the next touch can
            // become primary even after a suppressed gesture.
            self.primary = None;
            self.scrolling = false;
            self.pressed = false;
            self.suppressed = false;
            self.spent = false;
            self.held_since = None;
        }
        cmds
    }

    /// Fire the long press if it has come due on the UI clock: a right press
    /// and release at the touch-down point. Empty otherwise. `App` calls it
    /// every frame; every touch event calls it first.
    pub fn poll(&mut self) -> Vec<EmuCmd> {
        let due = self
            .long_press_deadline()
            .is_some_and(|deadline| crate::clock::now() >= deadline);
        if !due {
            return Vec::new();
        }
        self.held_since = None;
        self.spent = true;
        self.fired = Some(self.start_pos);
        let (x, y) = self.start_pos;
        vec![
            EmuCmd::MouseDown(x, y, MouseButton::Right),
            EmuCmd::MouseUp(x, y, MouseButton::Right),
        ]
    }

    /// This hold will not long press (the finger is on a surface where a
    /// right click means nothing, such as the on-screen keyboard): it stays
    /// a tap or a drag, whatever its length.
    pub fn cancel_long_press(&mut self) {
        self.held_since = None;
    }

    /// When the pending long press fires, if a finger is being held.
    pub fn long_press_deadline(&self) -> Option<Instant> {
        if self.primary.is_none() || self.suppressed || self.scrolling || self.pressed {
            return None;
        }
        self.held_since.map(|t| t + LONG_PRESS_DELAY)
    }

    /// The pending long press's touch-down point (screen space) and progress
    /// (0 at [`LONG_PRESS_CUE_DELAY`], 1 at the deadline), once the cue is
    /// due; `None` before that and when nothing is held.
    pub fn long_press_progress(&self) -> Option<((f64, f64), f64)> {
        let deadline = self.long_press_deadline()?;
        let held = crate::clock::since(deadline - LONG_PRESS_DELAY);
        if held < LONG_PRESS_CUE_DELAY {
            return None;
        }
        let span = (LONG_PRESS_DELAY - LONG_PRESS_CUE_DELAY).as_secs_f64();
        let progress = ((held - LONG_PRESS_CUE_DELAY).as_secs_f64() / span).min(1.0);
        Some((self.start_pos, progress))
    }

    /// The touch-down point of a long press that fired since the last call.
    pub fn take_long_press(&mut self) -> Option<(f64, f64)> {
        self.fired.take()
    }
}

#[cfg(test)]
mod long_press_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: TouchDeviceId = TouchDeviceId(0);
    const F1: TouchId = TouchId(1);
    const F2: TouchId = TouchId(2);
    const F3: TouchId = TouchId(3);

    #[test]
    fn tap_is_left_click_at_release_point() {
        let mut emu = TouchMouseEmu::new();
        assert_eq!(
            emu.on_start(DEV, F1, 100.0, 100.0),
            vec![EmuCmd::MouseMove(100.0, 100.0)]
        );
        // Sub-threshold jitter keeps it a tap and still tracks hover.
        assert_eq!(
            emu.on_move(DEV, F1, 103.0, 102.0),
            vec![EmuCmd::MouseMove(103.0, 102.0)]
        );
        assert_eq!(
            emu.on_end(DEV, F1),
            vec![
                EmuCmd::MouseDown(103.0, 102.0, MouseButton::Left),
                EmuCmd::MouseUp(103.0, 102.0, MouseButton::Left),
                EmuCmd::MouseLeave,
            ]
        );
    }

    #[test]
    fn drag_past_threshold_becomes_middle_drag_from_start_point() {
        let mut emu = TouchMouseEmu::new();
        emu.on_start(DEV, F1, 50.0, 50.0);
        // Crossing the threshold: middle-down at the START pos, then move.
        assert_eq!(
            emu.on_move(DEV, F1, 62.0, 50.0),
            vec![
                EmuCmd::MouseDown(50.0, 50.0, MouseButton::Middle),
                EmuCmd::MouseMove(62.0, 50.0),
            ]
        );
        // Further moves are plain moves.
        assert_eq!(
            emu.on_move(DEV, F1, 70.0, 55.0),
            vec![EmuCmd::MouseMove(70.0, 55.0)]
        );
        assert_eq!(
            emu.on_end(DEV, F1),
            vec![
                EmuCmd::MouseUp(70.0, 55.0, MouseButton::Middle),
                EmuCmd::MouseLeave
            ]
        );
    }

    #[test]
    fn second_finger_releases_middle_drag_and_suppresses() {
        let mut emu = TouchMouseEmu::new();
        emu.on_start(DEV, F1, 0.0, 0.0);
        emu.on_move(DEV, F1, 20.0, 0.0); // middle-drag in flight
                                         // Second finger lands → the drag must end IMMEDIATELY.
        assert_eq!(
            emu.on_start(DEV, F2, 100.0, 100.0),
            vec![EmuCmd::MouseUp(20.0, 0.0, MouseButton::Middle)]
        );
        // Suppressed: primary moves emit nothing.
        assert_eq!(emu.on_move(DEV, F1, 40.0, 0.0), Vec::<EmuCmd>::new());
        // Primary release: hover clear only, NO click, NO middle-up.
        assert_eq!(emu.on_end(DEV, F1), vec![EmuCmd::MouseLeave]);
        assert_eq!(emu.on_end(DEV, F2), Vec::<EmuCmd>::new());
        // A fresh finger afterwards becomes primary again.
        assert_eq!(
            emu.on_start(DEV, F3, 5.0, 5.0),
            vec![EmuCmd::MouseMove(5.0, 5.0)]
        );
    }

    #[test]
    fn pinch_with_still_primary_fires_no_phantom_click() {
        let mut emu = TouchMouseEmu::new();
        let mut all = Vec::new();
        all.extend(emu.on_start(DEV, F1, 100.0, 100.0));
        all.extend(emu.on_start(DEV, F2, 200.0, 100.0));
        all.extend(emu.on_move(DEV, F1, 98.0, 100.0)); // tiny move, < threshold
        all.extend(emu.on_move(DEV, F2, 205.0, 100.0));
        all.extend(emu.on_end(DEV, F2));
        all.extend(emu.on_end(DEV, F1));
        assert!(
            !all.iter()
                .any(|c| matches!(c, EmuCmd::MouseDown(_, _, MouseButton::Left))),
            "a pinch must never synthesize a left click, got: {all:?}"
        );
    }

    #[test]
    fn cancel_during_drag_releases_middle_without_click() {
        let mut emu = TouchMouseEmu::new();
        emu.on_start(DEV, F1, 0.0, 0.0);
        emu.on_move(DEV, F1, 30.0, 0.0);
        assert_eq!(
            emu.on_cancel(DEV, F1),
            vec![
                EmuCmd::MouseUp(30.0, 0.0, MouseButton::Middle),
                EmuCmd::MouseLeave
            ]
        );
    }

    #[test]
    fn cancel_before_threshold_fires_no_click() {
        let mut emu = TouchMouseEmu::new();
        emu.on_start(DEV, F1, 0.0, 0.0);
        assert_eq!(emu.on_cancel(DEV, F1), vec![EmuCmd::MouseLeave]);
    }

    #[test]
    fn non_primary_fingers_never_emit() {
        let mut emu = TouchMouseEmu::new();
        emu.on_start(DEV, F1, 0.0, 0.0);
        emu.on_start(DEV, F2, 50.0, 50.0);
        assert_eq!(emu.on_move(DEV, F2, 80.0, 80.0), Vec::<EmuCmd>::new());
        assert_eq!(emu.on_end(DEV, F2), Vec::<EmuCmd>::new());
        // F1 was suppressed by F2's arrival; its release is leave-only.
        assert_eq!(emu.on_end(DEV, F1), vec![EmuCmd::MouseLeave]);
    }
}
