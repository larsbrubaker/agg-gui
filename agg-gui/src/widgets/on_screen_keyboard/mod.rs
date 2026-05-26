//! # On-screen software keyboard
//!
//! agg-gui's own touch-input keyboard. Replaces the native iOS / Android
//! soft keyboard with one we control end-to-end so the user gets:
//!
//! - a consistent visual that matches the rest of the agg-gui app (no
//!   browser-chrome reflow, no surprise auto-correct rules, no native
//!   keyboard hiding the focused field at random),
//! - taps that synthesize the same [`Event::KeyDown`] events a physical
//!   keyboard would produce (so [`TextField`](crate::widgets::TextField),
//!   [`TextArea`](crate::widgets::TextArea), and any future text-bearing
//!   widget Just Works),
//! - per-OS chrome (iOS / Android / generic) that approximates the
//!   user's muscle memory.
//!
//! ## Architecture (follows the combo-popup pattern)
//!
//! - The keyboard is **not** a child widget in the tree. It lives in
//!   module-level thread-local state and is painted by [`App::paint`]
//!   after every other global overlay so it always sits on top.
//! - Mouse / touch events pass through
//!   [`handle_software_keyboard_mouse_down`] /
//!   [`handle_software_keyboard_mouse_move`] /
//!   [`handle_software_keyboard_mouse_up`] *before* the normal hit-test
//!   path; the keyboard either consumes them (a key tap) or returns
//!   `false` so they continue to the widget tree.
//! - Key taps push synthesized `(Key, Modifiers)` pairs into a queue;
//!   [`App`] drains the queue after each event handler and dispatches
//!   them through the normal [`App::on_key_down`] code path. The
//!   focused [`TextField`] receives `KeyDown { Key::Char('a') }` exactly
//!   like a physical key press.
//! - Show / hide is driven by the focused widget — when the App's focus
//!   changes to a widget whose [`Widget::accepts_text_input`] returns
//!   `true`, the keyboard slides up. Losing focus slides it down.
//! - The chrome style follows [`crate::input_profile::current_input_profile`]
//!   so an iPad and a Pixel see different keyboards from the same Rust
//!   binary.
//!
//! ## Scope of this first cut
//!
//! - Single US-QWERTY letter layout + a numbers / symbols layer.
//! - Tap-to-type (no long-press, no hold-to-repeat, no predictive bar
//!   yet — the module is structured to grow into those without a
//!   rewrite).
//! - Layout-driven painting via [`layouts::Layout`] so adding a new
//!   layer or layout is a data change, not a code change.

use crate::draw_ctx::DrawCtx;
use crate::event::{Key, Modifiers, MouseButton};
use crate::geometry::{Point, Rect};
use crate::input_profile::current_input_profile;

pub mod events;
pub mod key;
pub mod layouts;
pub mod state;
pub mod style;

use events::push_synthetic_key;
use layouts::{Layer, Layout};
use state::{with_state_mut, with_state_ref};
use style::Style;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// What kind of input the focused widget wants from the on-screen
/// keyboard.  Drives the initial layer the keyboard slides up into so
/// numeric fields see the digit pad instead of the letter row — same
/// hint browsers and native OSes derive from `<input type="number">` /
/// `UIKeyboardType.numberPad`.
///
/// Independent of input-validation: a field set to [`Numeric`] still
/// receives whatever the user actually types (the keyboard's mode-switch
/// keys remain available).  Pair with [`crate::widgets::TextField::with_char_filter`]
/// if you also want to reject non-digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardInputMode {
    /// Regular text — opens the letter layer (or Shifted if the
    /// auto-cap heuristic fires).  The historical default.
    #[default]
    Text,
    /// Numbers + common punctuation — opens directly into
    /// [`KeyboardLayer::Numbers`] so the user can start typing digits
    /// without tapping the `123` mode switch first.
    Numeric,
}

/// Enable / disable the on-screen keyboard globally. Disabled keyboards
/// never paint or capture events. The platform shell calls this once at
/// startup; defaults to `false` so apps that haven't opted in (or
/// desktop builds) see no behavior change.
///
/// Recommended pattern in a platform shell:
/// ```ignore
/// let profile = input_profile_from_hint(&user_agent, pointer_coarse);
/// set_input_profile(profile);
/// set_enabled(profile.is_mobile_touch());
/// ```
pub fn set_enabled(on: bool) {
    with_state_mut(|s| s.enabled = on);
}

