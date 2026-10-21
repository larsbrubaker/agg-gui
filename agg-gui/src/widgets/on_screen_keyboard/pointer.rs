//! Pointer routing for the on-screen keyboard: press, slide and release on
//! the keys, the commit of a key, Shift / Caps Lock, and hold-to-repeat.
//!
//! Split out of [`super`] (the public API and paint entry point) so each
//! file stays focused. Every point of the panel belongs to a key: the hit
//! areas come from [`super::hit`], so a touch in a gap types the nearest
//! key, and a finger sliding across the keys types the one under it when
//! it lifts, as on the phone's own keyboard.

use crate::event::{Key, Modifiers, MouseButton};
use crate::geometry::Point;

use super::events::push_synthetic_key;
use super::layouts::Layer;
use super::state::{self, with_state_mut, with_state_ref};
use super::{dismiss, is_visible, key};
// ---------------------------------------------------------------------------
// Pointer routing
// ---------------------------------------------------------------------------

/// `true` when the keyboard panel currently occupies `pos` and would
/// consume an event there.
pub fn contains_point(pos: Point) -> bool {
    if !is_visible() {
        return false;
    }
    with_state_ref(|s| {
        let frac = s.slide.value();
        if frac <= 0.001 {
            return false;
        }
        let panel_height = s.last_panel_height.unwrap_or(0.0);
        let panel_top = panel_height * frac;
        // Panel occupies [0, panel_top] in Y-up viewport coords.
        pos.y >= 0.0 && pos.y <= panel_top
    })
}

/// Handle a pointer-down inside the keyboard. Returns `true` if consumed
/// (the [`App`](crate::widget::App) skips its normal tree dispatch).
pub fn handle_software_keyboard_mouse_down(
    pos: Point,
    button: MouseButton,
    _modifiers: Modifiers,
) -> bool {
    if button != MouseButton::Left {
        return contains_point(pos);
    }
    if !contains_point(pos) {
        return false;
    }
    let hit = find_key_at(pos);
    with_state_mut(|s| {
        s.pressed_key_index = hit;
        s.captured_pointer = true;
        // Register a hold-to-repeat tracker if the pressed key supports
        // it (currently Backspace only).
        s.key_repeat = hit.and_then(|i| {
            s.last_painted_keys.get(i).and_then(|k| match k.action {
                key::KeyAction::Backspace => Some(state::KeyRepeatState {
                    key_index: i,
                    pressed_at: crate::clock::now(),
                    last_fired_at: None,
                }),
                _ => None,
            })
        });
    });
    if hit.is_some() {
        crate::animation::request_draw();
    }
    true
}

/// Handle a pointer-move while the keyboard is interactive. Returns
/// `true` if the keyboard wants to keep the pointer captured.
///
/// A finger sliding across the keys moves the pressed highlight (and the
/// key preview) with it, as on the phone; sliding off the panel clears it,
/// so lifting there types nothing.
pub fn handle_software_keyboard_mouse_move(pos: Point) -> bool {
    let captured = with_state_ref(|s| s.captured_pointer);
    if !captured {
        return false;
    }
    let new_hit = if contains_point(pos) {
        find_key_at(pos)
    } else {
        None
    };
    with_state_mut(|s| {
        if s.pressed_key_index != new_hit {
            s.pressed_key_index = new_hit;
            crate::animation::request_draw();
        }
    });
    true
}

/// Handle a pointer-up. The key under the finger when it lifts types,
/// wherever the press started (a finger that slid from one key to the
/// next types the second, as on the phone). Lifting off the panel types
/// nothing.
pub fn handle_software_keyboard_mouse_up(
    pos: Point,
    button: MouseButton,
    modifiers: Modifiers,
) -> bool {
    let captured = with_state_ref(|s| s.captured_pointer);
    if !captured {
        return false;
    }
    let pressed = with_state_mut(|s| {
        let p = s.pressed_key_index.take();
        s.captured_pointer = false;
        let repeat_fired = s
            .key_repeat
            .map(|r| r.last_fired_at.is_some())
            .unwrap_or(false);
        s.key_repeat = None;
        (p, repeat_fired)
    });
    let (pressed_idx, repeat_already_fired) = pressed;
    if button != MouseButton::Left {
        crate::animation::request_draw();
        return true;
    }
    let on_panel = contains_point(pos);
    let final_hit = if on_panel { find_key_at(pos) } else { None };
    if let Some(end) = final_hit {
        // Suppress the commit if hold-to-repeat already fired during
        // the press on this same key — otherwise the release would
        // synthesize one extra Backspace after the user lifted.
        let repeated_here = repeat_already_fired && pressed_idx == Some(end);
        if !repeated_here {
            commit_key_press(end, modifiers);
        }
    }
    crate::animation::request_draw();
    true
}

