//! The translation hook: English until an app sets a translator, the translator's answer after.
//! One test, because the translator is process-wide and tests in one binary run at once.

use agg_gui_files::{set_translator, tr};

fn shouting(english: &str) -> String {
    english.to_uppercase()
}

#[test]
fn text_is_english_until_a_translator_is_set_and_translated_after() {
    assert_eq!(tr("Create Folder..."), "Create Folder...");
    set_translator(shouting);
    assert_eq!(tr("Create Folder..."), "CREATE FOLDER...");
}
