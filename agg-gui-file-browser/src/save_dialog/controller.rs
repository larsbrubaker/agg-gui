//! [`SaveController`]: the Save dialog's logic without its widgets, so an app can wrap it in its
//! own page (its heading, field, buttons, footer and names) while
//! [`SavePage`](super::SavePage) is the dialog's default page over it.
//!
//! The controller holds the request, the dialog's browser, its answers and the name being typed
//! ([`SaveController::name_cell`], which the page's field shares). Save needs a folder that takes
//! files (the showing folder, below the root, writable, and allowed by the app's own rule,
//! [`SaveFolderRule`]: a cloud folder only while signed in) and a name:
//! [`SaveController::can_save`] says whether the folder takes it, [`SaveController::refusal`]
//! why not, and [`SaveController::accept_enabled`] whether Save can run now, so a page disables
//! Save up front.
//!
//! [`SaveController::accept`] names the typed name through the request's
//! [`FileNamer`](crate::FileNamer) (its refusal becomes the note), then asks the request's own
//! question hook, else the dialog's own question when a file of that name is already there
//! ("Confirm Save As"), else saves. A question goes to the app's [`QuestionHost`] when it has one
//! (its modal message box: confirming saves, cancelling stays); without one the controller asks
//! in the page ([`SaveMode::Asking`]; the page's buttons call [`SaveController::confirm`] or
//! [`SaveController::back`]). Saving hands the showing folder over and answers with it and the
//! file name ([`SavePick`]); should something still hold the folder, nothing is saved and the
//! note says so.
//!
//! [`SaveController::update`], called by the page at each layout before its browser lays out,
//! opens the dialog's place (a starting folder, else where it was last left: `move_dialog`'s
//! `place.rs`), asks once for the field's focus when the request gives a focus id, remembers the
//! user's moves, and takes the list's clicks: a click on a file in a folder that takes the save
//! puts its name in the field as the request shows names, and a double click saves.
//!
//! Create Folder ([`SaveController::create_folder`]) asks the new folder's name through the app's
//! dialogs when given them ([`SaveController::asking_names_through`]: its input box), else in the
//! page ([`SaveMode::NamingFolder`]); the folder is made in the showing folder, recorded for Undo,
//! then opened.
//!
//! Whenever what a page shows changes (the note, the question), [`SaveController::changes`]
//! moves on and a layout is asked for; a page rebuilds its parts when it or the browser's
//! navigation moves.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use agg_gui_files::{tr, Folder};

use crate::dialogs::{
    BrowserDialogs, NameRequest, ReplaceQuestion, SavePick, SaveRequest, SaveTarget,
};
use crate::memory::LocationFuture;
use crate::move_dialog::place::DialogPlace;
use crate::state::{BrowserState, ItemEvent};

/// What a Save dialog answers through: a pick, or a cancel.
pub struct SaveAnswers {
    pub accept: Rc<dyn Fn(SavePick)>,
    pub cancel: Rc<dyn Fn()>,
}

/// The app's own rule for the showing folder, asked after the dialog's (below the root,
/// writable): `Err` with the reason, already translated, when the folder takes no save now.
pub type SaveFolderRule = Rc<dyn Fn(&dyn Folder) -> Result<(), String>>;

/// The app's own way of asking a question before a save (its modal message box): it shows the
/// question and calls the answer with `true` to save (the question's confirming button) or
/// `false` to stay in the dialog. A question with no confirming label only ever answers `false`.
pub type QuestionHost = Rc<dyn Fn(ReplaceQuestion, Box<dyn FnOnce(bool)>)>;

/// What the dialog is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveMode {
    /// Taking a name.
    Normal,
    /// Asking `question` in the page before saving as `file_name`.
    Asking {
        question: ReplaceQuestion,
        file_name: String,
    },
    /// Asking the new folder's name in the page.
    NamingFolder,
}

struct Core {
    state: Rc<RefCell<BrowserState>>,
    request: SaveRequest,
    answers: SaveAnswers,
    name: Rc<RefCell<String>>,
    folder_name: Rc<RefCell<String>>,
    mode: RefCell<SaveMode>,
    note: RefCell<String>,
    changes: Cell<u64>,
    place: DialogPlace,
    folder_rule: RefCell<Option<SaveFolderRule>>,
    questions: RefCell<Option<QuestionHost>>,
    names: RefCell<Option<Rc<dyn BrowserDialogs>>>,
    focus_asked: Cell<bool>,
}

