//! Module-level state for the on-screen keyboard.
//!
//! Lives in a thread-local because the keyboard is a singleton — a
//! browser tab has exactly one on-screen keyboard at any time, and the
//! WASM target is single-threaded anyway. Native targets that want a
//! distinct keyboard per window would have to wrap this state in a
//! struct owned by the App; we don't ship that yet.

use std::cell::RefCell;
use std::time::Duration;
use web_time::Instant;

use crate::animation::Tween;

use super::key::PaintedKey;
use super::layouts::Layer;

/// Slide animation duration (seconds). Tuned to feel like an OS keyboard
/// raise — fast enough to not feel laggy, slow enough to register as a
/// transition rather than a snap.
pub const SLIDE_DURATION_SECS: f64 = 0.22;

/// All mutable state owned by the keyboard module.
///
/// Kept private to the module so callers can't accidentally diverge from
/// the controlled-mutation rules (e.g. retargeting the tween *also*
/// requires `request_draw` to wake the event loop).
pub struct KeyboardState {
    /// Host opted in via [`super::set_enabled`]. Defaults to `false` so
    /// desktop apps that never call it see no keyboard.
    pub enabled: bool,
    /// Set by [`super::set_text_input_focused`] when the focused widget
    /// reports `accepts_text_input`.
    pub text_input_focused: bool,
    /// Slide animation. Value in `[0.0, 1.0]` interprets as
    /// "fraction of the keyboard panel visible from the bottom".
    pub slide: Tween,
    /// Active layer (lowercase letters / shifted letters / numbers /
    /// symbols).
    pub current_layer: Layer,
    /// Painted keys from the most recent paint pass. Used for tap
    /// hit-testing. Coordinates are in the same Y-up viewport space the
    /// paint pass uses.
    pub last_painted_keys: Vec<PaintedKey>,
    /// Height of the most recently painted panel in logical pixels.
    /// `None` until first paint. Used by [`super::occluded_height`] to
    /// report how much screen real estate the keyboard is consuming.
    pub last_panel_height: Option<f64>,
    /// Index into `last_painted_keys` of the key currently held down,
    /// if any. Cleared when the pointer is released or moves off the
    /// panel.
    pub pressed_key_index: Option<usize>,
    /// `true` between MouseDown and MouseUp on the panel — used by
    /// the move/up handlers to know they should keep consuming events.
    pub captured_pointer: bool,
    /// `true` if the user has toggled caps lock on (via double-tap
    /// shift). Holds the keyboard in the `Shifted` layer until shift
    /// is tapped again.
    pub caps_lock: bool,
    /// Most recent shift-key tap, used to detect double-tap → caps lock.
    /// Cleared after a non-shift key press or after the double-tap
    /// window expires.
    pub last_shift_tap: Option<Instant>,
    /// State machine for the held key (currently only Backspace). When
    /// set we keep firing the key every `repeat_period` after an
    /// initial delay, until the pointer releases / leaves.
    pub key_repeat: Option<KeyRepeatState>,
    /// Set by [`super::dismiss`] when the user taps the keyboard's
    /// close key.  Drained once per event-loop iteration by the App
    /// (see `App::drain_keyboard_events`), which calls
    /// `set_focus(None)` so the previously-focused text field gets a
    /// `FocusLost` and the keyboard-aware lift retargets back to 0 —
    /// otherwise the keyboard panel slides down but the tree stays
    /// lifted, leaving an empty band where the keyboard used to be.
    pub dismiss_requested: bool,
}

/// Hold-to-repeat state captured the moment the user presses a
/// repeat-eligible key (currently Backspace only). Polled from
/// [`super::paint_software_keyboard`] every frame so the firing cadence
/// happens in lockstep with the animation loop — no separate timer
/// thread, fully WASM-friendly.
#[derive(Debug, Clone, Copy)]
pub struct KeyRepeatState {
    /// Index into `last_painted_keys`. We re-check the key still exists
    /// and is still under the pointer each tick.
    pub key_index: usize,
    /// When the user pressed the key down. `held_for` = now - pressed_at.
    pub pressed_at: Instant,
    /// When we last fired a synthetic key for this hold. `None` = never
    /// fired; the first fire happens after `initial_delay` elapses.
    pub last_fired_at: Option<Instant>,
}

impl KeyRepeatState {
    /// How long the user must hold before the first repeat fires.
    pub const INITIAL_DELAY: Duration = Duration::from_millis(450);
    /// Period between subsequent repeats. Constant for now; we could
    /// ramp it down for an accelerating delete-line feel later.
    pub const REPEAT_PERIOD: Duration = Duration::from_millis(70);
}

/// Maximum gap between two Shift taps to count as a double-tap →
/// caps lock toggle.
pub const SHIFT_DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(350);

impl Default for KeyboardState {
    fn default() -> Self {
        Self {
            enabled: false,
            text_input_focused: false,
            slide: Tween::new(0.0, SLIDE_DURATION_SECS),
            current_layer: Layer::Letters,
            last_painted_keys: Vec::new(),
            last_panel_height: None,
            pressed_key_index: None,
            captured_pointer: false,
            caps_lock: false,
            last_shift_tap: None,
            key_repeat: None,
            dismiss_requested: false,
        }
    }
}

impl KeyboardState {
    /// Current eased visible fraction. Wraps [`Tween::value`] so callers
    /// outside the module don't need a mutable borrow just to peek.
    pub fn visible_fraction(&self) -> f64 {
        self.slide.value()
    }
}

thread_local! {
    static STATE: RefCell<KeyboardState> = RefCell::new(KeyboardState::default());
}

pub fn with_state_ref<R>(f: impl FnOnce(&KeyboardState) -> R) -> R {
    STATE.with(|cell| f(&cell.borrow()))
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut KeyboardState) -> R) -> R {
    STATE.with(|cell| f(&mut cell.borrow_mut()))
}
