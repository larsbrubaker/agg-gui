//! WASM platform convention bridge.
//!
//! JavaScript detects the browser client's OS and reports it here so the shared
//! menu and shortcut code can display and match platform-native accelerators.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn set_client_platform(platform_name: &str) {
    agg_gui::set_platform(agg_gui::platform_from_name(platform_name));
    crate::mark_dirty();
}