/// The Save dialog's logic (see the module header). Clones share one dialog.
#[derive(Clone)]
pub struct SaveController {
    core: Rc<Core>,
}

impl SaveController {
    /// The dialog saving per `request`, browsing `state`, answering through `answers`; the name
    /// starts as the request's.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        request: SaveRequest,
        answers: SaveAnswers,
    ) -> Self {
        Self {
            core: Rc::new(Core {
                state: Rc::clone(state),
                name: Rc::new(RefCell::new(request.initial_name.clone())),
                folder_name: Rc::new(RefCell::new(String::new())),
                mode: RefCell::new(SaveMode::Normal),
                note: RefCell::new(String::new()),
                changes: Cell::new(0),
                place: DialogPlace::default(),
                folder_rule: RefCell::new(None),
                questions: RefCell::new(None),
                names: RefCell::new(None),
                focus_asked: Cell::new(false),
                request,
                answers,
            }),
        }
    }

    /// The dialog opening where it was last left under `key` (unless given a starting folder)
    /// and remembering where the user goes.
    pub fn remembering(self, key: impl Into<String>) -> Self {
        self.core.place.remember_under(key);
        self
    }

    /// The dialog opening in the folders `start` finds below the root (the design's own folder).
    pub fn starting_in(self, start: LocationFuture) -> Self {
        self.core.place.start_in(start);
        self
    }

    /// The dialog taking a save only in folders `rule` allows too (see [`SaveFolderRule`]).
    pub fn with_folder_rule(self, rule: SaveFolderRule) -> Self {
        *self.core.folder_rule.borrow_mut() = Some(rule);
        self
    }

    /// The dialog asking its questions through `host` rather than in the page.
    pub fn with_question_host(self, host: QuestionHost) -> Self {
        *self.core.questions.borrow_mut() = Some(host);
        self
    }

    /// The dialog asking Create Folder's name through `dialogs`' `ask_name` rather than in the
    /// page.
    pub fn asking_names_through(self, dialogs: Rc<dyn BrowserDialogs>) -> Self {
        *self.core.names.borrow_mut() = Some(dialogs);
        self
    }

    /// The dialog's browser.
    pub fn state(&self) -> &Rc<RefCell<BrowserState>> {
        &self.core.state
    }

    /// The request the dialog answers.
    pub fn request(&self) -> &SaveRequest {
        &self.core.request
    }

    /// The name typed so far, shared with the page's field (`TextField::with_text_cell`).
    pub fn name_cell(&self) -> Rc<RefCell<String>> {
        Rc::clone(&self.core.name)
    }

    /// The name typed so far.
    pub fn name(&self) -> String {
        self.core.name.borrow().clone()
    }

    /// The new folder's name typed so far, for a page asking it itself ([`SaveMode::NamingFolder`]).
    pub fn folder_name_cell(&self) -> Rc<RefCell<String>> {
        Rc::clone(&self.core.folder_name)
    }

    /// The name field's grey hint while empty, from the request.
    pub fn placeholder(&self) -> Option<&str> {
        self.core.request.placeholder.as_deref()
    }

    /// What the dialog is doing.
    pub fn mode(&self) -> SaveMode {
        self.core.mode.borrow().clone()
    }

    /// What the last try said (a name the namer refused, a busy folder), until the name changes.
    pub fn note(&self) -> String {
        self.core.note.borrow().clone()
    }

    /// Moves on whenever the note or the mode changes.
    pub fn changes(&self) -> u64 {
        self.core.changes.get()
    }

    /// Whether the showing folder takes the save.
    pub fn can_save(&self) -> bool {
        self.refusal().is_none()
    }

    /// Why the showing folder takes no save, or `None` when it does: "Choose a folder you can
    /// save in." for the root or a folder that can't be written, else the app's rule's reason.
    pub fn refusal(&self) -> Option<String> {
        let path = {
            let s = self.core.state.borrow();
            let writable = s.depth() > 1
                && s.active_container()
                    .is_some_and(|c| c.as_writable().is_some());
            if !writable {
                return Some(tr("Choose a folder you can save in."));
            }
            s.open_path().last().cloned()
        };
        let rule = self.core.folder_rule.borrow().clone()?;
        let folder = path?;
        let folder = folder.borrow();
        rule(folder.as_ref()).err()
    }

    /// Whether Save can run now: the folder takes the save, a name is typed and the dialog has
    /// not answered.
    pub fn accept_enabled(&self) -> bool {
        !self.core.place.accepted.get()
            && !self.core.name.borrow().trim().is_empty()
            && self.can_save()
    }

    fn changed(&self) {
        let core = &self.core;
        core.changes.set(core.changes.get() + 1);
        agg_gui::animation::request_layout();
    }

    /// The page's field changed: a new name is a new save, so the note and any question asked in
    /// the page, both about the old one, go.
    pub fn edited(&self) {
        let asking = matches!(*self.core.mode.borrow(), SaveMode::Asking { .. });
        if asking {
            *self.core.mode.borrow_mut() = SaveMode::Normal;
        }
        if asking || !self.core.note.borrow().is_empty() {
            self.core.note.borrow_mut().clear();
            self.changed();
        }
    }

    /// Puts `name` in the field (the page's field shows it at its next layout).
    pub fn set_name(&self, name: impl Into<String>) {
        *self.core.name.borrow_mut() = name.into();
        self.core.note.borrow_mut().clear();
        *self.core.mode.borrow_mut() = SaveMode::Normal;
        self.changed();
    }

    /// What the page calls at each layout before laying its browser out (see the module
    /// header).
    pub fn update(&self) {
        self.core.place.update(&self.core.state);
        if let Some(id) = self.core.request.focus_id {
            if !self.core.focus_asked.replace(true) {
                agg_gui::focus::request_focus(id);
            }
        }
        let events = self.core.state.borrow_mut().take_item_events();
        for event in events {
            if !self.can_save() {
                continue;
            }
            let (entry, save) = match event {
                ItemEvent::Clicked(entry) => (entry, false),
                ItemEvent::Activated(entry) => (entry, true),
            };
            self.set_name((self.core.request.shown_name)(entry.name()));
            if save {
                self.accept();
            }
        }
    }

    /// Save, as the module header says.
    pub fn accept(&self) {
        let core = &self.core;
        if core.place.accepted.get() {
            return;
        }
        if let Some(why) = self.refusal() {
            *core.note.borrow_mut() = why;
            return self.changed();
        }
        let typed = core.name.borrow().trim().to_string();
        if typed.is_empty() {
            return;
        }
        let file_name = match (core.request.namer)(&typed) {
            Ok(name) => name,
            Err(why) => {
                *core.note.borrow_mut() = why;
                return self.changed();
            }
        };
        let exists = self.exists(&file_name);
        let question = self.app_question(&file_name, exists).or_else(|| {
            exists
                .then(|| ReplaceQuestion::replace_existing(&(core.request.shown_name)(&file_name)))
        });
        let Some(question) = question else {
            return self.finish(file_name);
        };
        let host = core.questions.borrow().clone();
        match host {
            Some(host) => {
                let weak: Weak<Core> = Rc::downgrade(&self.core);
                host(
                    question,
                    Box::new(move |save| {
                        if let (true, Some(core)) = (save, weak.upgrade()) {
                            SaveController { core }.finish(file_name);
                        }
                    }),
                );
            }
            None => {
                *core.mode.borrow_mut() = SaveMode::Asking {
                    question,
                    file_name,
                };
                self.changed();
            }
        }
    }

    /// The in-page question's confirming button: saves under the name it asked about.
    pub fn confirm(&self) {
        let asked = match &*self.core.mode.borrow() {
            SaveMode::Asking { file_name, .. } => Some(file_name.clone()),
            _ => None,
        };
        if let Some(file_name) = asked {
            self.finish(file_name);
        }
    }

    /// Back to taking a name from a question or the new folder's name asked in the page.
    pub fn back(&self) {
        *self.core.mode.borrow_mut() = SaveMode::Normal;
        self.changed();
    }

    /// Answers as cancelled.
    pub fn cancel(&self) {
        (self.core.answers.cancel)();
    }

    /// Whether the showing folder lists a file named `file_name`.
    fn exists(&self, file_name: &str) -> bool {
        self.core
            .state
            .borrow()
            .active_container()
            .is_some_and(|c| c.items().iter().any(|i| i.name() == file_name))
    }

    /// The request's own question before saving as `file_name`, if it has one: its hook handed
    /// the showing folder with the browser's state let go.
    fn app_question(&self, file_name: &str, exists: bool) -> Option<ReplaceQuestion> {
        let hook = self.core.request.replace_question.as_ref()?;
        let folder = self.core.state.borrow().open_path().last().cloned()?;
        let folder = folder.borrow();
        hook(&SaveTarget {
            folder: folder.as_ref(),
            file_name,
            exists,
        })
    }

    /// Hands the showing folder over with `file_name` (the user has agreed to any question).
    fn finish(&self, file_name: String) {
        let core = &self.core;
        if core.place.accepted.get() || !self.can_save() {
            return;
        }
        // A folder something else still holds stays showing, so the user can try again there.
        if !core.state.borrow().can_hand_over_active_container() {
            return self.busy();
        }
        let exists = self.exists(&file_name);
        core.place.accepted.set(true);
        let taken = core.state.borrow_mut().take_active_container();
        match taken.map(Rc::try_unwrap) {
            Some(Ok(folder)) => (core.answers.accept)(SavePick {
                folder: folder.into_inner(),
                file_name,
                replaces: exists,
            }),
            _ => {
                core.place.accepted.set(false);
                self.busy();
            }
        }
    }

    /// Nothing was saved because the folder was held: back to taking a name, saying so.
    fn busy(&self) {
        *self.core.mode.borrow_mut() = SaveMode::Normal;
        *self.core.note.borrow_mut() = tr("The folder was busy, so nothing was saved. Try again.");
        self.changed();
    }

    /// Create Folder: asks the new folder's name through the app's dialogs, or in the page (see
    /// the module header). Does nothing where the folder takes no save.
    pub fn create_folder(&self) {
        if !self.can_save() {
            return;
        }
        let dialogs = self.core.names.borrow().clone();
        let Some(dialogs) = dialogs else {
            *self.core.mode.borrow_mut() = SaveMode::NamingFolder;
            return self.changed();
        };
        let request = NameRequest::new(tr("Create Folder"), tr("Folder Name"), "", tr("Create"));
        let weak: Weak<Core> = Rc::downgrade(&self.core);
        dialogs.ask_name(
            request,
            Box::new(move |name| {
                if let (Some(name), Some(core)) = (name, weak.upgrade()) {
                    SaveController { core }.create_folder_named(&name);
                }
            }),
        );
    }

    /// The page's new-folder field accepted: makes the folder it names.
    pub fn confirm_folder_name(&self) {
        let name = self.core.folder_name.borrow().clone();
        self.create_folder_named(&name);
    }

    /// Makes the folder `name` in the showing folder, recorded for Undo, then opens it. An empty
    /// name makes nothing.
    pub fn create_folder_named(&self, name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let state = &self.core.state;
        let depth = state.borrow().depth() - 1;
        let made = BrowserState::write_recorded(state, depth, |w| w.create_folder_recorded(&name));
        *self.core.mode.borrow_mut() = SaveMode::Normal;
        self.core.folder_name.borrow_mut().clear();
        if made.is_ok() {
            let link = {
                let s = state.borrow();
                s.active_container()
                    .and_then(|c| c.child_containers().into_iter().find(|l| l.name() == name))
            };
            if let Some(link) = link {
                state.borrow_mut().open(link.as_ref());
            }
        }
        self.changed();
    }

    /// What a page over the controller reports: `bread_crumb_trail`, `name`, `placeholder`,
    /// `question` ("normal", "replace {file name}" or "new folder"), `can_save` and
    /// `accept_enabled`.
    pub fn properties(&self) -> Vec<(&'static str, String)> {
        let mode = match self.mode() {
            SaveMode::Normal => "normal".to_string(),
            SaveMode::Asking { file_name, .. } => format!("replace {file_name}"),
            SaveMode::NamingFolder => "new folder".to_string(),
        };
        vec![
            (
                "bread_crumb_trail",
                self.core.state.borrow().path_names().join("/"),
            ),
            ("name", self.name()),
            (
                "placeholder",
                self.placeholder().unwrap_or_default().to_string(),
            ),
            ("question", mode),
            ("can_save", self.can_save().to_string()),
            ("accept_enabled", self.accept_enabled().to_string()),
        ]
    }
}
