//! Browser platform glue: canvas lookup, client-platform detection, the
//! `agg_gui::fullscreen` service (with the mobile orientation lock), and the
//! fatal-error panel.
//!
//! Detection and fullscreen moved from `demo-wgpu`'s `web_shell`; the fatal
//! panel from AtomArtist's `show_fatal`.

use wasm_bindgen::JsCast;

use super::mark_dirty;
use super::sensors::screen_angle_degrees;
use crate::error::WebShellError;

pub(super) fn find_canvas(id: &str) -> Result<web_sys::HtmlCanvasElement, WebShellError> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or(WebShellError::NoWindow)?;
    document
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or_else(|| WebShellError::CanvasNotFound(id.to_string()))
}

/// Detect the client platform once at boot: OS name for shortcut labels,
/// `(pointer: coarse)` for the touch input profile / on-screen keyboard /
/// UX scale.
///
/// Debug override: `?agg_input=mobile` (or `=desktop`) in the page URL forces
/// the result, so a desktop browser can exercise the mobile layout.
pub(super) fn apply_client_platform() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let name = window.navigator().user_agent().unwrap_or_default();
    let mut pointer_coarse = window
        .match_media("(pointer: coarse)")
        .ok()
        .flatten()
        .map(|m| m.matches())
        .unwrap_or(false);
    if let Ok(search) = window.location().search() {
        if search.contains("agg_input=mobile") {
            pointer_coarse = true;
        } else if search.contains("agg_input=desktop") {
            pointer_coarse = false;
        }
    }
    agg_gui::set_platform(agg_gui::platform_from_name(&name));
    let profile = agg_gui::input_profile::input_profile_from_hint(&name, pointer_coarse);
    agg_gui::input_profile::set_input_profile(profile);
    agg_gui::widgets::on_screen_keyboard::set_enabled(profile.is_mobile_touch());
    // UX zoom only at the platform-shell boundary, where the device class is
    // genuinely known — programmatic profile flips never resize the UI.
    agg_gui::ux_scale::set_ux_scale(profile.recommended_ux_scale());
}

/// Apply app-requested fullscreen toggles and track the live state (Esc
/// leaves fullscreen without telling the app). Returns whether the page is
/// fullscreen now.
pub(super) fn service_fullscreen(canvas: &web_sys::HtmlCanvasElement) -> bool {
    let document = web_sys::window().and_then(|w| w.document());
    if agg_gui::fullscreen::take_request() {
        // Requests originate from click/keydown handlers, so the browser's
        // transient user activation is still in effect.
        if let Some(document) = &document {
            if document.fullscreen_element().is_some() {
                document.exit_fullscreen();
            } else {
                let _ = canvas.request_fullscreen();
            }
        }
    }
    let fs_now = document
        .map(|d| d.fullscreen_element().is_some())
        .unwrap_or(false);
    if fs_now != agg_gui::fullscreen::is_active() {
        agg_gui::fullscreen::set_active(fs_now);
        if agg_gui::input_profile::is_mobile_touch() {
            lock_orientation(fs_now);
        }
        mark_dirty();
    }
    fs_now
}

/// Mobile: pin the *current* orientation while fullscreen, so a tilt-steered
/// game doesn't spin back to portrait when the player leans the device.
/// `lock()` only works in fullscreen; rejections are swallowed.
fn lock_orientation(fullscreen: bool) {
    let Some(orientation) = web_sys::window()
        .and_then(|w| w.screen().ok())
        .map(|s| s.orientation())
    else {
        return;
    };
    if !fullscreen {
        let _ = orientation.unlock();
        return;
    }
    let landscape = screen_angle_degrees() % 180.0 != 0.0;
    let lock = js_sys::Reflect::get(&orientation, &"lock".into())
        .ok()
        .filter(|f| f.is_function())
        .map(js_sys::Function::from);
    if let Some(lock) = lock {
        let kind = if landscape { "landscape" } else { "portrait" };
        if let Ok(p) = lock.call1(&orientation, &kind.into()) {
            if let Ok(p) = p.dyn_into::<js_sys::Promise>() {
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
                });
            }
        }
    }
}

/// Replace the canvas with a readable error panel — users without WebGPU
/// should see *why* the page is blank, not a dead canvas.
pub(super) fn show_fatal_panel(message: &str) {
    let Some(canvas) = super::CANVAS.with(|c| c.borrow().clone()) else {
        return;
    };
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if let Ok(panel) = document.create_element("div") {
        let _ = panel.set_attribute(
            "style",
            "max-width:40em;margin:4em auto;padding:1.5em 2em;\
             font:16px/1.5 system-ui,sans-serif;color:#333;\
             background:#fff3f0;border:1px solid #e0b4a8;border-radius:8px;",
        );
        let _ = panel.set_attribute("role", "alert");
        panel.set_text_content(Some(message));
        let _ = canvas.replace_with_with_node_1(&panel);
    }
}
