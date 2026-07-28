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
//!   - A tap (release before moving [`TOUCH_SCROLL_THRESHOLD`] px) is a
//!     left click at the release point.
//!   - A drag past the threshold becomes a MIDDLE-button drag starting
//!     at the touch-down point — `ScrollView` pans on middle-drag, so a
//!     finger drag scrolls by the exact finger delta.  Canvas-style
//!     widgets that want to own finger drags (e.g. the lion demo)
//!     consume middle-drag instead of left-drag.
//!   - The moment a second finger lands, single-finger emulation is
//!     suppressed until every finger lifts: an in-flight middle drag is
//!     released immediately, and the eventual release emits no click.
//!     (The multi-finger gesture is reported via `MultiTouchInfo`
//!     instead.)  This also kills the phantom click a pinch used to
//!     fire when the first finger barely moved.
//!
//! All coordinates are the same screen-space (Y-down, physical-pixel)
//! units the `App::on_mouse_*` entry points accept.

use std::collections::BTreeSet;

use crate::event::MouseButton;
use crate::touch_state::{TouchDeviceId, TouchId};

/// Distance (physical px) the primary finger must travel before the
/// gesture stops being a potential tap and becomes a middle-drag.
pub const TOUCH_SCROLL_THRESHOLD: f64 = 8.0;

/// A mouse event the emulator wants replayed through `App::on_mouse_*`.
/// Coordinates are screen-space, matching those entry points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum EmuCmd {
    MouseMove(f64, f64),
    MouseDown(f64, f64, MouseButton),
    MouseUp(f64, f64, MouseButton),
    MouseLeave,
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
    /// True once a second finger landed while this primary was active;
    /// no further mouse events are emitted until all fingers lift.
    suppressed: bool,
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
            self.suppressed = false;
            return vec![EmuCmd::MouseMove(x, y)];
        }
        // A second (or later) finger: hand the gesture over to the
        // multi-touch aggregate.  Release any in-flight middle drag so
        // ScrollView doesn't keep panning while the user pinches.
        if self.primary.is_some() && !self.suppressed {
            self.suppressed = true;
            if self.scrolling {
                self.scrolling = false;
                return vec![EmuCmd::MouseUp(
                    self.last_pos.0,
                    self.last_pos.1,
                    MouseButton::Middle,
                )];
            }
        }
        Vec::new()
    }

    pub fn on_move(&mut self, device: TouchDeviceId, id: TouchId, x: f64, y: f64) -> Vec<EmuCmd> {
        if self.primary != Some((device, id)) || self.suppressed {
            return Vec::new();
        }
        let mut cmds = Vec::new();
        if !self.scrolling {
            let dx = x - self.start_pos.0;
            let dy = y - self.start_pos.1;
            if (dx * dx + dy * dy).sqrt() >= TOUCH_SCROLL_THRESHOLD {
                // Threshold crossed: this is a drag, not a tap.  The
                // middle button goes down at the START point so the
                // scroll pan covers the full finger travel.
                self.scrolling = true;
                cmds.push(EmuCmd::MouseDown(
                    self.start_pos.0,
                    self.start_pos.1,
                    MouseButton::Middle,
                ));
            }
        }
        // The primary finger always drives the hover position, even
        // before the threshold — widgets show hover/pressed states.
        cmds.push(EmuCmd::MouseMove(x, y));
        self.last_pos = (x, y);
        cmds
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
        self.active.remove(&(device, id));
        let mut cmds = Vec::new();
        if self.primary == Some((device, id)) {
            self.primary = None;
            let (x, y) = self.last_pos;
            if self.suppressed {
                // The gesture went multi-touch: the middle button was
                // already released when the second finger landed, so
                // only the hover state needs clearing — and crucially,
                // NO click fires.
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
            self.suppressed = false;
        }
        cmds
    }
}

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
