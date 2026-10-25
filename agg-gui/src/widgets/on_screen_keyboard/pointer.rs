//! Pointer routing for the on-screen keyboard: press, slide and release on
//! the keys, the commit of a key, Shift / Caps Lock, and hold-to-repeat.
//!
//! Split out of [`super`] (the public API and paint entry point) so each
//! file stays focused. Every point of the panel belongs to a key: the hit
//! areas come from [`super::hit`], so a touch in a gap types the nearest
//! key, and a finger sliding across the keys types the one under it when
//! it lifts, as on the phone's own keyboard. Holding a key with long-press
//! alternates opens them (`alternates.rs`): the finger then picks one by
//! sliding along the popup's row and types it by lifting. A press on the
//! space bar (and Gboard's Backspace) may instead become a caret slide or the
//! iPhone's trackpad, which `cursor_slide.rs` runs from these handlers.

use crate::event::{Key, Modifiers, MouseButton};
use crate::geometry::Point;

use super::alternates::{alternates_for, AlternatesPopup, LONG_PRESS_DELAY};
use super::cursor_slide;
use super::events::push_synthetic_key;
use super::layouts::{Layer, LayoutFamily};
use super::state::{self, with_state_mut, with_state_ref, HoldState, KeyboardState};
use super::style::Style;
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
        s.alternates = None;
        s.linger = None;
        s.hold = hold_for(s, hit);
        cursor_slide::arm(s, hit, pos, family());
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

/// How long a released key stays lit with its preview.
pub(super) const PRESS_LINGER: std::time::Duration = std::time::Duration::from_millis(100);

/// The key drawn pressed: the one under the finger, else one released
/// less than [`PRESS_LINGER`] ago.
pub(super) fn drawn_pressed(s: &KeyboardState) -> Option<usize> {
    if cursor_slide::trackpad_active(s) {
        // The iPhone's trackpad shows no key pressed.
        return None;
    }
    s.pressed_key_index.or_else(|| {
        s.linger
            .filter(|(_, until)| crate::clock::now() < *until)
            .map(|(i, _)| i)
    })
}

/// Keep the app's wake-ups for the keyboard's own timed changes: the moment
/// a released key's linger ends and the moment a held key's alternates are
/// due. Nothing else changes the screen then, so without a scheduled draw a
/// host that sleeps while idle (the web and desktop shells) left the
/// preview up, or the alternates closed, until some unrelated repaint.
/// `App::paint` clears every scheduled draw as each frame starts, so the
/// wake the press or lift asked for is gone after the frame it also asked
/// for; called on every paint, this asks again until the moment passes.
pub(super) fn rearm_timed_wakes() {
    let now = crate::clock::now();
    let due = with_state_ref(|s| {
        [
            s.linger.map(|(_, until)| until),
            s.hold.map(|h| h.pressed_at + LONG_PRESS_DELAY),
            cursor_slide::due(s),
        ]
    });
    for when in due.into_iter().flatten() {
        if when > now {
            crate::animation::request_draw_after(when - now);
        }
    }
}

/// The keyboard family the active input profile draws.
fn family() -> LayoutFamily {
    LayoutFamily::for_profile(crate::input_profile::current_input_profile())
}

