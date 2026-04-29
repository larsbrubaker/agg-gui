//! WASM font-loading exports.
//!
//! The browser shell fetches font bytes on demand and passes them here so the
//! shared demo UI can install fonts without depending on web APIs.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn default_font_request() -> String {
    format!(
        "{}\t{}",
        demo_ui::DEFAULT_FONT_NAME,
        demo_ui::font_asset_by_name(demo_ui::DEFAULT_FONT_NAME)
            .map(|asset| asset.path)
            .unwrap_or("assets/Nunito_Regular.ttf")
    )
}

#[wasm_bindgen]
pub fn fallback_font_paths() -> String {
    format!(
        "{}\t{}",
        demo_ui::FONT_AWESOME_PATH,
        demo_ui::EMOJI_FONT_PATH
    )
}

#[wasm_bindgen]
pub fn take_pending_font_request() -> Option<String> {
    demo_ui::take_pending_font_request().map(|(name, path)| format!("{name}\t{path}"))
}

#[wasm_bindgen]
pub fn install_loaded_font(
    name: String,
    primary_bytes: Vec<u8>,
    icon_bytes: Vec<u8>,
    emoji_bytes: Vec<u8>,
) -> bool {
    let ok = demo_ui::install_font_bytes(&name, primary_bytes, Some(icon_bytes), Some(emoji_bytes))
        .is_ok();
    if ok {
        crate::mark_dirty();
    }
    ok
}
