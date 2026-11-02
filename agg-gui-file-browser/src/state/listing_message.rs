//! A folder whose listing is only a message: the one `MessageItem` a folder lists when it has
//! nothing else to show ("Loading..." while its listing is on its way; why it could not be
//! listed, such as an expired sign-in). The message is the folder's state, not something the user
//! can select, open, rename or drag, so the list shows it in the place of its rows
//! ([`ListingMessage`], `BrowserState::listing_message`) and lists no rows for it
//! (`mark_listed`), so Select All and the arrow keys find nothing either. A missing-file row (a
//! file that could not be opened) stays a row, and search results never count: their sections
//! say where each result came from. How the message is drawn is the list widget's.

use agg_gui_files::{Item, ListingFailure, MessageItem};

use super::{BrowserEntry, BrowserState};

/// A folder's listing that is only a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingMessage {
    /// What the message says.
    pub text: String,
    /// Whether it is the "Loading..." placeholder rather than a refusal.
    pub loading: bool,
    /// Whether the folder was refused for the sign-in, which signing in recovers.
    pub sign_in: bool,
}

/// The message `entries` are, when they are only one: a single `MessageItem` that is not a
/// missing-file row. Says nothing of the sign-in, which only the folder knows
/// (`BrowserState::listing_message`).
pub fn entries_message(entries: &[BrowserEntry]) -> Option<ListingMessage> {
    let [BrowserEntry::Item(item)] = entries else {
        return None;
    };
    let message = item.as_any()?.downcast_ref::<MessageItem>()?;
    if message.is_missing_file() {
        return None;
    }
    Some(ListingMessage {
        text: message.name().to_string(),
        loading: message.name() == MessageItem::loading().name(),
        sign_in: false,
    })
}

/// Whether `entries` are only a message ([`entries_message`]).
pub fn is_only_a_message(entries: &[BrowserEntry]) -> bool {
    entries_message(entries).is_some()
}

impl BrowserState {
    /// The message the showing folder lists in place of its contents, if that is all it lists;
    /// a refusal for the sign-in says so (`sign_in`). Never for search results.
    pub fn listing_message(&self) -> Option<ListingMessage> {
        if self.showing_search_results().is_some() {
            return None;
        }
        let failure = self.active_container().and_then(|c| c.listing_failure());
        entries_message(&self.entries()).map(|message| ListingMessage {
            sign_in: !message.loading && failure == Some(ListingFailure::SignIn),
            ..message
        })
    }
}
