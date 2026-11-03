//! [`SavePage`]: the Save dialog's default page over a [`SaveController`] (`save_dialog/controller.rs`,
//! which holds what saving does: the name, whether the folder takes the save, the questions,
//! Create Folder, the remembered place). An app wanting its own page (its heading, field,
//! buttons and footer) builds it over the controller instead.
//!
//! The page: its heading (the request's title), a browser page of its own (`move_dialog/page.rs`'s
//! [`BrowserPage`]: read-only folders hidden, so nothing is offered that can't take the file), the
//! name field (the request's label over it, sharing the controller's name, with the request's
//! placeholder and focus id, Enter saving), a note, and the buttons: "Create Folder", Cancel and
//! Save, both enabled while the folder takes the save. The note is the controller's (a name the
//! namer refused, a busy folder) or, without one, why the folder takes no save ("Choose a folder
//! you can save in."). A question the controller asks in the page shows as the note
//! ("Confirm Save As: ...") with its Cancel and confirming buttons; a question with no confirming
//! button only goes back. Create Folder asks the folder's name in the page ("Folder Name", Create
//! and Cancel) unless the controller asks through the app's dialogs. The widgets are named by
//! the look's `BrowserNames` (`save_name_field`, `save_note`, `create_folder_button`,
//! `folder_name_field`, `dialog_confirm`, `dialog_cancel`).
//!
//! F5 lists the folder again (`BrowserState::relist`), for a file written there behind the
//! dialog's back, and leaves the key unconsumed; natively only, since in a web page F5 is the
//! browser's own reload. The page hears it as a key nothing focused took (`on_unconsumed_key`),
//! whatever has the keyboard: the name field, nothing (the dialog's list never takes it), or
//! something else; agg-gui's `ModalSheet` offers such keys to its content before swallowing them.
//!
//! The page is built once; the note and the buttons are built again when the folder or the
//! controller's mode or note changes, so the name field keeps its text, caret and keyboard.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexRow, Label, Modifiers, Rect, Size, SizedBox,
    TextField, Widget,
};
use agg_gui_files::tr;

use crate::dialogs::SaveRequest;
use crate::move_dialog::BrowserPage;
use crate::state::BrowserState;

pub mod controller;

pub use controller::{QuestionHost, SaveAnswers, SaveController, SaveFolderRule, SaveMode};

/// The Save page's widget name.
pub const SAVE_AS_WINDOW: &str = "Save As Window";
/// The name field's default name (`BrowserNames::save_name_field`).
pub const SAVE_NAME_FIELD: &str = "Save Name Field";
/// The note's default name (`BrowserNames::save_note`).
pub const SAVE_NOTE: &str = "Save As Note";
/// The "Create Folder" button's default name (`BrowserNames::create_folder_button`).
pub const CREATE_FOLDER_BUTTON: &str = "Create Folder In Button";
/// The new folder's name field's default name (`BrowserNames::folder_name_field`).
pub const FOLDER_NAME_FIELD: &str = "Save As Folder Name Field";

