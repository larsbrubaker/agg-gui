//! Device-sensor plumbing for [`crate::web_shell`]: tilt
//! (`deviceorientation`) and gamepad polling.
//!
//! Split out of the parent shell module so `web_shell.rs` stays inside
//! the project's 800-line file limit. Everything here feeds the
//! platform-agnostic `agg_gui::tilt` / `agg_gui::gamepad` state that
//! apps read, and asks the parent for a repaint via
//! [`super::mark_dirty`]. The parent calls into three entry points: the
//! rAF tick runs [`service_tilt_requests`] + [`poll_gamepads`] (and
//! reads [`screen_angle_degrees`] for its fullscreen orientation lock),
//! and the canvas `pointerdown` listener runs
//! [`service_tilt_permission_gesture`] so iOS sees its permission
//! prompt inside a genuine user gesture.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use super::mark_dirty;

thread_local! {
    /// Set when the app asked for tilt but the platform gates the
    /// sensor behind a permission prompt that must run inside a user
    /// gesture (iOS 13+). The next pointerdown services it.
    static TILT_PERMISSION_WAIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The display's rotation relative to the device's natural
/// orientation, in degrees — folds the sensor's device-frame readings
/// into screen space so apps never care how the phone is held.
pub(super) fn screen_angle_degrees() -> f64 {
    web_sys::window()
        .and_then(|w| w.screen().ok())
        .map(|s| s.orientation().angle().unwrap_or(0) as f64)
        .unwrap_or(0.0)
}

fn install_tilt_listener() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let cb = Closure::<dyn FnMut(web_sys::DeviceOrientationEvent)>::new(
        |e: web_sys::DeviceOrientationEvent| {
            let (Some(beta), Some(gamma)) = (e.beta(), e.gamma()) else {
                return;
            };
            // Rotate the device-frame (gamma = lean right, beta = lean
            // toward the user) into screen space by the display angle.
            let a = screen_angle_degrees().to_radians();
            let sx = gamma * a.cos() + beta * a.sin();
            let sy = -gamma * a.sin() + beta * a.cos();
            agg_gui::tilt::set_reading(sx, sy);
            mark_dirty();
        },
    );
    let _ =
        window.add_event_listener_with_callback("deviceorientation", cb.as_ref().unchecked_ref());
    cb.forget();
    agg_gui::tilt::set_enabled(true);
}

/// iOS 13+ puts `DeviceOrientationEvent.requestPermission` on the
/// constructor; its presence means the sensor is permission-gated.
fn device_orientation_needs_permission() -> bool {
    js_sys::Reflect::get(&js_sys::global(), &"DeviceOrientationEvent".into())
        .ok()
        .filter(|c| !c.is_undefined())
        .and_then(|c| js_sys::Reflect::get(&c, &"requestPermission".into()).ok())
        .map(|f| f.is_function())
        .unwrap_or(false)
}

/// rAF-side: consume an app tilt request — install immediately where
/// no permission is needed, otherwise arm the next-gesture prompt.
pub(super) fn service_tilt_requests() {
    if !agg_gui::tilt::take_enable_request() {
        return;
    }
    if device_orientation_needs_permission() {
        TILT_PERMISSION_WAIT.with(|c| c.set(true));
    } else {
        install_tilt_listener();
    }
}

/// Pointerdown-side: run the iOS permission prompt inside the user
/// gesture it demands, then install the listener on "granted".
pub(super) fn service_tilt_permission_gesture() {
    if !TILT_PERMISSION_WAIT.with(|c| c.get()) {
        return;
    }
    TILT_PERMISSION_WAIT.with(|c| c.set(false));
    let request = js_sys::Reflect::get(&js_sys::global(), &"DeviceOrientationEvent".into())
        .and_then(|ctor| js_sys::Reflect::get(&ctor, &"requestPermission".into()));
    let Ok(request) = request else {
        return;
    };
    let Ok(promise) = js_sys::Function::from(request).call0(&JsValue::UNDEFINED) else {
        return;
    };
    let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(v) = wasm_bindgen_futures::JsFuture::from(promise).await {
            if v.as_string().as_deref() == Some("granted") {
                install_tilt_listener();
            }
        }
    });
}

/// Poll the Web Gamepad API and publish the first live pad
/// (agg_gui::gamepad). Standard mapping: axes[0]/[1] = left stick
/// (y positive down — already screen convention), buttons by
/// position. Browsers expose pads only after a first button press.
pub(super) fn poll_gamepads() {
    use agg_gui::gamepad::{buttons, GamepadState};
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(pads) = window.navigator().get_gamepads() else {
        agg_gui::gamepad::set_state(None);
        return;
    };
    let pad = pads
        .iter()
        .find(|p| !p.is_null())
        .and_then(|p| p.dyn_into::<web_sys::Gamepad>().ok());
    let Some(pad) = pad else {
        agg_gui::gamepad::set_state(None);
        return;
    };
    let axes = pad.axes();
    let axis = |i: u32| axes.get(i).as_f64().unwrap_or(0.0);
    let btns = pad.buttons();
    let down = |i: u32| {
        btns.get(i)
            .dyn_into::<web_sys::GamepadButton>()
            .map(|b| b.pressed())
            .unwrap_or(false)
    };
    // Standard-mapping indices → position bits.
    let pairs = [
        (0, buttons::SOUTH),
        (1, buttons::EAST),
        (2, buttons::WEST),
        (3, buttons::NORTH),
        (4, buttons::L1),
        (5, buttons::R1),
        (8, buttons::SELECT),
        (9, buttons::START),
        (12, buttons::DPAD_UP),
        (13, buttons::DPAD_DOWN),
        (14, buttons::DPAD_LEFT),
        (15, buttons::DPAD_RIGHT),
    ];
    let mut mask = 0u32;
    for (idx, bit) in pairs {
        if down(idx) {
            mask |= bit;
        }
    }
    let state = GamepadState {
        left_x: axis(0),
        left_y: axis(1),
        buttons: mask,
    };
    agg_gui::gamepad::set_state(Some(state));
    // Pads have no events — anything held/deflected must keep the
    // frame loop pumping or the app never sees the change.
    if state != GamepadState::default() {
        mark_dirty();
    }
}
