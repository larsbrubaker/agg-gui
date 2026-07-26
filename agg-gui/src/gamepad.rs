//! # Gamepad input — platform shell → app
//!
//! Same flow as [`crate::tilt`]: platform shells poll their gamepad
//! API each frame (Web Gamepad API in the browser, a crate like
//! `gilrs` natively — shells own the dependency) and publish the
//! first active pad's state here; apps read [`state`] whenever they
//! gather input. `None` means no pad is connected (or none has been
//! touched yet — browsers hide pads until the first button press).
//!
//! Axes are in SCREEN convention: `left_x` positive toward the
//! screen's right edge, `left_y` positive toward its bottom (the
//! standard-mapping Gamepad API convention). Buttons use the
//! position-based [`buttons`] bits so apps don't care about
//! Xbox/PlayStation labels.

use std::sync::Mutex;

/// Button bits, by physical position (standard gamepad mapping).
pub mod buttons {
    /// Bottom face button (Xbox A / PS Cross).
    pub const SOUTH: u32 = 1 << 0;
    /// Right face button (Xbox B / PS Circle).
    pub const EAST: u32 = 1 << 1;
    /// Left face button (Xbox X / PS Square).
    pub const WEST: u32 = 1 << 2;
    /// Top face button (Xbox Y / PS Triangle).
    pub const NORTH: u32 = 1 << 3;
    pub const L1: u32 = 1 << 4;
    pub const R1: u32 = 1 << 5;
    pub const SELECT: u32 = 1 << 6;
    pub const START: u32 = 1 << 7;
    pub const DPAD_UP: u32 = 1 << 8;
    pub const DPAD_DOWN: u32 = 1 << 9;
    pub const DPAD_LEFT: u32 = 1 << 10;
    pub const DPAD_RIGHT: u32 = 1 << 11;
}

/// One pad's state, republished by the shell every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GamepadState {
    /// Left stick, -1..1 per axis, screen convention (y down).
    pub left_x: f64,
    pub left_y: f64,
    /// Pressed buttons as [`buttons`] bits.
    pub buttons: u32,
}

impl GamepadState {
    pub fn pressed(&self, bit: u32) -> bool {
        self.buttons & bit != 0
    }
}

static STATE: Mutex<Option<GamepadState>> = Mutex::new(None);

/// Shell side: publish the current pad state (`None` = disconnected).
pub fn set_state(state: Option<GamepadState>) {
    *STATE.lock().unwrap() = state;
}

/// App side: the most recent pad state, if a pad is live.
pub fn state() -> Option<GamepadState> {
    *STATE.lock().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrips_and_buttons_test() {
        set_state(Some(GamepadState {
            left_x: 0.5,
            left_y: -1.0,
            buttons: buttons::SOUTH | buttons::START,
        }));
        let s = state().unwrap();
        assert_eq!(s.left_x, 0.5);
        assert!(s.pressed(buttons::SOUTH));
        assert!(s.pressed(buttons::START));
        assert!(!s.pressed(buttons::EAST));
        set_state(None);
        assert_eq!(state(), None);
    }
}
