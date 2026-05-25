//! Module-level state for the on-screen keyboard.
//!
//! Lives in a thread-local because the keyboard is a singleton — a
//! browser tab has exactly one on-screen keyboard at any time, and the
//! WASM target is single-threaded anyway. Native targets that want a
//! distinct keyboard per window would have to wrap this state in a
//! struct owned by the App; we don't ship that yet.

use std::cell::RefCell;

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
}

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
