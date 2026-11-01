//! `ENGLISH_TEXTS` lists exactly the English literals this crate hands to `tr`, so an app that
//! checks its translations against the list checks every text the crate shows. The crate's own
//! sources are read and every `tr("...")` literal compared with the list.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use agg_gui_file_browser::ENGLISH_TEXTS;

/// Every `tr("...")` literal in the `.rs` files under `dir`.
fn literals_in(dir: &Path, found: &mut BTreeSet<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            literals_in(&path, found);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("a source file reads as text");
        for (at, _) in text.match_indices("tr(\"") {
            // `tr(` itself, not the end of a longer name.
            let before = text[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let rest = &text[at + 4..];
            let end = rest.find('"').expect("a closed string literal");
            found.insert(rest[..end].to_string());
        }
    }
}

#[test]
fn english_texts_lists_every_literal_handed_to_tr() {
    let mut found = BTreeSet::new();
    literals_in(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    let listed: BTreeSet<String> = ENGLISH_TEXTS.iter().map(|t| t.to_string()).collect();
    assert_eq!(
        listed.len(),
        ENGLISH_TEXTS.len(),
        "each text is listed once"
    );
    assert_eq!(
        found, listed,
        "ENGLISH_TEXTS and the crate's tr(\"...\") literals differ"
    );
}