/// Read the global enabled flag.
pub fn is_enabled() -> bool {
    with_state_ref(|s| s.enabled)
}

/// Whether the keyboard is currently visible (visible-fraction > 0 in the
/// slide animation). The host shell uses this to (a) skip the native
/// keyboard hack, and (b) potentially reserve safe-area space.
pub fn is_visible() -> bool {
    with_state_ref(|s| s.visible_fraction() > 0.001)
}

/// Top edge of the keyboard panel in viewport coordinates (Y-up). When
/// the keyboard is hidden this returns the viewport bottom (i.e. zero
/// keyboard intrusion). Useful for the App layout to shrink the safe
/// area so the focused widget doesn't sit under the keyboard.
pub fn occluded_height(viewport_height: f64) -> f64 {
    with_state_ref(|s| {
        if !s.enabled {
            return 0.0;
        }
        let target_h = s.last_panel_height.unwrap_or(0.0);
        target_h * s.visible_fraction()
    })
    .min(viewport_height)
}

/// Height the keyboard panel WILL occupy when fully open, regardless
/// of the current slide-animation state.  Returned in logical pixels
/// (Y-up); the panel sits at the bottom of the viewport so its top
/// edge lies at `y = target_panel_height(...)`.
///
/// Computed deterministically from the active input profile + layer,
/// so callers (notably the keyboard-aware focus auto-scroll) get a
/// useful answer on the very first focus event — *before* the panel
/// has ever painted.  Falls back to the most-recent painted height
/// when the layout subsystem isn't ready (no font / no profile);
/// returns `0.0` when the keyboard is disabled, so call sites need
/// no extra `is_enabled` check.
pub fn target_panel_height(viewport_width: f64) -> f64 {
    with_state_ref(|s| {
        if !s.enabled {
            return 0.0;
        }
        let style = Style::for_profile(current_input_profile());
        let layer = s.current_layer;
        let layout = Layout::for_layer(layer);
        let computed = layout.compute_panel_height(viewport_width, &style);
        // Fall back to the last painted height in the (theoretical)
        // case where the layout function returns a degenerate 0 —
        // keeps the auto-scroll robust even if a future profile ships
        // an empty layout by mistake.
        if computed > 0.0 {
            computed
        } else {
            s.last_panel_height.unwrap_or(0.0)
        }
    })
}

/// Called by [`App`](crate::widget::App) when the focused widget changes.
/// Causes the keyboard to slide up / down by retargeting the slide tween.
///
/// `existing_text` lets the keyboard apply the iOS-style auto-capitalize
/// heuristic: if the field is empty when it gains focus, the first
/// letter row starts in [`Layer::Shifted`] so the user's first tap
/// produces an upper-case letter. After that initial tap the layer
/// reverts to lowercase (one-shot shift), matching what every mobile
/// OS does for sentence-start capitalization. `None` (no value
/// available) is treated as "don't change the layer".
///
/// `mode` lets the focused widget opt into the numeric layer — e.g.
/// a quantity field that wants the digit pad up first.  When
/// [`KeyboardInputMode::Numeric`] is passed the auto-cap heuristic is
/// skipped and the keyboard opens on [`Layer::Numbers`].
pub fn set_text_input_focused(
    focused: bool,
    existing_text: Option<&str>,
    mode: KeyboardInputMode,
) {
    with_state_mut(|s| {
        if !s.enabled {
            return;
        }
        s.text_input_focused = focused;
        let target = if focused { 1.0 } else { 0.0 };
        s.slide.set_target(target);
        if focused {
            match mode {
                KeyboardInputMode::Numeric => {
                    // Numeric fields skip the sentence-start heuristic;
                    // open directly on the digit pad. Caps-lock is also
                    // reset so a leftover shift toggle from a previous
                    // letter-mode field doesn't carry into the digits.
                    s.current_layer = Layer::Numbers;
                    s.caps_lock = false;
                    s.last_shift_tap = None;
                }
                KeyboardInputMode::Text => {
                    if let Some(text) = existing_text {
                        let last_non_space = text.trim_end().chars().last();
                        let sentence_start = match last_non_space {
                            None => true, // empty
                            Some(c) if c == '.' || c == '!' || c == '?' || c == '\n' => true,
                            _ => false,
                        };
                        s.current_layer = if sentence_start {
                            Layer::Shifted
                        } else {
                            Layer::Letters
                        };
                    }
                }
            }
        }
        crate::animation::request_draw();
    });
}

