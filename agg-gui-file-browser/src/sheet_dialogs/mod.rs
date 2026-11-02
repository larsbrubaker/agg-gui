//! A ready set of dialogs ([`SheetDialogs`], a `BrowserDialogs`) for an app without its own, shown
//! on agg-gui's `ModalSheet` by a [`DialogSheet`] the app places as a late child of its root
//! `Stack` (so it paints over the app and its scrim takes every press while it shows). An app
//! with its own dialogs gives them through the hook instead.
//!
//! - A confirm: the question's title and message, Cancel and the confirming button (the
//!   default: Enter presses it; Escape cancels).
//! - Ask for a name: the title, the prompt, a field holding the starting name with all of it
//!   selected (typing replaces it), Cancel and the confirming button. Enter in the field or the
//!   button accepts; an empty name accepts nothing and the sheet stays open, so the user can
//!   type one. Whether the name is free is the folder's to say: a name it refuses shows on the
//!   browser's status line, the dialog closed.
//! - The Move dialog (`move_dialog/`), once given a root to browse from
//!   ([`SheetDialogs::with_browse_root`]): a browser of its own over a new root, saying what
//!   accepting will do, answering with the folder picked and whether the items copy; browsing
//!   with the app's config when given one ([`SheetDialogs::with_browse_config`]) and opening where
//!   it was last left under the app's key ([`SheetDialogs::with_move_location_key`]). Without a
//!   root Move is left unoffered.
//! - The Save dialog (`save_dialog.rs`'s `SavePage`) over the same root and config, opening
//!   where it was last left under its own key ([`SheetDialogs::with_save_location_key`]),
//!   answering with the folder and file name; the app writes the file.
//! - The Open dialog (`open_dialog.rs`'s `OpenPage`) over the same root and config, listing every
//!   folder (read-only ones too: opening takes nothing in) and the files its request's filter
//!   keeps, one selectable at a time unless the request allows several; opening where it was last
//!   left under its own key ([`SheetDialogs::with_open_location_key`]), answering with the files
//!   and their folder; the app reads them.
//! - No picker of files to add (agg-gui has no system one): Add stays greyed rather than doing
//!   nothing.
//!
//! Each question answers exactly once: a button, Escape, or the sheet closing any other way
//! (cancelled). A question asked while one shows replaces it, the first answered as cancelled.
//! Answers run from the sheet's event handling, with no browser state borrowed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::animation::request_layout;

use agg_gui_files::Folder;

use crate::dialogs::{
    BrowserDialogs, Confirmation, DialogKind, MovePick, MoveRequest, NameRequest, OpenPick,
    OpenRequest, PickedMoveTarget, PickedOpenTarget, PickedSaveTarget, SavePick, SaveRequest,
};

/// Makes the root the Move dialog's browser opens over (a new one per dialog).
pub type BrowseRoot = Rc<dyn Fn() -> Box<dyn Folder>>;

/// Makes the config of the Move dialog's browser (a new one per dialog).
pub type BrowseConfig = Rc<dyn Fn() -> crate::config::BrowserConfig>;

mod sheet;

pub use sheet::{DialogSheet, CANCEL_BUTTON, CONFIRM_BUTTON, NAME_FIELD};

/// A question waiting to show, with what hears its answer.
pub(crate) enum Question {
    Confirm(Confirmation, Box<dyn FnOnce(bool)>),
    AskName(NameRequest, Box<dyn FnOnce(Option<String>)>),
    PickMove(MoveRequest, PickedMoveTarget),
    PickSave(SaveRequest, PickedSaveTarget),
    PickOpen(OpenRequest, PickedOpenTarget),
}

impl Question {
    /// Answers the question as cancelled.
    pub(crate) fn cancel(self) {
        match self {
            Question::Confirm(_, answer) => answer(false),
            Question::AskName(_, answer) => answer(None),
            Question::PickMove(_, answer) => answer(None),
            Question::PickSave(_, answer) => answer(None),
            Question::PickOpen(_, answer) => answer(None),
        }
    }
}

/// What the dialogs and their sheet share: the question waiting for the sheet's next layout,
/// and whether one shows.
#[derive(Default)]
pub(crate) struct Shared {
    pub pending: RefCell<Option<Question>>,
    pub showing: Cell<bool>,
    pub browse_root: RefCell<Option<BrowseRoot>>,
    pub browse_config: RefCell<Option<BrowseConfig>>,
    pub move_location_key: RefCell<Option<String>>,
    pub save_location_key: RefCell<Option<String>>,
    pub open_location_key: RefCell<Option<String>>,
}

/// Dialogs shown on a [`DialogSheet`] (see the module header).
#[derive(Clone, Default)]
pub struct SheetDialogs {
    shared: Rc<Shared>,
}

