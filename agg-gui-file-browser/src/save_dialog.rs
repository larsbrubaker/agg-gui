//! [`SavePage`]: the Save dialog's page. Its heading (the request's title), a browser page of its
//! own (`move_dialog/page.rs`'s [`BrowserPage`]: read-only folders hidden, so nothing is offered
//! that can't take the file; opening where it was last left when given a location key), the name
//! field (the request's label over it, starting with its name, Enter saving), a note, and the
//! buttons: "Create Folder", Cancel and Save.
//!
//! Save needs a folder that can take files (the showing folder, below the root, writable: "Choose
//! a folder you can save in." otherwise) and a name. The typed name goes through the request's
//! [`FileNamer`](crate::FileNamer) (an app's extension added or checked; its refusal shows in the
//! note). Before saving, the request's question hook ([`SaveRequest::with_replace_question`]) is
//! asked whether the app has a question of its own; without one, a file of that name already in
//! the folder asks "Confirm Save As", ""Bracket" already exists. Do you want to replace it?" with
//! Replace and Cancel. A question is asked in the page itself (the dialogs show one at a time);
//! its Cancel goes back to the page, as does typing in the name field, and a question with no
//! confirming button only goes back. Saving hands the showing folder over and answers with it and
//! the file name; writing the file is the app's. Should something still hold the folder, nothing
//! is saved and the note says so.
//!
//! A click on a file in a folder that takes the save puts its name in the field as the request
//! shows names ([`SaveRequest::with_extension`]: "Bracket.mcx" fills in "Bracket"), how the user
//! aims a save at a file already there; a double click on it saves (asking first, as it is
//! there). The page takes the list's clicks before laying its browser page out. F5 lists the
//! folder again (`BrowserState::relist`), for a file written there behind the dialog's back, and
//! leaves the key unconsumed; natively only, since in a web page F5 is the browser's own reload.
//!
//! Create Folder asks for the folder's name in the page too ("Folder Name", Create and Cancel)
//! and makes it in the showing folder, recorded for Undo, then opens it.
//!
//! The page is built once; the note and the buttons are built again when the folder or the
//! question changes, so the name field keeps its text, caret and keyboard.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexRow, Label, Rect, Size, SizedBox, TextField, Widget,
};
use agg_gui_files::tr;

use crate::dialogs::{ReplaceQuestion, SavePick, SaveRequest, SaveTarget};
use crate::move_dialog::BrowserPage;
use crate::sheet_dialogs::{CANCEL_BUTTON, CONFIRM_BUTTON};
use crate::state::{BrowserState, ItemEvent};

/// The Save page's widget name.
pub const SAVE_AS_WINDOW: &str = "Save As Window";
/// The name field.
pub const SAVE_NAME_FIELD: &str = "Save Name Field";
/// The note under the field (why Save can't run, or the question asked).
pub const SAVE_NOTE: &str = "Save As Note";
/// The "Create Folder" button.
pub const CREATE_FOLDER_BUTTON: &str = "Create Folder In Button";
/// The new folder's name field.
pub const FOLDER_NAME_FIELD: &str = "Save As Folder Name Field";

/// What the page answers through: a pick, or a cancel.
pub struct SaveAnswers {
    pub accept: Rc<dyn Fn(SavePick)>,
    pub cancel: Rc<dyn Fn()>,
}

/// What the page's bottom shows.
#[derive(Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    /// Asking `question` before saving as `file_name`.
    Ask {
        question: ReplaceQuestion,
        file_name: String,
    },
    /// Asking the new folder's name.
    NewFolder,
}

/// What the page's buttons share.
struct Core {
    state: Rc<RefCell<BrowserState>>,
    request: SaveRequest,
    answers: SaveAnswers,
    name: Rc<RefCell<String>>,
    folder_name: Rc<RefCell<String>>,
    mode: RefCell<Mode>,
    note: RefCell<String>,
    accepted: Rc<Cell<bool>>,
    /// Bumped whenever the bottom must be built again.
    changes: Cell<u64>,
}

impl Core {
    fn changed(&self) {
        self.changes.set(self.changes.get() + 1);
        agg_gui::animation::request_layout();
    }

    /// Whether the showing folder takes files.
    fn can_save_here(&self) -> bool {
        let s = self.state.borrow();
        s.depth() > 1
            && s.active_container()
                .is_some_and(|c| c.as_writable().is_some())
    }

