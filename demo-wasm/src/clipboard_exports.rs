//! WASM clipboard, text-input focus (and the focused field's text) and
//! dictation exports (its state, and removing or reinstalling its provider)
//! — mirror of
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
    // Delegate the "is this an editable text widget" decision to agg-gui so
    // every editor (TextField, TextArea, RichTextEdit) is enrolled in one
    // place. RichTextEdit was previously omitted here, which left it without
    // the hidden-textarea focus that browsers require to deliver copy / cut /
    // paste events — so clipboard shortcuts silently did nothing in it.
    crate::DEMO_APP.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|app| app.focused_is_text_input())
            .unwrap_or(false)
    })
}

/// The focused text widget's text, `None` when no text widget has focus:
/// for page automation (`demo/tests/touch_paste.test.ts` reads back what a
/// touch Paste put in the field).
#[wasm_bindgen]
pub fn focused_text() -> Option<String> {
    crate::DEMO_APP.with(|cell| {
        let app = cell.borrow();
        let app = app.as_ref()?;
        let mut w: &dyn agg_gui::Widget = app.root();
        for &i in app.focused_path()? {
            w = w.children().get(i)?.as_ref();
        }
        w.text_input_value()
    })
}

/// The on-screen keyboard's dictation state as text (`Ready`, `Listening {
/// level: .. }`, ...), or `None` without a voice provider: for page
/// automation (`demo/tests/dictation.test.ts`) and console debugging.
#[wasm_bindgen]
pub fn dictation_state() -> Option<String> {
    agg_gui::widgets::on_screen_keyboard::voice_state().map(|s| format!("{s:?}"))
}

/// Remove the on-screen keyboard's voice provider (`false`), or install a
/// fresh one (`true`): for page automation (`demo/tests/dictation_drain.test.ts`
/// checks that a provider dropped mid-dictation releases its worker, its
/// microphone tracks and its audio context).
#[wasm_bindgen]
pub fn dictation_set_enabled(enabled: bool) {
    if enabled {
        crate::install_dictation();
    } else {
        agg_gui::widgets::on_screen_keyboard::set_voice_input(None);
    }
}
