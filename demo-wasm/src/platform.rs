//! WASM platform convention bridge — mirror of `demo-wasm/src/platform.rs`.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn set_client_platform(platform_name: &str) {
    agg_gui::set_platform(agg_gui::platform_from_name(platform_name));
    crate::mark_dirty();
}