    /// Save, as the module header says: the typed name named, then the app's question or the
    /// dialog's own asked, or the save handed over.
    fn save(&self) {
        if self.accepted.get() {
            return;
        }
        if !self.can_save_here() {
            *self.note.borrow_mut() = tr("Choose a folder you can save in.");
            return self.changed();
        }
        let typed = self.name.borrow().trim().to_string();
        if typed.is_empty() {
            return;
        }
        let file_name = match (self.request.namer)(&typed) {
            Ok(name) => name,
            Err(why) => {
                *self.note.borrow_mut() = why;
                return self.changed();
            }
        };
        let exists = self.exists(&file_name);
        let question = self.app_question(&file_name, exists).or_else(|| {
            exists
                .then(|| ReplaceQuestion::replace_existing(&(self.request.shown_name)(&file_name)))
        });
        match question {
            Some(question) => {
                *self.mode.borrow_mut() = Mode::Ask {
                    question,
                    file_name,
                };
                self.changed();
            }
            None => self.finish(file_name),
        }
    }

    /// Whether the showing folder lists a file named `file_name`.
    fn exists(&self, file_name: &str) -> bool {
        self.state
            .borrow()
            .active_container()
            .is_some_and(|c| c.items().iter().any(|i| i.name() == file_name))
    }

    /// The request's own question before saving as `file_name`, if it has one: its hook handed
    /// the showing folder with the browser's state let go.
    fn app_question(&self, file_name: &str, exists: bool) -> Option<ReplaceQuestion> {
        let hook = self.request.replace_question.as_ref()?;
        let folder = self.state.borrow().open_path().last().cloned()?;
        let folder = folder.borrow();
        hook(&SaveTarget {
            folder: folder.as_ref(),
            file_name,
            exists,
        })
    }

    /// Hands the showing folder over with `file_name` (the user has agreed to any question).
    fn finish(&self, file_name: String) {
        if self.accepted.get() || !self.can_save_here() {
            return;
        }
        // A folder something else still holds stays showing, so the user can try again there.
        if !self.state.borrow().can_hand_over_active_container() {
            return self.busy();
        }
        let exists = self.exists(&file_name);
        self.accepted.set(true);
        let taken = self.state.borrow_mut().take_active_container();
        match taken.map(Rc::try_unwrap) {
            Some(Ok(folder)) => (self.answers.accept)(SavePick {
                folder: folder.into_inner(),
                file_name,
                replaces: exists,
            }),
            _ => {
                self.accepted.set(false);
                self.busy();
            }
        }
    }

    /// Nothing was saved because the folder was held: back to the page, saying so.
    fn busy(&self) {
        *self.mode.borrow_mut() = Mode::Normal;
        *self.note.borrow_mut() = tr("The folder was busy, so nothing was saved. Try again.");
        self.changed();
    }

    /// The question's confirming button: saves under the name it asked about.
    fn confirm_question(&self) {
        let asked = match &*self.mode.borrow() {
            Mode::Ask { file_name, .. } => Some(file_name.clone()),
            _ => None,
        };
        if let Some(file_name) = asked {
            self.finish(file_name);
        }
    }

    /// Back to the page from a question or the new folder's name.
    fn back(&self) {
        *self.mode.borrow_mut() = Mode::Normal;
        self.changed();
    }

    /// The list's clicks and double clicks since the last layout: a file's name fills the field
    /// where the folder takes the save, and a double click saves.
    fn apply_item_events(&self) {
        let events = self.state.borrow_mut().take_item_events();
        for event in events {
            if !self.can_save_here() {
                continue;
            }
            let (entry, save) = match event {
                ItemEvent::Clicked(entry) => (entry, false),
                ItemEvent::Activated(entry) => (entry, true),
            };
            *self.name.borrow_mut() = (self.request.shown_name)(entry.name());
            self.note.borrow_mut().clear();
            *self.mode.borrow_mut() = Mode::Normal;
            self.changed();
            if save {
                self.save();
            }
        }
    }

