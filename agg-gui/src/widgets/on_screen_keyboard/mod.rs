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
//! ## Why agg-gui draws its own keyboard (a settled decision)
//!
//! agg-gui types on touch screens with this keyboard, not the device's native
//! soft keyboard, and will keep doing so. This is not a stopgap: the keyboard
//! is a product surface we own and keep improving until it is as good as the
//! one on the user's phone. Don't reopen the native route without reading this.
//!
//! agg-gui draws its whole UI into one canvas. A browser can't open its
//! keyboard for a canvas, so every canvas-drawn framework fakes a focused DOM
//! text element and then has to keep its layout in step with a keyboard whose
//! size and timing the browser reports badly. That is a well-documented,
//! unsolved problem, not something we failed to figure out:
//!
//! - MatterCAD spent hours on the native keyboard (2026): the app couldn't tell
//!   how high to lift its layout, and the page's JavaScript and the app
//!   disagreed about when the keyboard rose and fell.
//! - egui/eframe uses that hidden-input trick; its maintainer's own tracking
//!   issue says "Text editing on mobile web is pretty bad in eframe" (the
//!   keyboard covering the field, janky focus, Backspace failing, the keyboard
//!   not always appearing on iOS): <https://github.com/emilk/egui/issues/4500>.
//! - Slint's WASM input never raised the mobile keyboard
//!   (<https://github.com/slint-ui/slint/issues/215>); Iced on native Android
//!   gets raw key events only, so no composition, autocorrect or swipe.
//! - Avalonia's WASM keyboard is broken on Android browsers; a maintainer
//!   called the hidden-input approach a hack that "will never work properly"
//!   (<https://github.com/AvaloniaUI/Avalonia/issues/11665>).
//! - Flutter web, the most mature canvas-drawn framework, also uses a hidden
//!   input and still has the keyboard covering the field on Safari
//!   (<https://github.com/flutter/flutter/issues/42211>).
//! - The browser APIs meant to fix this don't yet: Chrome's VirtualKeyboard API
//!   overshoots the keyboard's height while it animates and reports its
//!   rectangle in the wrong coordinates (only `keyboard-inset-height` is right),
//!   and Safari and Firefox lack it
//!   (<https://zouhir.org/blog/virtual-keyboard-api/>); EditContext, the proper
//!   API for canvas editors, is experimental and Chromium-only.
//!
//! Owning the keyboard removes that whole class of problems: its height is
//! ours, so the app always lays out exactly above it; it opens and closes when
//! we say; it never covers the field; it behaves the same in every browser and
//! natively.
//!
//! ### The bar: as good as the phone's own keyboard
//!
//! References: `docs/Android Keyboard/` (Gboard). The keyboard must
//! - leave no dead space: every point belongs to the nearest key, so a touch
//!   between keys types the closest one (met: `hit.rs` tiles the panel with
//!   the keys' hit areas, and a sliding finger types the key it lifts on);
//! - look and feel like the platform keyboard: key shapes, gaps, proportions,
//!   the middle row's half-key offset, the bottom row, light and dark colours
//!   (met for Android, measured off a Gboard screenshot: one key pitch per
//!   width unit, 15 dp margins, 45 dp keys, the hide chevron below the keys;
//!   met for the iPhone (`layouts_ios.rs`): no number row, 3 pt margins,
//!   33 x 42 pt keys on a 54 pt row pitch, white letter keys and grey
//!   function keys with a 1 pt shadow on the grey tray, Shift lit white
//!   while upper case is on, `123 · space · return`, the emoji key below the
//!   keys, and the pressed key growing into a balloon; open: Gboard's symbol
//!   pages have one row fewer than its letters, so its height changes with
//!   the page, where Gboard's stays put);
//! - show letters in the case Shift will type, with Shift and Caps Lock as on
//!   the phone, and label Enter with what it does (Done, Search, Next, Go)
//!   (met: a field says what Enter does and whether it holds sentences with
//!   [`TextInputHints`]; one-line fields open in lower case; Gboard draws the
//!   action as an icon, the iPhone as a word on a blue key for done, search,
//!   go and send);
//! - offer the long-press alternates (accents, digits) and the symbol, number
//!   and emoji pages (met for alternates, `alternates.rs`: Gboard's accent
//!   sets, a top-row letter's digit where there is no number row, currency,
//!   dashes and curly quotes, in each platform's popup style; open: the emoji
//!   page. The iPhone's emoji key opens the symbols page until it exists,
//!   so the key the user expects is there and does something useful);
//! - let the user put the keyboard away (Gboard: the chevron below the keys;
//!   the iPhone has no hide key, and as there a tap outside the field takes
//!   its focus and the keyboard with it);
//! - add no latency, repeat a held Backspace, and never lose a tap to a drag or
//!   a long press;
//! - tell the app its exact height the moment it changes, so the app lays out
//!   above it and keeps the focused field in view.
//!
//! Swipe (glide) typing needs a word-prediction engine and per-language
//! dictionaries; it is the one thing the phone's keyboard has that this one
//! doesn't yet, and if it is built, it belongs here.
//!
//! Look again only if the platform changes: EditContext (or an equivalent) in
//! Safari and Chrome on Android and iOS, with keyboard geometry reported
//! correctly while it animates. If the native route is ever retried, start from
//! Flutter's pattern (a hidden input over the field, the settled visible-area
//! height) and test on real phones first.
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
//! ## Scope today
//!
//! - Gboard-style US-QWERTY pages (see `docs/Android Keyboard/`):
//!   letters with a number row, `?123` symbols, `=\<` extended
//!   symbols, and a numeric pad for numeric fields; on an iPhone the iOS
//!   pages (`layouts_ios.rs`), picked per input profile
//!   ([`layouts::Layout::for_profile`]).
//! - Tap and slide to type, a preview over the pressed letter, long-press
//!   alternates, hold-to-repeat Backspace, Shift / Caps Lock, and an Enter
//!   key drawn as the field's action. No predictive bar yet.
//! - Layout-driven painting via [`layouts::Layout`] so adding a new
//!   layer or layout is a data change, not a code change.
//! - Voice input: with a speech-to-text provider registered
//!   ([`set_voice_input`]), a microphone key where the platform keyboard has
//!   one (Gboard's toolbar row, the iPhone's strip below the keys) takes
//!   dictation into the focused field, committed words inserted and the
//!   unconfirmed tail shown as composition text. The interface, its states
//!   and how a Web Worker or native recorder plugs in are in [`voice`].