/// A long-press timer for painted key `hit` when it has alternates, armed
/// so the app wakes when the popup is due.
fn hold_for(s: &KeyboardState, hit: Option<usize>) -> Option<HoldState> {
    let index = hit?;
    let key::KeyAction::Char(c) = s.last_painted_keys.get(index)?.action else {
        return None;
    };
    alternates_for(c, family())?;
    crate::animation::request_draw_after(LONG_PRESS_DELAY);
    Some(HoldState {
        key_index: index,
        pressed_at: crate::clock::now(),
    })
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
    // A caret slide or trackpad owns the finger once it has begun.
    if cursor_slide::track(pos) {
        return true;
    }
    // With the alternates open the finger picks along their row.
    let tracked = with_state_mut(|s| match s.alternates.as_mut() {
        Some(popup) => {
            let before = popup.highlighted;
            popup.track(pos);
            if popup.highlighted != before {
                crate::animation::request_draw();
            }
            true
        }
        None => false,
    });
    if tracked {
        return true;
    }
    let new_hit = if contains_point(pos) {
        find_key_at(pos)
    } else {
        None
    };
    with_state_mut(|s| {
        if s.pressed_key_index != new_hit {
            s.pressed_key_index = new_hit;
            // A finger that moved to another key starts its long press over.
            s.hold = hold_for(s, new_hit);
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
    // A caret slide ends with the lift and types nothing.
    if cursor_slide::release() {
        return true;
    }
    // Lifting with the alternates open types the highlighted one (or
    // nothing, away from the row) and nothing else.
    let popup = with_state_mut(|s| {
        let popup = s.alternates.take()?;
        s.pressed_key_index = None;
        s.captured_pointer = false;
        s.key_repeat = None;
        s.hold = None;
        Some(popup)
    });
    if let Some(popup) = popup {
        // The highlight is where the last move left it: a finger that never
        // moved types the default, wherever the row had to sit.
        let _ = pos;
        if button == MouseButton::Left {
            if let Some(c) = popup.chosen() {
                with_state_mut(|s| s.last_shift_tap = None);
                type_char(c, modifiers);
            }
        }
        crate::animation::request_draw();
        return true;
    }
    let pressed = with_state_mut(|s| {
        s.hold = None;
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
        // A character key stays lit with its preview a moment after the
        // lift, so even a quick tap shows which key it typed. (A key that
        // pages the keyboard doesn't: its index means another key now.)
        let typed_char = with_state_mut(|s| {
            let is_char = s
                .last_painted_keys
                .get(end)
                .is_some_and(|k| matches!(k.action, key::KeyAction::Char(_)));
            if is_char {
                s.linger = Some((end, crate::clock::now() + PRESS_LINGER));
            }
            is_char
        });
        if typed_char {
            crate::animation::request_draw_after(PRESS_LINGER);
        }
    }
    crate::animation::request_draw();
    true
}

/// Let go of the pressed key without typing it (the platform cancelled the
/// touch, or a second finger cancelled it): closes the alternates, stops a
/// repeat and cancels a caret gesture.
pub fn cancel_press() {
    with_state_mut(|s| {
        s.pressed_key_index = None;
        s.captured_pointer = false;
        s.key_repeat = None;
        s.hold = None;
        s.alternates = None;
        cursor_slide::cancel(s);
    });
    crate::animation::request_draw();
}

/// Whether a second finger landing now should cancel the press under the
/// first (a Backspace slide or the iPhone trackpad under way) rather than
/// finish it with a lift.
pub fn overlap_cancels_press() -> bool {
    with_state_ref(cursor_slide::overlap_cancels)
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
    // Any key but the microphone ends a dictation first, its shown tail
    // committed, as on Gboard: typed keys never mix into the composition.
    if !matches!(
        painted.action,
        key::KeyAction::Voice | key::KeyAction::Inert
    ) {
        super::voice_session::end_keeping_tail();
    }
    // Clear any pending shift-double-tap detection on a non-shift commit
    // so a Shift tap that's *not* immediately followed by another Shift
    // tap doesn't accidentally promote to caps-lock when the user later
    // taps Shift unrelated.
    let is_shift_action = matches!(painted.action, key::KeyAction::Switch(Layer::Shifted));
    if !is_shift_action {
        with_state_mut(|s| s.last_shift_tap = None);
    }
    match painted.action {
        key::KeyAction::Char(c) => type_char(c, modifiers),
        key::KeyAction::Backspace => push_synthetic_key(Key::Backspace, modifiers),
        key::KeyAction::Enter => {
            push_synthetic_key(Key::Enter, modifiers);
        }
        key::KeyAction::Space => push_synthetic_key(Key::Char(' '), modifiers),
        key::KeyAction::Switch(target) => {
            handle_layer_switch(target);
        }
        key::KeyAction::Dismiss => dismiss(),
        key::KeyAction::Voice => super::voice_session::toggle(),
        key::KeyAction::Inert => {}
    }
    crate::animation::request_draw();
}

/// Type `c` (a key's character or one of its alternates) with Shift held
/// while the keyboard is shifted, then drop a one-shot Shift.
fn type_char(c: char, modifiers: Modifiers) {
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
    crate::animation::request_draw();
}

/// Open the held key's alternates once it has been held for
/// [`LONG_PRESS_DELAY`]. Called once per paint (before the keys are laid
/// out again, so the held key's face is the last painted one); the press
/// armed a wake-up for the moment the popup is due. `width` is the
/// viewport's, which the popup stays inside.
pub(super) fn tick_long_press(style: &Style, width: f64) {
    let family = family();
    let opened = with_state_mut(|s| {
        let hold = s.hold?;
        if !s.captured_pointer || s.pressed_key_index != Some(hold.key_index) {
            s.hold = None;
            return None;
        }
        if crate::clock::since(hold.pressed_at) < LONG_PRESS_DELAY {
            return None;
        }
        s.hold = None;
        let painted = s.last_painted_keys.get(hold.key_index)?;
        let key::KeyAction::Char(c) = painted.action else {
            return None;
        };
        let alternates = alternates_for(c, family)?;
        s.alternates = Some(AlternatesPopup::open(
            hold.key_index,
            painted.rect,
            alternates,
            (style.key_h_gap, style.key_v_gap),
            // The popup's frame reaches a little past its row.
            8.0,
            width - 8.0,
        ));
        Some(())
    });
    if opened.is_some() {
        crate::animation::request_draw();
    }
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

/// Advance the hold-to-repeat state machine. Called once per frame (at the
/// start of `App::layout`, through [`super::tick_held_keys`]) so
/// the cadence rides on the animation loop, which [`super::needs_draw`]
/// keeps running while a repeat is held. When the held key has been down
/// long enough we synthesize a `Backspace`.
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
        // The field changed: draw it. (Frames keep coming between repeats
        // because `needs_draw` is true while a repeat is held, not because
        // of this request, which the next paint clears as it starts.)
        crate::animation::request_draw();
    }
}
