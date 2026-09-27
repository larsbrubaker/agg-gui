//! Browser-only tests for the `localStorage` settings backend.
//!
//! `localStorage` is a browser API, so this target is wasm-only. Run with
//! `wasm-pack test --headless --chrome agg-gui-web-shell` (needs chromedriver).
//! The platform-neutral auto-save logic is covered natively in
//! `src/settings.rs`.

#![cfg(target_arch = "wasm32")]

use agg_gui_web_shell::{LocalStorageSettings, SettingsAutoSave, SettingsStore};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn round_trips_through_local_storage() {
    let store = LocalStorageSettings::new("agg-gui-web-shell.test.roundtrip");
    store.save("hello").expect("localStorage writable in a test browser");
    assert_eq!(store.load().as_deref(), Some("hello"));
}

#[wasm_bindgen_test]
fn auto_save_seeds_from_stored_blob() {
    let key = "agg-gui-web-shell.test.seed";
    LocalStorageSettings::new(key)
        .save("same")
        .expect("localStorage writable");
    let (mut saver, loaded) = SettingsAutoSave::new(LocalStorageSettings::new(key));
    assert_eq!(loaded.as_deref(), Some("same"));
    assert!(!saver.tick(true, || "same".into()));
    assert!(saver.tick(true, || "changed".into()));
    assert_eq!(saver.store().load().as_deref(), Some("changed"));
}