/// The Save dialog's page (see the module header).
pub struct SavePage {
    controller: SaveController,
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
        let mut controller = SaveController::new(state, request, answers);
        if let Some(key) = location_key {
            controller = controller.remembering(key);
        }
        Self::over(controller, font)
    }

    /// The page over `controller` (an app's own hooks and starting folder set on it).
    pub fn over(controller: SaveController, font: Arc<Font>) -> Self {
        let names = controller.state().borrow().config().look.names().clone();
        let page = BrowserPage::new(controller.state(), Arc::clone(&font));
        let request = controller.request();
        let heading = Label::new(request.title.clone(), Arc::clone(&font))
            .with_font_size(16.0)
            .with_strong(true);
        let label = Label::new(request.name_label.clone(), Arc::clone(&font));
        let (editing, saving) = (controller.clone(), controller.clone());
        let mut field = TextField::new(Arc::clone(&font))
            .with_text_cell(controller.name_cell())
            .with_select_all_on_focus(true)
            .on_change(move |_| editing.edited())
            .on_enter(move |_| saving.accept())
            .with_name(names.save_name_field.clone());
        if let Some(placeholder) = controller.placeholder() {
            field = field.with_placeholder(placeholder.to_string());
        }
        if let Some(id) = request.focus_id {
            field = field.with_focus_id(id);
        }
        Self {
            controller,
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

    /// The controller the page shows.
    pub fn controller(&self) -> &SaveController {
        &self.controller
    }

    fn button(&self, label: String, name: &str, run: impl Fn(&SaveController) + 'static) -> Button {
        let controller = self.controller.clone();
        Button::new(label, Arc::clone(&self.font))
            .on_click(move || run(&controller))
            .with_name(name)
    }

    /// The note and the buttons, for the folder and the question now.
    fn rebuild(&mut self) {
        let names = self
            .controller
            .state()
            .borrow()
            .config()
            .look
            .names()
            .clone();
        let (note, mut row) = match self.controller.mode() {
            SaveMode::Normal => {
                let refusal = self.controller.refusal();
                let can = refusal.is_none();
                let note = match self.controller.note() {
                    n if n.is_empty() => refusal.unwrap_or_default(),
                    n => n,
                };
                let create = self.button(
                    tr("Create Folder"),
                    &names.create_folder_button,
                    SaveController::create_folder,
                );
                let cancel =
                    self.button(tr("Cancel"), &names.dialog_cancel, SaveController::cancel);
                let save = self
                    .button(tr("Save"), &names.dialog_confirm, SaveController::accept)
                    .with_enabled_fn(move || can);
                let row = FlexRow::new()
                    .with_gap(8.0)
                    .add(Box::new(create.with_enabled_fn(move || can)))
                    .add_flex(Box::new(SizedBox::new().with_height(1.0)), 1.0)
                    .add(Box::new(cancel.with_cancel_action()))
                    .add(Box::new(save.with_default_action()));
                (note, row)
            }
            SaveMode::Asking { question, .. } => {
                let note = format!("{}: {}", question.title, question.message);
                let back = self
                    .button(
                        question.cancel_label.clone(),
                        &names.dialog_cancel,
                        SaveController::back,
                    )
                    .with_cancel_action();
                let mut row = FlexRow::new()
                    .with_gap(8.0)
                    .add_flex(Box::new(SizedBox::new().with_height(1.0)), 1.0);
                row = match &question.confirm_label {
                    Some(label) => {
                        let confirm = self.button(
                            label.clone(),
                            &names.dialog_confirm,
                            SaveController::confirm,
                        );
                        row.add(Box::new(back))
                            .add(Box::new(confirm.with_default_action()))
                    }
                    // Going back is all there is: Enter does it too.
                    None => row.add(Box::new(back.with_default_action())),
                };
                (note, row)
            }
            SaveMode::NamingFolder => {
                let creating = self.controller.clone();
                let field = TextField::new(Arc::clone(&self.font))
                    .with_text_cell(self.controller.folder_name_cell())
                    .with_placeholder(tr("Folder Name"))
                    .on_enter(move |_| creating.confirm_folder_name())
                    .with_name(names.folder_name_field.clone());
                let back = self.button(tr("Cancel"), &names.dialog_cancel, SaveController::back);
                let create = self.button(
                    tr("Create"),
                    &names.dialog_confirm,
                    SaveController::confirm_folder_name,
                );
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
                .with_name(names.save_note.clone()),
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
        self.controller.update();
        let key = (
            self.controller.state().borrow().navigation(),
            self.controller.changes(),
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
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    /// F5 lists the folder again, natively (see the module header); the key goes on.
    fn on_unconsumed_key(&mut self, key: &agg_gui::Key, _modifiers: Modifiers) -> EventResult {
        #[cfg(not(target_arch = "wasm32"))]
        if *key == agg_gui::Key::F(5) {
            self.controller.state().borrow_mut().relist();
        }
        let _ = key;
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        let keep = ["bread_crumb_trail", "name", "question", "can_save"];
        let mut properties = self.controller.properties();
        properties.retain(|(key, _)| keep.contains(key));
        properties
    }
}