/// The key that owns `pos`: the one whose hit area (see [`super::hit`])
/// contains it, else the key nearest to it, so no point on the panel
/// types nothing.
pub(super) fn find_key_at(pos: Point) -> Option<usize> {
    with_state_ref(|s| super::hit::key_at(&s.last_painted_keys, pos))
}
fn commit_key_press(index: usize, modifiers: Modifiers) {
    let painted = with_state_ref(|s| s.last_painted_keys.get(index).cloned());
    let Some(painted) = painted else {
        return;
    };
    // Clear any pending shift-double-tap detection on a non-shift commit
    // so a Shift tap that's *not* immediately followed by another Shift
    // tap doesn't accidentally promote to caps-lock when the user later
    // taps Shift unrelated.
    let is_shift_action = matches!(painted.action, key::KeyAction::Switch(Layer::Shifted));
    if !is_shift_action {
        with_state_mut(|s| s.last_shift_tap = None);
    }
    match painted.action {
        key::KeyAction::Char(c) => {
            let mut mods = modifiers;
            let was_shifted = with_state_ref(|s| s.current_layer == Layer::Shifted);
            if was_shifted {
                mods.shift = true;
            }
            push_synthetic_key(Key::Char(c), mods);
            // One-shot shift: drop back to base layer after a single
            // character — unless caps lock is engaged, in which case
            // stay in Shifted.
            with_state_mut(|s| {
                if s.current_layer == Layer::Shifted && !s.caps_lock {
                    s.current_layer = Layer::Letters;
                }
            });
        }
        key::KeyAction::Backspace => push_synthetic_key(Key::Backspace, modifiers),
        key::KeyAction::Enter => {
            push_synthetic_key(Key::Enter, modifiers);
        }
        key::KeyAction::Space => push_synthetic_key(Key::Char(' '), modifiers),
        key::KeyAction::Switch(target) => {
            handle_layer_switch(target);
        }
        key::KeyAction::Dismiss => dismiss(),
    }
    crate::animation::request_draw();
}

/// Apply a layer-switch action with special handling for the Shift
/// key:
/// - First tap → toggle into [`Layer::Shifted`] (one-shot upper case).
/// - Second tap within [`state::SHIFT_DOUBLE_TAP_WINDOW`] → engage caps
///   lock; keyboard stays Shifted until shift is tapped again.
/// - Tap while caps lock is on → release caps lock + drop to lowercase.
/// - Any other layer switch (?123 / ABC / =\< / 1234) just changes the
///   layer and clears caps-lock state.
pub(super) fn handle_layer_switch(target: Layer) {
    if target == Layer::Shifted || target == Layer::Letters {
        with_state_mut(|s| {
            let now = crate::clock::now();
            let recently_tapped = s
                .last_shift_tap
                .map(|t| now.duration_since(t) <= state::SHIFT_DOUBLE_TAP_WINDOW)
                .unwrap_or(false);

            if s.caps_lock {
                // Caps lock release: tap shift → drop back to lowercase.
                s.caps_lock = false;
                s.current_layer = Layer::Letters;
                s.last_shift_tap = None;
            } else if recently_tapped {
                // Double-tap → caps lock on.
                s.caps_lock = true;
                s.current_layer = Layer::Shifted;
                s.last_shift_tap = None;
            } else {
                // First tap → one-shot shift (or unshift if currently Shifted).
                s.current_layer = match s.current_layer {
                    Layer::Shifted => Layer::Letters,
                    _ => Layer::Shifted,
                };
                s.last_shift_tap = Some(now);
            }
        });
    } else {
        with_state_mut(|s| {
            s.current_layer = target;
            s.last_shift_tap = None;
        });
    }
}

/// Advance the hold-to-repeat state machine. Called once per paint so
/// the cadence rides on the animation loop. When the held key has been
/// down long enough we synthesize a `Backspace` and request another
/// draw so the loop keeps pumping for the next repeat.
pub(super) fn tick_key_repeat() {
    let now = crate::clock::now();
    let action = with_state_mut(|s| {
        let repeat = s.key_repeat.as_mut()?;
        // Repeat is only valid while the user is still holding the key
        // (captured_pointer == true && pressed_key_index matches).
        if !s.captured_pointer || s.pressed_key_index != Some(repeat.key_index) {
            s.key_repeat = None;
            return None;
        }
        let held = now.duration_since(repeat.pressed_at);
        let should_fire = match repeat.last_fired_at {
            None => held >= state::KeyRepeatState::INITIAL_DELAY,
            Some(t) => now.duration_since(t) >= state::KeyRepeatState::REPEAT_PERIOD,
        };
        if should_fire {
            let key = s.last_painted_keys.get(repeat.key_index)?.action;
            repeat.last_fired_at = Some(now);
            return Some(key);
        }
        None
    });
    if let Some(action) = action {
        if let key::KeyAction::Backspace = action {
            push_synthetic_key(Key::Backspace, Modifiers::default());
        }
        // Keep the loop hot for the next tick.
        crate::animation::request_draw();
    }
}
