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
//!   ([`SheetDialogs::with_move_root`]): a browser of its own over a new root, saying what
//!   accepting will do, answering with the folder picked and whether the items copy. Without a
//!   root Move is left unoffered.
//! - No file picker (agg-gui has none): Add stays greyed rather than doing nothing.
//!
//! Each question answers exactly once: a button, Escape, or the sheet closing any other way
//! (cancelled). A question asked while one shows replaces it, the first answered as cancelled.
//! Answers run from the sheet's event handling, with no browser state borrowed.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::animation::request_layout;

use agg_gui_files::Folder;

use crate::dialogs::{
    BrowserDialogs, Confirmation, DialogKind, MovePick, MoveRequest, NameRequest, PickedMoveTarget,
};

/// Makes the root the Move dialog's browser opens over (a new one per dialog).
pub type MoveRoot = Rc<dyn Fn() -> Box<dyn Folder>>;

mod sheet;

pub use sheet::{DialogSheet, CANCEL_BUTTON, CONFIRM_BUTTON, NAME_FIELD};

/// A question waiting to show, with what hears its answer.
pub(crate) enum Question {
    Confirm(Confirmation, Box<dyn FnOnce(bool)>),
    AskName(NameRequest, Box<dyn FnOnce(Option<String>)>),
    PickMove(MoveRequest, PickedMoveTarget),
}

impl Question {
    /// Answers the question as cancelled.
    pub(crate) fn cancel(self) {
        match self {
            Question::Confirm(_, answer) => answer(false),
            Question::AskName(_, answer) => answer(None),
            Question::PickMove(_, answer) => answer(None),
        }
    }
}

/// What the dialogs and their sheet share: the question waiting for the sheet's next layout,
/// and whether one shows.
#[derive(Default)]
pub(crate) struct Shared {
    pub pending: RefCell<Option<Question>>,
    pub showing: Cell<bool>,
    pub move_root: RefCell<Option<MoveRoot>>,
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
    pub fn with_move_root(self, root: impl Fn() -> Box<dyn Folder> + 'static) -> Self {
        *self.shared.move_root.borrow_mut() = Some(Rc::new(root));
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
            DialogKind::PickMoveTarget => self.shared.move_root.borrow().is_some(),
            DialogKind::PickFiles => false,
        }
    }

    fn confirm(&self, question: Confirmation, answer: Box<dyn FnOnce(bool)>) {
        self.ask(Question::Confirm(question, answer));
    }

    fn ask_name(&self, request: NameRequest, answer: Box<dyn FnOnce(Option<String>)>) {
        self.ask(Question::AskName(request, answer));
    }

    fn pick_move_target(&self, request: MoveRequest, answer: PickedMoveTarget) {
        match self.shared.move_root.borrow().is_some() {
            true => self.ask(Question::PickMove(request, answer)),
            false => answer(None),
        }
    }
}

/// Answers a Move question with `pick`.
pub(crate) fn answer_move(question: Option<Question>, pick: MovePick) {
    if let Some(Question::PickMove(_, answer)) = question {
        answer(Some(pick));
    }
}
