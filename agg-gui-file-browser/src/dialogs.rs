//! The questions a browser asks the user, through the app's dialogs ([`BrowserDialogs`]):
//! confirming a change ([`Confirmation`]: removing items), asking for a name ([`NameRequest`]:
//! a new folder, a rename) and picking files to add.
//!
//! The browser asks only what [`BrowserDialogs::offers`] says the app can show, and leaves the
//! action that needs a dialog it lacks unavailable (greyed in its menu), so nothing is ever done
//! without the question being asked. [`NoDialogs`] offers none: a browser without dialogs adds,
//! names and removes nothing through its menus. Each method's default answers as if the user
//! cancelled, so a call that reaches it changes nothing.
//!
//! An answer may come before the call returns or long after (a dialog the user takes time
//! over); the browser is not borrowed while it waits.

use std::rc::Rc;

use agg_gui_files::Item;

/// A dialog a browser can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DialogKind {
    /// Confirm a change ([`BrowserDialogs::confirm`]).
    Confirm,
    /// Ask for a name ([`BrowserDialogs::ask_name`]).
    AskName,
    /// Pick files to add ([`BrowserDialogs::pick_files`]).
    PickFiles,
}

/// A change the user confirms: the dialog's title, what it says, and the confirming button's
/// label, all already translated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Confirmation {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
}

impl Confirmation {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
        }
    }
}

/// A name the user types: the dialog's title, its prompt, the name it starts with, and the
/// confirming button's label, all already translated.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NameRequest {
    pub title: String,
    pub prompt: String,
    pub initial_name: String,
    pub confirm_label: String,
}

impl NameRequest {
    pub fn new(
        title: impl Into<String>,
        prompt: impl Into<String>,
        initial_name: impl Into<String>,
        confirm_label: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            prompt: prompt.into(),
            initial_name: initial_name.into(),
            confirm_label: confirm_label.into(),
        }
    }
}

/// Hears the files the user picked, as items (none when they cancelled).
pub type PickedFiles = Box<dyn FnOnce(Vec<Rc<dyn Item>>)>;

/// The app's dialogs (see the module header).
pub trait BrowserDialogs {
    /// Whether the app can show a `kind` dialog. The default offers none.
    fn offers(&self, kind: DialogKind) -> bool {
        let _ = kind;
        false
    }

    /// Asks the user to confirm `question`; `answer` hears whether they did. The default answers
    /// no.
    fn confirm(&self, question: Confirmation, answer: Box<dyn FnOnce(bool)>) {
        let _ = question;
        answer(false);
    }

    /// Asks the user for a name; `answer` hears it, or `None` when they cancelled. The default
    /// answers `None`.
    fn ask_name(&self, request: NameRequest, answer: Box<dyn FnOnce(Option<String>)>) {
        let _ = request;
        answer(None);
    }

    /// Asks the user to pick files to add; `answer` hears them as items (none when they
    /// cancelled). The default answers none.
    fn pick_files(&self, answer: PickedFiles) {
        answer(Vec::new());
    }
}

/// The dialogs of an app with none (see the module header).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDialogs;

impl BrowserDialogs for NoDialogs {}