/// Programmatic dismiss — used by the keyboard's close key, and by
/// host code that wants to hide the keyboard.
///
/// Sets a one-shot `dismiss_requested` flag the App drains every
/// event loop iteration via [`take_dismiss_request`] / `App::drain_keyboard_events`,
/// which clears focus on the previously-focused text field.  That
/// `FocusLost` is what retargets the keyboard-aware lift back to 0
/// so the tree slides down alongside the keyboard panel — without
/// it the panel falls but the lifted tree stays parked above an
/// empty band where the keyboard used to sit.
pub fn dismiss() {
    with_state_mut(|s| {
        s.text_input_focused = false;
        s.slide.set_target(0.0);
        s.dismiss_requested = true;
        crate::animation::request_draw();
    });
}

/// Atomically read-and-clear the dismiss-request flag set by
/// [`dismiss`].  Called once per event loop iteration by the App so
/// the focused text field gets a `FocusLost` and the screen-lift
/// tween retargets back to 0.  Returns `true` if a dismiss was pending.
pub fn take_dismiss_request() -> bool {
    with_state_mut(|s| {
        let pending = s.dismiss_requested;
        s.dismiss_requested = false;
        pending
    })
}

/// `true` if the keyboard wants another frame this paint cycle (slide
/// animation in flight, or a hold-to-repeat key is active). [`App::wants_draw`]
/// consults this so the rAF / event loop keeps pumping while the
/// keyboard has work to do.
pub fn needs_draw() -> bool {
    with_state_ref(|s| s.slide.is_animating() || s.key_repeat.is_some())
}

// ---------------------------------------------------------------------------
// Paint
// ---------------------------------------------------------------------------

/// Paint the keyboard panel and its keys. Called by [`App::paint`] last,
/// after all other global-overlay drains, so the keyboard always sits on
/// top of normal content, combo popups, tooltips, and modal overlays.
///
/// `viewport` is the logical (pre-`device_scale`) viewport size — the
/// caller is responsible for any `ctx.scale(device_scale, …)` save/restore
/// wrap (mirrors how combo popups are drained).
pub fn paint_software_keyboard(ctx: &mut dyn DrawCtx, viewport: crate::geometry::Size) {
    // Advance the hold-to-repeat state machine first so it has a chance
    // to fire before the next paint reuses cached key positions.
    tick_key_repeat();

    let visible_fraction = with_state_mut(|s| s.slide.tick());
    if visible_fraction <= 0.001 {
        // Hidden — also clear cached key hit-rects so a stale layout
        // doesn't leak into the next show cycle.
        with_state_mut(|s| s.last_painted_keys.clear());
        return;
    }

    let style = Style::for_profile(current_input_profile());
    let layer = with_state_ref(|s| s.current_layer);
    let layout = Layout::for_layer(layer);

    // Compute panel rect. The fully-extended height is determined by the
    // layout (rows + paddings); we then slide it up from off-screen by
    // (1 - visible_fraction) * height.
    let panel_height = layout.compute_panel_height(viewport.width, &style);
    let panel_width = viewport.width;
    with_state_mut(|s| s.last_panel_height = Some(panel_height));

    // Y-up coordinates: panel bottom edge sits at `bottom_y`, panel
    // ranges [bottom_y, bottom_y + panel_height].
    let hidden_offset = panel_height * (1.0 - visible_fraction);
    let bottom_y = -hidden_offset;
    let panel = Rect::new(0.0, bottom_y, panel_width, panel_height);

    paint_panel_background(ctx, panel, &style);

    // Lay out + paint keys, caching their hit rects for tap dispatch.
    let painted_keys = layout.paint(ctx, panel, &style, layer);
    with_state_mut(|s| s.last_painted_keys = painted_keys);
}

