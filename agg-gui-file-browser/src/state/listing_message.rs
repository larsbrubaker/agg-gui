//! A folder whose listing is only a message: the one `MessageItem` a folder lists when it has
//! nothing else to show ("Loading..." while its listing is on its way; why it could not be
//! listed, such as an expired sign-in). The message is the folder's state, not something the user
//! can select, open, rename or drag, so the list shows it in the place of its rows
//! ([`ListingMessage`], `BrowserState::listing_message`) and lists no rows for it
//! (`mark_listed`), so Select All and the arrow keys find nothing either. A missing-file row (a
//! file that could not be opened) stays a row, and search results never count: their sections
//! say where each result came from. How the message is drawn is the list widget's. A message is
//! the folder's state rather than a file, so the options' item filter never hides it
//! (`keeps_item`); a search's folder result answers to the folder rules instead (`keeps_result`).

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
    /// Whether the list shows `item`: a folder's message always, anything else when the options'
    /// item filter (if any) keeps it.
    pub(super) fn keeps_item(&self, item: &dyn Item) -> bool {
        let message = item.as_any().is_some_and(|a| a.is::<MessageItem>());
        message || self.item_filter.as_ref().is_none_or(|keep| keep(item))
    }

    /// Whether the list shows a search result: a folder found by its name is a folder row,
    /// shown unless "Show Folders" is off and when the options' folder filter keeps it; anything
    /// else as `keeps_item` says.
    pub(super) fn keeps_result(&self, item: &dyn Item) -> bool {
        match item.as_container_link() {
            Some(link) => {
                self.show_containers && self.container_filter.as_ref().is_none_or(|f| f(link))
            }
            None => self.keeps_item(item),
        }
    }

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