    /// Makes the folder named in the folder field here, then opens it.
    fn create_folder(&self) {
        let name = self.folder_name.borrow().trim().to_string();
        if name.is_empty() {
            return;
        }
        let depth = self.state.borrow().depth() - 1;
        let made =
            BrowserState::write_recorded(&self.state, depth, |w| w.create_folder_recorded(&name));
        *self.mode.borrow_mut() = Mode::Normal;
        self.folder_name.borrow_mut().clear();
        if made.is_ok() {
            let link = {
                let s = self.state.borrow();
                s.active_container()
                    .and_then(|c| c.child_containers().into_iter().find(|l| l.name() == name))
            };
            if let Some(link) = link {
                self.state.borrow_mut().open(link.as_ref());
            }
        }
        self.changed();
    }
}

/// The Save dialog's page (see the module header).
pub struct SavePage {
    core: Rc<Core>,
    font: Arc<Font>,
    built: Option<(u64, u64)>,
    bounds: Rect,
    /// The heading, the browser page, the name label, the name field, the note, the buttons.
    children: Vec<Box<dyn Widget>>,
}

impl SavePage {
    /// The page saving per `request`, browsing `state`, answering through `answers`; with a
    /// `location_key`, opening where it was last left and remembering where it goes.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        request: SaveRequest,
        answers: SaveAnswers,
        font: Arc<Font>,
        location_key: Option<&str>,
    ) -> Self {
        let mut page = BrowserPage::new(state, Arc::clone(&font));
        if let Some(key) = location_key {
            page = page.remembering(key);
        }
        let name = Rc::new(RefCell::new(request.initial_name.clone()));
        let core = Rc::new(Core {
            state: Rc::clone(state),
            accepted: page.accepted_flag(),
            name: Rc::clone(&name),
            folder_name: Rc::new(RefCell::new(String::new())),
            mode: RefCell::new(Mode::Normal),
            note: RefCell::new(String::new()),
            changes: Cell::new(0),
            request,
            answers,
        });
        let heading = Label::new(core.request.title.clone(), Arc::clone(&font))
            .with_font_size(16.0)
            .with_strong(true);
        let label = Label::new(core.request.name_label.clone(), Arc::clone(&font));
        let saving = Rc::clone(&core);
        let field = TextField::new(Arc::clone(&font))
            .with_text_cell(name)
            .with_select_all_on_focus(true)
            .on_change({
                let core = Rc::clone(&core);
                move |_| {
                    // A new name is a new save: the note and any question were about the old.
                    let asking = matches!(*core.mode.borrow(), Mode::Ask { .. });
                    if asking {
                        *core.mode.borrow_mut() = Mode::Normal;
                    }
                    if asking || !core.note.borrow().is_empty() {
                        core.note.borrow_mut().clear();
                        core.changed();
                    }
                }
            })
            .on_enter(move |_| saving.save())
            .with_name(SAVE_NAME_FIELD);
        Self {
            core,
            font,
            built: None,
            bounds: Rect::default(),
            children: vec![
                Box::new(heading),
                Box::new(page),
                Box::new(label),
                Box::new(field),
                Box::new(SizedBox::new()),
                Box::new(SizedBox::new()),
            ],
        }
    }

    fn button(&self, label: String, name: &str, run: impl Fn(&Core) + 'static) -> Button {
        let core = Rc::clone(&self.core);
        Button::new(label, Arc::clone(&self.font))
            .on_click(move || run(&core))
            .with_name(name)
    }

    /// The note and the buttons, for the folder and the question now.
    fn rebuild(&mut self) {
        let mode = self.core.mode.borrow().clone();
        let (note, mut row) = match &mode {
            Mode::Normal => {
                let can = self.core.can_save_here();
                let note = match self.core.note.borrow().clone() {
                    n if n.is_empty() && !can => tr("Choose a folder you can save in."),
                    n => n,
                };
                let create = self.button(tr("Create Folder"), CREATE_FOLDER_BUTTON, |c| {
                    *c.mode.borrow_mut() = Mode::NewFolder;
                    c.changed();
                });
                let cancel = self.button(tr("Cancel"), CANCEL_BUTTON, |c| (c.answers.cancel)());
                let save = self
                    .button(tr("Save"), CONFIRM_BUTTON, Core::save)
                    .with_enabled_fn(move || can);
                let row = FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(create.with_enabled_fn(move || can)))
                    .add_flex(Box::new(SizedBox::new().with_height(1.0)), 1.0)
                    .add(Box::new(cancel.with_cancel_action()))
                    .add(Box::new(save.with_default_action()));
                (note, row)
            }
            Mode::Ask { question, .. } => {
                let note = format!("{}: {}", question.title, question.message);
                let back = self
                    .button(question.cancel_label.clone(), CANCEL_BUTTON, Core::back)
                    .with_cancel_action();
                let mut row = FlexRow::new()
                    .with_gap(8.0)
                    .add_flex(Box::new(SizedBox::new().with_height(1.0)), 1.0);
                row = match &question.confirm_label {
                    Some(label) => {
                        let confirm =
                            self.button(label.clone(), CONFIRM_BUTTON, Core::confirm_question);
                        row.add(Box::new(back))
                            .add(Box::new(confirm.with_default_action()))
                    }
                    // Going back is all there is: Enter does it too.
                    None => row.add(Box::new(back.with_default_action())),
                };
                (note, row)
            }
            Mode::NewFolder => {
                let creating = Rc::clone(&self.core);
                let field = TextField::new(Arc::clone(&self.font))
                    .with_text_cell(Rc::clone(&self.core.folder_name))
                    .with_placeholder(tr("Folder Name"))
                    .on_enter(move |_| creating.create_folder())
                    .with_name(FOLDER_NAME_FIELD);
                let back = self.button(tr("Cancel"), CANCEL_BUTTON, Core::back);
                let create = self.button(tr("Create"), CONFIRM_BUTTON, Core::create_folder);
                let row = FlexRow::new()
                    .with_gap(8.0)
                    .add_flex(Box::new(field), 1.0)
                    .add(Box::new(back.with_cancel_action()))
                    .add(Box::new(create.with_default_action()));
                (String::new(), row)
            }
        };
        row = row.with_padding(0.0);
        self.children[4] = Box::new(
            Label::new(note, Arc::clone(&self.font))
                .with_wrap(true)
                .with_name(SAVE_NOTE),
        );
        self.children[5] = Box::new(row);
    }
}

