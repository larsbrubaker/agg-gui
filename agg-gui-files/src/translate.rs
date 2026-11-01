//! The translation hook for text the user reads. The crate writes its text in English and hands
//! it to [`tr`]; an app with translations points [`set_translator`] at its own lookup once at
//! startup, and without one the English shows. Every English literal handed to `tr` in this
//! crate is in [`ENGLISH_TEXTS`], so an app can check its translation table covers them (a test,
//! `tests/english_texts.rs`, keeps the list and the literals the same). Text made from a template
//! is translated as the template, with its `{0}`-style holes filled after.

use std::sync::RwLock;

/// An app's lookup from English to the user's language.
type Translator = fn(&str) -> String;

/// The app's translator, `None` until one is set.
static TRANSLATOR: RwLock<Option<Translator>> = RwLock::new(None);

/// Every English literal this crate hands to [`tr`].
pub const ENGLISH_TEXTS: &[&str] = &[
    "A folder cannot be copied into itself",
    "A name cannot be empty",
    "Could not be opened",
    "Loading",
    "Missing",
    "Search Results",
    "{0} has no content to copy",
    "{0} is already in this folder",
    "{0} is not in this folder",
];

/// `english` in the user's language: the translator's answer once one is set, `english` itself
/// before.
pub fn tr(english: &str) -> String {
    // Only a plain pointer is stored under the write guard and no app code runs while it is
    // held, so the lock is never left poisoned in practice; should it be, the value it holds
    // is still a whole pointer and is used as it is.
    let translator = match TRANSLATOR.read() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    };
    match translator {
        Some(translate) => translate(english),
        None => english.to_string(),
    }
}

/// Makes `translate` the translator [`tr`] asks, replacing any set before.
pub fn set_translator(translate: Translator) {
    match TRANSLATOR.write() {
        Ok(mut guard) => *guard = Some(translate),
        Err(poisoned) => *poisoned.into_inner() = Some(translate),
    }
}
