//! A folder whose listing is only a message ("Loading...", a refusal) lists no rows a user could
//! select, says its message for the list to show in their place, and says when signing in would
//! list it; a missing-file row stays a row.

use std::rc::Rc;

use agg_gui::Modifiers;
use agg_gui_file_browser::{BrowserConfig, BrowserEntry, BrowserState, ListingMessage};
use agg_gui_files::{Folder, FolderLink, Item, ListingFailure, MessageItem};

/// A folder listing one message item, refused for `failure`.
struct MessageFolder {
    message: Rc<dyn Item>,
    failure: Option<ListingFailure>,
}

impl Folder for MessageFolder {
    fn id(&self) -> &str {
        "messages"
    }
    fn name(&self) -> &str {
        "Cloud"
    }
    fn is_protected(&self) -> bool {
        true
    }
    fn child_containers(&self) -> Vec<Rc<dyn FolderLink>> {
        Vec::new()
    }
    fn items(&self) -> Vec<Rc<dyn Item>> {
        vec![Rc::clone(&self.message)]
    }
    fn load(&mut self) {}
    fn listing_failure(&self) -> Option<ListingFailure> {
        self.failure
    }
}

fn browser(
    message: MessageItem,
    failure: Option<ListingFailure>,
) -> Rc<std::cell::RefCell<BrowserState>> {
    let folder = MessageFolder {
        message: Rc::new(message),
        failure,
    };
    BrowserState::new(Box::new(folder), BrowserConfig::default())
}

#[test]
fn a_loading_folder_shows_its_message_and_lists_no_rows() {
    let state = browser(MessageItem::loading(), None);
    let message = state.borrow().listing_message();
    assert_eq!(
        message,
        Some(ListingMessage {
            text: MessageItem::loading().name().to_string(),
            loading: true,
            sign_in: false,
        })
    );
    BrowserState::click(&state, 0, Modifiers::default());
    state.borrow_mut().select_all();
    assert!(
        state.borrow().selected_entries().is_empty(),
        "nothing to select"
    );
}

#[test]
fn a_folder_refused_for_the_sign_in_says_signing_in_lists_it() {
    let state = browser(
        MessageItem::new("Sign in to see this folder"),
        Some(ListingFailure::SignIn),
    );
    let message = state.borrow().listing_message().expect("a message");
    assert!(message.sign_in);
    assert!(!message.loading);
    let other = browser(
        MessageItem::new("Error Loading Contents"),
        Some(ListingFailure::Other),
    );
    assert_eq!(
        other.borrow().listing_message().map(|m| m.sign_in),
        Some(false)
    );
}

#[test]
fn a_missing_file_row_stays_a_row() {
    let state = browser(MessageItem::missing_file("bracket.zip"), None);
    assert_eq!(state.borrow().listing_message(), None);
    let entries = state.borrow().entries();
    assert!(!agg_gui_file_browser::is_only_a_message(&entries));
    BrowserState::click(&state, 0, Modifiers::default());
    assert!(matches!(
        state.borrow().selected_entries().as_slice(),
        [BrowserEntry::Item(_)]
    ));
}
