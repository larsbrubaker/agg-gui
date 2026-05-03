//! WASM clipboard and text-input focus exports — mirror of
//! `demo-wasm/src/clipboard_exports.rs`.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn wasm_clipboard_get() -> Option<String> {
    agg_gui::wasm_clipboard::get()
}

#[wasm_bindgen]
pub fn wasm_clipboard_get_html() -> Option<String> {
    agg_gui::wasm_clipboard::get_html()
}

#[wasm_bindgen]
pub fn wasm_clipboard_set(text: &str) {
    agg_gui::wasm_clipboard::set(text);
}

#[wasm_bindgen]
pub fn text_input_focused() -> bool {
    crate::DEMO_APP.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|app| app.focused_widget_type_name())
            .map(|name| matches!(name, "TextField" | "TextArea"))
            .unwrap_or(false)
    })
}