impl SheetDialogs {
    /// Dialogs with no sheet yet: make it with [`SheetDialogs::sheet`].
    pub fn new() -> Self {
        Self::default()
    }

    /// The dialogs, with the Move dialog browsing from the root `root` makes.
    pub fn with_browse_root(self, root: impl Fn() -> Box<dyn Folder> + 'static) -> Self {
        *self.shared.browse_root.borrow_mut() = Some(Rc::new(root));
        self
    }

    /// The dialogs, with the Move, Save and Open dialogs' browsers made with the config `config`
    /// makes: the app's look, spawner and memory (start its options from
    /// `move_dialog::dialog_options`; the Open dialog drops the folder filter and takes its
    /// request's file filter and selection). Without one the dialogs browse in `DefaultLook` on
    /// the default spawner.
    pub fn with_browse_config(
        self,
        config: impl Fn() -> crate::config::BrowserConfig + 'static,
    ) -> Self {
        *self.shared.browse_config.borrow_mut() = Some(Rc::new(config));
        self
    }

    /// The dialogs, with the Move dialog opening where it was last left, remembered through its
    /// config's `BrowserMemory` under `key` (an app's own key: MatterCAD's is
    /// `MoveItemLastBrowsedPath`).
    pub fn with_move_location_key(self, key: impl Into<String>) -> Self {
        *self.shared.move_location_key.borrow_mut() = Some(key.into());
        self
    }

    /// The dialogs, with the Save dialog opening where it was last left, remembered under `key`
    /// (MatterCAD's is `SaveAsLastBrowsedPath`).
    pub fn with_save_location_key(self, key: impl Into<String>) -> Self {
        *self.shared.save_location_key.borrow_mut() = Some(key.into());
        self
    }

    /// The dialogs, with the Open dialog opening where it was last left, remembered under `key`.
    pub fn with_open_location_key(self, key: impl Into<String>) -> Self {
        *self.shared.open_location_key.borrow_mut() = Some(key.into());
        self
    }

    /// The sheet these dialogs show on, drawing its text in `font`; place it as a late child of
    /// the app's root `Stack`.
    pub fn sheet(&self, font: std::sync::Arc<agg_gui::text::Font>) -> DialogSheet {
        DialogSheet::new(Rc::clone(&self.shared), font)
    }

    /// Whether a question is showing or waiting to show.
    pub fn is_asking(&self) -> bool {
        self.shared.showing.get() || self.shared.pending.borrow().is_some()
    }

    fn ask(&self, question: Question) {
        let replaced = self.shared.pending.borrow_mut().replace(question);
        if let Some(replaced) = replaced {
            replaced.cancel();
        }
        request_layout();
    }
}

impl BrowserDialogs for SheetDialogs {
    fn offers(&self, kind: DialogKind) -> bool {
        match kind {
            DialogKind::Confirm | DialogKind::AskName => true,
            DialogKind::PickMoveTarget
            | DialogKind::PickSaveTarget
            | DialogKind::PickOpenTarget => self.shared.browse_root.borrow().is_some(),
            DialogKind::PickFiles => false,
        }
    }

    fn confirm(&self, question: Confirmation, answer: Box<dyn FnOnce(bool)>) {
        self.ask(Question::Confirm(question, answer));
    }

    fn ask_name(&self, request: NameRequest, answer: Box<dyn FnOnce(Option<String>)>) {
        self.ask(Question::AskName(request, answer));
    }

    fn pick_save_target(&self, request: SaveRequest, answer: PickedSaveTarget) {
        match self.shared.browse_root.borrow().is_some() {
            true => self.ask(Question::PickSave(request, answer)),
            false => answer(None),
        }
    }

    fn pick_move_target(&self, request: MoveRequest, answer: PickedMoveTarget) {
        match self.shared.browse_root.borrow().is_some() {
            true => self.ask(Question::PickMove(request, answer)),
            false => answer(None),
        }
    }

    fn pick_open_target(&self, request: OpenRequest, answer: PickedOpenTarget) {
        match self.shared.browse_root.borrow().is_some() {
            true => self.ask(Question::PickOpen(request, answer)),
            false => answer(None),
        }
    }
}

/// Answers an Open question with `pick`.
pub(crate) fn answer_open(question: Option<Question>, pick: OpenPick) {
    if let Some(Question::PickOpen(_, answer)) = question {
        answer(Some(pick));
    }
}

/// Answers a Save question with `pick`.
pub(crate) fn answer_save(question: Option<Question>, pick: SavePick) {
    if let Some(Question::PickSave(_, answer)) = question {
        answer(Some(pick));
    }
}

/// Answers a Move question with `pick`.
pub(crate) fn answer_move(question: Option<Question>, pick: MovePick) {
    if let Some(Question::PickMove(_, answer)) = question {
        answer(Some(pick));
    }
}