impl Widget for SavePage {
    fn type_name(&self) -> &'static str {
        "SavePage"
    }
    fn id(&self) -> Option<&str> {
        Some(SAVE_AS_WINDOW)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    /// 16 of padding; the heading at the top; the label, field, note and buttons at the bottom;
    /// the browser between, 8 apart.
    fn layout(&mut self, available: Size) -> Size {
        self.core.apply_item_events();
        let key = (
            self.core.state.borrow().navigation(),
            self.core.changes.get(),
        );
        if self.built != Some(key) {
            self.rebuild();
            self.built = Some(key);
        }
        const PAD: f64 = 16.0;
        const GAP: f64 = 8.0;
        let width = (available.width - 2.0 * PAD).max(0.0);
        let heading = self.children[0].layout(Size::new(width, available.height));
        let top = available.height - PAD - heading.height;
        self.children[0].set_bounds(Rect::new(PAD, top, width, heading.height));
        let mut bottom = PAD;
        for i in [5, 4, 3, 2] {
            let size = self.children[i].layout(Size::new(width, available.height));
            self.children[i].set_bounds(Rect::new(PAD, bottom, width, size.height));
            bottom += size.height + GAP;
        }
        let room = Size::new(width, (top - GAP - bottom).max(0.0));
        self.children[1].layout(room);
        self.children[1].set_bounds(Rect::new(PAD, bottom, room.width, room.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    /// F5 lists the folder again, natively (see the module header); the key goes on.
    fn on_event(&mut self, event: &Event) -> EventResult {
        #[cfg(not(target_arch = "wasm32"))]
        if let Event::KeyDown {
            key: agg_gui::Key::F(5),
            ..
        } = event
        {
            self.core.state.borrow_mut().relist();
        }
        let _ = event;
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        let state = self.core.state.borrow();
        let mode = match &*self.core.mode.borrow() {
            Mode::Normal => "normal".to_string(),
            Mode::Ask { file_name, .. } => format!("replace {file_name}"),
            Mode::NewFolder => "new folder".to_string(),
        };
        vec![
            ("bread_crumb_trail", state.path_names().join("/")),
            ("name", self.core.name.borrow().clone()),
            ("question", mode),
            ("can_save", self.core.can_save_here().to_string()),
        ]
    }
}
