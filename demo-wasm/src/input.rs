//! Wasm-bindgen input bindings — mouse, touch, keyboard, DPR, draw-need
//! polling.  Split out of `lib.rs` so the crate root stays under the
//! 800-line guardrail.
//!
//! Every `#[wasm_bindgen] pub fn` here is callable from JS by name
//! (see `demo/src/app.ts`).  They translate browser events into the
//! agg-gui input vocabulary and forward them to the singleton
//! [`DEMO_APP`].  As a submodule of the crate root, this file accesses
//! the thread-local state directly via `use crate::*`.

use std::cell::{Cell, RefCell};

use agg_gui::{App, Key, Modifiers, MouseButton};
use wasm_bindgen::prelude::*;

use crate::{mark_dirty, DEMO_APP, MOUSE_BUTTONS_DOWN, NEEDS_DRAW, RUN_MODE};

thread_local! {
    /// Latches `true` the first time we observe `DEMO_APP` already
    /// borrowed, so the JS console gets exactly one warning instead of
    /// one per event.  The condition itself is a poisoned-borrow
    /// situation: a prior panic somewhere in `render()` or an event
    /// handler unwound through wasm-bindgen without dropping the
    /// `RefMut`, leaving the cell permanently borrowed.  The original
    /// panic is logged earlier in the same console session — we just
    /// stop the cascade so the user sees it.
    static APP_POISONED_LOGGED: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` with a mutable borrow of `DEMO_APP`'s `App`, but skip the
/// call entirely if the cell is already borrowed.
///
/// Without this guard a single panic during one event handler poisons
/// the `RefCell`, and every subsequent JS-driven event panics again
/// with `RefCell already borrowed` — burying the real cause under an
/// infinite cascade.  The first poisoned attempt logs a one-time
/// warning so the user can scroll up to find the original panic.
fn with_app_mut(f: impl FnOnce(&mut App)) {
    DEMO_APP.with(|cell: &RefCell<Option<App>>| match cell.try_borrow_mut() {
        Ok(mut borrow) => {
            if let Some(app) = borrow.as_mut() {
                f(app);
            }
        }
        Err(_) => {
            if !APP_POISONED_LOGGED.with(|c| c.replace(true)) {
                web_sys::console::warn_1(&JsValue::from_str(
                    "agg-gui: skipping input event — DEMO_APP RefCell is \
                     already borrowed.  An earlier panic above this line is \
                     the root cause; subsequent events are dropped to avoid \
                     a cascade panic.",
                ));
            }
        }
    });
}

#[wasm_bindgen]
pub fn set_device_pixel_ratio(dpr: f64) {
    agg_gui::set_device_scale(dpr.max(0.5));
    mark_dirty();
}

/// Hand the JS-side platform / `pointer: coarse` detection result to
/// agg-gui. Sets both `Platform` (so Ctrl/Cmd shortcut labels match)
/// and `InputProfile` (so the on-screen keyboard auto-activates on
/// touch devices). Idempotent — safe to call repeatedly if the host
/// shell wants to refresh the profile (e.g. on viewport rotation).
#[wasm_bindgen]
pub fn set_client_platform(name: &str, pointer_coarse: bool) {
    agg_gui::set_platform(agg_gui::platform_from_name(name));
    let profile = agg_gui::input_profile::input_profile_from_hint(name, pointer_coarse);
    agg_gui::input_profile::set_input_profile(profile);
    agg_gui::widgets::on_screen_keyboard::set_enabled(profile.is_mobile_touch());
    // Apply the recommended UX zoom only here — at the platform-shell
    // boundary where we genuinely know whether the user is on a real
    // mobile device. `set_input_profile` no longer auto-applies the
    // scale, so demos that flip the profile programmatically (the
    // mobile-keyboard demo's radio) don't accidentally resize the
    // whole UI.
    agg_gui::ux_scale::set_ux_scale(profile.recommended_ux_scale());
    mark_dirty();
}

/// Whether the on-screen keyboard wants to be visible right now. The JS
/// shell uses this to skip its old "focus the hidden HTML textarea"
/// workaround when the agg-gui keyboard has taken over.
#[wasm_bindgen]
pub fn software_keyboard_visible() -> bool {
    agg_gui::widgets::on_screen_keyboard::is_visible()
}

/// Whether the on-screen keyboard is enabled for this device class at all
/// (true on mobile-touch devices — see `set_client_platform`).
///
/// The JS shell gates its native-keyboard fallback on THIS rather than on
/// [`software_keyboard_visible`]: on a device the emulated keyboard owns,
/// the hidden HTML textarea must *never* be focused, because focusing it
/// is exactly what pops up the native OS keyboard the emulated one
/// replaces. Visibility can't be used for that gate — it tracks the
/// slide-up animation, which is still at fraction 0 at the instant a field
/// is tapped, so the fallback would race the native keyboard open for a
/// frame. `enabled` is a static device-class flag with no such race.
#[wasm_bindgen]
pub fn software_keyboard_enabled() -> bool {
    agg_gui::widgets::on_screen_keyboard::is_enabled()
}

#[wasm_bindgen]
pub fn on_mouse_move(x: f64, y: f64) {
    with_app_mut(|app| app.on_mouse_move(x, y));
    if let Some(window) = web_sys::window() {
        if let Some(doc) = window.document() {
            if let Some(el) = doc.get_element_by_id("canvas") {
                let style = agg_gui::web_adapter::cursor_style(agg_gui::current_cursor_icon());
                let _ = el.set_attribute("style", &style);
            }
        }
    }
}

#[wasm_bindgen]
pub fn on_mouse_down(x: f64, y: f64, button: u8) {
    MOUSE_BUTTONS_DOWN.set(MOUSE_BUTTONS_DOWN.get().saturating_add(1));
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    with_app_mut(|app| app.on_mouse_down(x, y, btn, Modifiers::default()));
}

#[wasm_bindgen]
pub fn on_mouse_up(x: f64, y: f64, button: u8) {
    MOUSE_BUTTONS_DOWN.set(MOUSE_BUTTONS_DOWN.get().saturating_sub(1));
    let btn = match button {
        0 => MouseButton::Left,
        1 => MouseButton::Middle,
        2 => MouseButton::Right,
        n => MouseButton::Other(n),
    };
    with_app_mut(|app| app.on_mouse_up(x, y, btn, Modifiers::default()));
}

#[wasm_bindgen]
pub fn on_mouse_wheel(x: f64, y: f64, delta_y: f64) {
    with_app_mut(|app| app.on_mouse_wheel(x, y, delta_y));
}

#[wasm_bindgen]
pub fn on_mouse_leave() {
    with_app_mut(|app| app.on_mouse_leave());
}

#[wasm_bindgen]
pub fn on_touch_start(id: u32, x: f64, y: f64, force: f64) {
    let f = if force > 0.0 {
        Some(force as f32)
    } else {
        None
    };
    with_app_mut(|app| {
        app.on_touch_start(
            agg_gui::TouchDeviceId(0),
            agg_gui::TouchId(id as u64),
            x,
            y,
            f,
        );
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_move(id: u32, x: f64, y: f64, force: f64) {
    let f = if force > 0.0 {
        Some(force as f32)
    } else {
        None
    };
    with_app_mut(|app| {
        app.on_touch_move(
            agg_gui::TouchDeviceId(0),
            agg_gui::TouchId(id as u64),
            x,
            y,
            f,
        );
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_end(id: u32) {
    with_app_mut(|app| {
        app.on_touch_end(agg_gui::TouchDeviceId(0), agg_gui::TouchId(id as u64));
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_touch_cancel(id: u32) {
    with_app_mut(|app| {
        app.on_touch_cancel(agg_gui::TouchDeviceId(0), agg_gui::TouchId(id as u64));
    });
    mark_dirty();
}

#[wasm_bindgen]
pub fn on_key_down(key_str: &str, shift: bool, ctrl: bool, alt: bool, meta: bool) {
    if let Some(key) = parse_js_key(key_str) {
        let mods = Modifiers {
            shift,
            ctrl,
            alt,
            meta,
        };
        with_app_mut(|app| app.on_key_down(key, mods));
    }
}

#[wasm_bindgen]
pub fn needs_draw() -> bool {
    let continuous = RUN_MODE.with(|c| {
        c.borrow()
            .as_ref()
            .map(|rc| rc.get() == demo_ui::RunMode::Continuous)
            .unwrap_or(false)
    });
    if continuous {
        return true;
    }
    if NEEDS_DRAW.with(|c| c.get()) {
        return true;
    }
    // `try_borrow` so a poisoned-borrow cascade (see `with_app_mut`)
    // can't turn polling into a fresh panic — return `false` so the
    // animation loop quiets down instead of looping forever on
    // `RuntimeError: unreachable`.
    let want = DEMO_APP.with(|c| {
        c.try_borrow()
            .ok()
            .and_then(|b| b.as_ref().map(|a| a.wants_draw()))
            .unwrap_or(false)
    });
    want
}

fn parse_js_key(key: &str) -> Option<Key> {
    agg_gui::web_adapter::key(key)
}