use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;
use crate::input_profile::current_input_profile;

pub mod alternates;
mod alternates_paint;
pub mod enter_action;
pub mod events;
mod hit;
pub mod key;
mod layout_paint;
pub mod layouts;
mod layouts_ios;
mod paint;
mod pointer;
pub mod state;
pub mod style;
pub mod voice;
mod voice_paint;
pub(crate) mod voice_session;

pub use enter_action::{EnterAction, TextInputHints};
use layouts::{Layer, Layout};
use pointer::tick_key_repeat;
pub use pointer::{
    cancel_press, contains_point, handle_software_keyboard_mouse_down,
    handle_software_keyboard_mouse_move, handle_software_keyboard_mouse_up,
};
use state::{with_state_mut, with_state_ref};
use style::Style;
pub use voice::{
    cancel_voice_input, has_voice_input, is_dictating, set_voice_input, voice_state, VoiceContext,
    VoiceInput, VoicePoll, VoiceState, VoiceText,
};

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
    /// Digits + arithmetic — opens directly into
    /// [`KeyboardLayer::NumPad`] so the user can start typing digits
    /// without tapping the `?123` mode switch first.
    Numeric,
}

/// Enable / disable the on-screen keyboard globally. Disabled keyboards
/// never paint or capture events. The platform shell calls this once at
/// startup; defaults to `false` so apps that haven't opted in (or
/// desktop builds) see no behavior change.
///
/// The key labels are drawn in the app's UI font, the one installed with
/// [`crate::font_settings::set_system_font`].
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
        let layout = Layout::for_profile(layer, current_input_profile());
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