fn paint_panel_background(ctx: &mut dyn DrawCtx, panel: Rect, style: &Style) {
    ctx.set_fill_color(style.panel_bg);
    ctx.begin_path();
    ctx.rect(panel.x, panel.y, panel.width, panel.height);
    ctx.fill();

    // Top accent line so the keyboard reads as a distinct surface from
    // whatever the app is painting behind it.
    ctx.set_stroke_color(style.panel_top_border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    let top_y = panel.y + panel.height;
    ctx.move_to(panel.x, top_y);
    ctx.line_to(panel.x + panel.width, top_y);
    ctx.stroke();
}

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
                    pressed_at: web_time::Instant::now(),
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
pub fn handle_software_keyboard_mouse_move(pos: Point) -> bool {
    let (captured, _) = with_state_ref(|s| (s.captured_pointer, s.pressed_key_index));
    if !captured {
        return false;
    }
    // Track hover for visual feedback on a drag inside the keyboard.
    let new_hit = find_key_at(pos);
    with_state_mut(|s| {
        if s.pressed_key_index != new_hit {
            s.pressed_key_index = new_hit;
            crate::animation::request_draw();
        }
    });
    true
}

/// Handle a pointer-up. If the release lands on the same key as the
/// press, that key fires (`push_synthetic_key`).
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
    if let (Some(start), Some(end)) = (pressed_idx, final_hit) {
        // Suppress the tap commit if hold-to-repeat already fired at
        // least once during the press — otherwise the release would
        // synthesize one extra Backspace after the user lifted.
        if start == end && !repeat_already_fired {
            commit_key_press(end, modifiers);
        }
    }
    crate::animation::request_draw();
    true
}

fn find_key_at(pos: Point) -> Option<usize> {
    with_state_ref(|s| {
        s.last_painted_keys
            .iter()
            .enumerate()
            .find(|(_, k)| k.rect.contains(pos))
            .map(|(i, _)| i)
    })
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
/// - Any other layer switch (123 / ABC / #+=) just changes the layer
///   and clears caps-lock state.
fn handle_layer_switch(target: Layer) {
    if target == Layer::Shifted || target == Layer::Letters {
        with_state_mut(|s| {
            let now = web_time::Instant::now();
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
fn tick_key_repeat() {
    let now = web_time::Instant::now();
    let action = with_state_mut(|s| {
        let Some(repeat) = s.key_repeat.as_mut() else {
            return None;
        };
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
        match action {
            key::KeyAction::Backspace => {
                push_synthetic_key(Key::Backspace, Modifiers::default());
            }
            _ => {}
        }
        // Keep the loop hot for the next tick.
        crate::animation::request_draw();
    }
}

// ---------------------------------------------------------------------------
// Synthetic key drain (called from App after each pointer event)
// ---------------------------------------------------------------------------

pub use events::drain_synthetic_keys;

// Re-export common types for ergonomics.
pub use key::{KeyAction, KeyCap};
pub use layouts::Layer as KeyboardLayer;

// ---------------------------------------------------------------------------
// Internal — invoked by App through `crate::widgets::on_screen_keyboard::test_hook`
// in tests only.
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod test_hook {
    use super::*;
    use crate::animation::Tween;
    use state::KeyboardState;

    #[allow(dead_code)]
    pub fn force_layer(layer: Layer) {
        with_state_mut(|s| s.current_layer = layer);
    }

    pub fn force_visible() {
        with_state_mut(|s| {
            s.enabled = true;
            s.text_input_focused = true;
            s.slide = Tween::new(1.0, 0.0);
            s.last_panel_height = Some(240.0);
        });
    }

    pub fn reset() {
        with_state_mut(|s| {
            *s = KeyboardState::default();
        });
    }

    /// Re-export `handle_layer_switch` so caps-lock behaviour can be
    /// exercised from cross-module tests without first synthesising a
    /// full paint pass.
    pub fn simulate_shift_tap() {
        super::handle_layer_switch(super::Layer::Shifted);
    }

    /// Read caps-lock state without exposing the full module state.
    pub fn caps_lock() -> bool {
        with_state_ref(|s| s.caps_lock)
    }

    /// Read current layer for tests.
    pub fn current_layer() -> Layer {
        with_state_ref(|s| s.current_layer)
    }
}
