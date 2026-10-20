//! A haptic tick when a long press fires: agg-gui turns a held finger into
//! a right click (`agg_gui::touch_emulation::long_press`) and calls its
//! listeners; this one asks the browser to vibrate for 10 ms, the short
//! click a phone gives when a context menu opens.
//!
//! `navigator.vibrate` is missing on some browsers (iOS Safari), so it is
//! looked up and called as a plain JS function: where it is absent nothing
//! happens, and an exception it throws is dropped rather than trapping the
//! wasm module.

use std::cell::Cell;

use wasm_bindgen::{JsCast, JsValue};

/// How long the tick vibrates, in milliseconds.
const TICK_MS: u32 = 10;

thread_local! {
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
}

/// Register the vibrate listener once per page.
pub(super) fn install() {
    if INSTALLED.with(|i| i.replace(true)) {
        return;
    }
    agg_gui::on_long_press(|_| vibrate(TICK_MS));
}

fn vibrate(ms: u32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let navigator = window.navigator();
    let Ok(function) = js_sys::Reflect::get(&navigator, &JsValue::from_str("vibrate")) else {
        return;
    };
    if let Some(function) = function.dyn_ref::<js_sys::Function>() {
        // The result (whether the browser vibrated) changes nothing here: a
        // page without permission or a device without a motor stays silent.
        let _ = function.call1(&navigator, &JsValue::from(ms));
    }
}