/// Lay the tree out in the room above the keyboard while it is up, instead
/// of lifting the whole tree to show the focused field (the default).
///
/// With this on, [`App::layout`](crate::widget::App::layout) gives the root
/// the viewport less [`layout_room`]: the root's bottom edge sits on the
/// keyboard's top, so a bottom-docked panel rests on the keyboard and the
/// rest of the layout shrinks to fit, and the root's top stays on screen.
/// The focused field's innermost scrolling ancestor then scrolls the field
/// into view (`widget/keyboard_resize.rs`). When the keyboard goes away the
/// layout returns to the whole viewport. Off by default.
pub fn set_resizes_layout(on: bool) {
    with_state_mut(|s| s.resizes_layout = on);
    crate::animation::request_layout();
}

/// Whether [`set_resizes_layout`] is on.
pub fn resizes_layout() -> bool {
    with_state_ref(|s| s.resizes_layout)
}

/// The height the keyboard takes from the bottom of a `viewport` (logical)
/// layout: its full panel height while it is up or sliding up, with the
/// keyboard enabled and [`set_resizes_layout`] on; `0.0` otherwise. The
/// layout takes the whole room at once rather than following the slide, so
/// the tree is laid out once per show and once per hide.
pub fn layout_room(viewport: crate::geometry::Size) -> f64 {
    let up = with_state_ref(|s| s.enabled && s.resizes_layout && s.slide.target() > 0.5);
    if !up {
        return 0.0;
    }
    target_panel_height(viewport.width).clamp(0.0, viewport.height.max(0.0))
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
/// skipped and the keyboard opens on [`Layer::NumPad`].
pub fn set_text_input_focused(focused: bool, existing_text: Option<&str>, mode: KeyboardInputMode) {
    set_text_input_focused_with_hints(focused, existing_text, mode, TextInputHints::default());
}

/// [`set_text_input_focused`] with the focused field's [`TextInputHints`]:
/// what its Enter key does (the key's icon) and whether it starts sentences
/// with a capital. With `hints.auto_capitalize` off the letters always open
/// in lower case. The App calls this on every focus change with the
/// widget's [`Widget::text_input_hints`](crate::widget::Widget::text_input_hints).
pub fn set_text_input_focused_with_hints(
    focused: bool,
    existing_text: Option<&str>,
    mode: KeyboardInputMode,
    hints: TextInputHints,
) {
    let opened = with_state_mut(|s| {
        if !s.enabled {
            return false;
        }
        let opened = focused && !s.text_input_focused;
        s.text_input_focused = focused;
        if focused {
            s.hints = hints;
        }
        let target = if focused { 1.0 } else { 0.0 };
        s.slide.set_target(target);
        if focused {
            match mode {
                KeyboardInputMode::Numeric => {
                    // Numeric fields skip the sentence-start heuristic;
                    // open directly on the digit pad. Caps-lock is also
                    // reset so a leftover shift toggle from a previous
                    // letter-mode field doesn't carry into the digits.
                    s.current_layer = Layer::NumPad;
                    s.caps_lock = false;
                    s.last_shift_tap = None;
                }
                KeyboardInputMode::Text if !hints.auto_capitalize => {
                    // A field that doesn't hold sentences opens in lower
                    // case, as the phone does for names and searches.
                    if !s.caps_lock {
                        s.current_layer = Layer::Letters;
                    }
                }
                KeyboardInputMode::Text => {
                    if let Some(text) = existing_text {
                        s.current_layer = if is_sentence_start(text) {
                            Layer::Shifted
                        } else {
                            Layer::Letters
                        };
                    }
                }
            }
        }
        crate::animation::request_draw();
        opened
    });
    if opened {
        // The keyboard is coming up: a voice provider starts getting ready
        // (downloading its model) now, not at the first tap on the mic.
        voice_session::keyboard_opened();
    }
}

/// Whether text ending in `text` (the field's text, or its text before the
/// caret) leaves the next word starting a sentence: nothing but whitespace,
/// or, past any trailing whitespace other than line breaks (spaces, tabs, a
/// no-break or ideographic space), `.`, `!`, `?` or a line break (`\n`,
/// `\r\n` or `\r`). The rule behind the keyboard opening with Shift on,
/// and behind [`VoiceContext::sentence_start`].
pub(crate) fn is_sentence_start(text: &str) -> bool {
    let line_break = |c: char| c == '\n' || c == '\r';
    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace() && !line_break(c));
    match trimmed.chars().last() {
        None => true,
        Some(c) => matches!(c, '.' | '!' | '?') || line_break(c),
    }
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
/// animation in flight, a hold-to-repeat key is active, or a dictation is
/// running). [`App::wants_draw`]
/// consults this so the rAF / event loop keeps pumping while the
/// keyboard has work to do.
pub fn needs_draw() -> bool {
    with_state_ref(|s| s.slide.is_animating() || s.key_repeat.is_some())
        || voice_session::wants_frames()
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

    let profile = current_input_profile();
    let style = Style::for_profile(profile);
    // A key held long enough opens its alternates before the keys are laid
    // out again.
    pointer::tick_long_press(&style, viewport.width);
    pointer::rearm_timed_wakes();
    let layer = with_state_ref(|s| s.current_layer);
    let layout = Layout::for_profile(layer, profile);
    let voice = voice_session::paint_view();
    let paint_state = with_state_ref(|s| layout_paint::PaintState {
        layer: s.current_layer,
        caps_lock: s.caps_lock,
        pressed: pointer::drawn_pressed(s),
        enter: s.hints.enter_action,
        family: layout.family,
        alternates: s.alternates.clone(),
        voice,
    });

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

    // The keyboard paints after the whole widget tree, and every widget
    // scopes the font it sets to its own save/restore, so the context
    // reaches this point with no font (a GPU frame starts with none) and
    // text drawn now would silently draw nothing. The key labels use the
    // app's UI font, the one installed with
    // `font_settings::set_system_font`; a host that installed none keeps
    // whatever font it left on the context. The save/restore keeps the
    // labels' font from leaking into anything painted later.
    ctx.save();
    if let Some(font) = crate::font_settings::current_system_font() {
        ctx.set_font(font);
    }

    paint_panel_background(ctx, panel, &style);

    // Lay out + paint keys, caching their hit rects for tap dispatch.
    let painted_keys = layout.paint(ctx, panel, &style, &paint_state);
    ctx.restore();
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
// Synthetic key drain (called from App after each pointer event)
// ---------------------------------------------------------------------------

pub use events::drain_synthetic_keys;

// Re-export common types for ergonomics.
pub use key::{KeyAction, KeyCap, KeyGlyph};
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
        super::voice_session::reset();
    }

    /// Re-export `handle_layer_switch` so caps-lock behaviour can be
    /// exercised from cross-module tests without first synthesising a
    /// full paint pass.
    pub fn simulate_shift_tap() {
        super::pointer::handle_layer_switch(super::Layer::Shifted);
    }

    /// The dictation status line this frame shows (the voice keys' text).
    pub fn voice_status() -> Option<String> {
        super::voice_session::paint_view()
            .as_ref()
            .and_then(super::voice_paint::status_text)
    }

    /// Read caps-lock state without exposing the full module state.
    pub fn caps_lock() -> bool {
        with_state_ref(|s| s.caps_lock)
    }

    /// Read current layer for tests.
    pub fn current_layer() -> Layer {
        with_state_ref(|s| s.current_layer)
    }

    /// The painted key that owns `pos` (viewport, Y-up), by the same
    /// lookup a touch uses.
    pub fn key_at(pos: crate::geometry::Point) -> Option<usize> {
        super::pointer::find_key_at(pos)
    }

    /// The font size painted key `key`'s text label draws at, by the
    /// paint pass's own rule (a letter key for alphabetic characters).
    pub fn cap_font_size(key: &super::key::PaintedKey) -> Option<f64> {
        let super::key::KeyCap::Text(text) = &key.cap else {
            return None;
        };
        let kind = match key.action {
            super::key::KeyAction::Char(c) if c.is_alphanumeric() => {
                super::layouts::KeyKind::Letter
            }
            _ => super::layouts::KeyKind::Utility,
        };
        let style = Style::for_profile(current_input_profile());
        let mut fb = crate::Framebuffer::new(4, 4);
        let mut ctx = crate::GfxCtx::new(&mut fb);
        if let Some(font) = crate::font_settings::current_system_font() {
            ctx.set_font(font);
        }
        Some(super::paint::cap_font_size(
            &mut ctx, text, kind, key.rect, &style,
        ))
    }
}
