//! [`OpenPage`]: the Open dialog's page, for an app with no system Open dialog of its own. Its
//! heading (the request's title), a browser page of its own (`move_dialog/page.rs`'s
//! [`BrowserPage`], listing every folder, read-only ones too, and only the files the request's
//! filter keeps; one file selectable at a time unless the request allows several; opening where
//! it was last left when given a location key), a line saying what Open will open, then Cancel
//! and Open.
//!
//! The line names the selected files, or asks for one ("Choose a file to open.", "Choose files to
//! open." where several can be picked). Open is enabled while files are selected, answering with
//! them (in the order they were selected) and the folder listing them; with only one folder
//! selected it opens that folder instead, as a double click on it does. A double click on a file
//! opens the selection it belongs to. Reading the files is the app's.
//!
//! What Open does is [`OpenController`]'s, so an app can wrap it in its own page; the page is its
//! default chrome, its buttons named by the look's `BrowserNames::dialog_cancel` and
//! `dialog_confirm`. The page has the controller take the list's double clicks before laying its
//! browser page out; its line and buttons are built again whenever the browser's view changes (a
//! selection, a folder).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{Button, DrawCtx, Event, EventResult, FlexRow, Label, Rect, Size, SizedBox, Widget};
use agg_gui_files::{tr, FolderLink, Item};

use crate::dialogs::{OpenPick, OpenRequest};
use crate::memory::LocationFuture;
use crate::move_dialog::place::DialogPlace;
use crate::move_dialog::BrowserPage;
use crate::state::{BrowserEntry, BrowserState, ItemEvent};

/// The Open page's widget name.
pub const OPEN_WINDOW: &str = "Open Window";
/// The line saying what Open will open.
pub const OPEN_SUMMARY: &str = "Open Summary";

/// What the page answers through: a pick, or a cancel.
pub struct OpenAnswers {
    pub accept: Rc<dyn Fn(OpenPick)>,
    pub cancel: Rc<dyn Fn()>,
}

/// What Open does with the selection now.
enum Choice {
    /// Answers with these files.
    Files(Vec<Rc<dyn Item>>),
    /// Opens this folder in the page.
    Folder(Rc<dyn FolderLink>),
    /// Nothing to open.
    Nothing,
}

/// What an Open dialog's controller shares.
struct Core {
    state: Rc<RefCell<BrowserState>>,
    request: OpenRequest,
    answers: OpenAnswers,
    place: DialogPlace,
}

/// The Open dialog's logic without its widgets, so an app can wrap it in its own page; clones
/// share one dialog. It says what Open does with the selection ([`OpenController::summary`],
/// [`OpenController::accept_enabled`]), opens (answering with the selected files, or opening
/// the one folder selected), takes the list's double clicks in [`OpenController::update`] (which
/// the page calls at each layout before its browser lays out) and keeps the dialog's place
/// (`move_dialog`'s `place.rs`).
#[derive(Clone)]
pub struct OpenController {
    core: Rc<Core>,
}

impl OpenController {
    /// The dialog opening per `request`, browsing `state` (made with the request's filter and
    /// selection), answering through `answers`.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        request: OpenRequest,
        answers: OpenAnswers,
    ) -> Self {
        Self {
            core: Rc::new(Core {
                state: Rc::clone(state),
                request,
                answers,
                place: DialogPlace::default(),
            }),
        }
    }

    /// The dialog opening where it was last left under `key` (unless given a starting folder)
    /// and remembering where the user goes.
    pub fn remembering(self, key: impl Into<String>) -> Self {
        self.core.place.remember_under(key);
        self
    }

    /// The dialog opening in the folders `start` finds below the root.
    pub fn starting_in(self, start: LocationFuture) -> Self {
        self.core.place.start_in(start);
        self
    }

    /// The dialog's browser.
    pub fn state(&self) -> &Rc<RefCell<BrowserState>> {
        &self.core.state
    }

    /// The request the dialog answers.
    pub fn request(&self) -> &OpenRequest {
        &self.core.request
    }

    /// What Open does with `selected` (see the module header).
    fn choice(selected: &[BrowserEntry]) -> Choice {
        let files: Vec<Rc<dyn Item>> = selected
            .iter()
            .filter_map(|entry| match entry {
                BrowserEntry::Item(item) => Some(Rc::clone(item)),
                BrowserEntry::Container(_) => None,
            })
            .collect();
        match (files.is_empty(), selected) {
            (false, _) => Choice::Files(files),
            (true, [BrowserEntry::Container(link)]) => Choice::Folder(Rc::clone(link)),
            (true, _) => Choice::Nothing,
        }
    }

    fn current(&self) -> Choice {
        Self::choice(&self.core.state.borrow().selected_entries())
    }

    /// The line saying what Open will open: the selected files' names, or what to choose.
    pub fn summary(&self) -> String {
        match self.current() {
            Choice::Files(files) => {
                let names: Vec<&str> = files.iter().map(|f| f.name()).collect();
                names.join(", ")
            }
            Choice::Folder(_) | Choice::Nothing => match self.core.request.multiple {
                true => tr("Choose files to open."),
                false => tr("Choose a file to open."),
            },
        }
    }

    /// Whether Open does anything with the selection now.
    pub fn accept_enabled(&self) -> bool {
        !matches!(self.current(), Choice::Nothing)
    }

    /// The Open button: answers with the selected files, or opens the one folder selected.
    pub fn accept(&self) {
        match self.current() {
            Choice::Files(files) => self.answer(files),
            Choice::Folder(link) => self.core.state.borrow_mut().open(link.as_ref()),
            Choice::Nothing => {}
        }
    }

    /// Answers as cancelled.
    pub fn cancel(&self) {
        (self.core.answers.cancel)();
    }

    /// Answers with `files` and the folder listing them, once.
    fn answer(&self, files: Vec<Rc<dyn Item>>) {
        let accepted = &self.core.place.accepted;
        if accepted.get() {
            return;
        }
        let Some(folder) = self.core.state.borrow().open_path().last().cloned() else {
            return;
        };
        accepted.set(true);
        (self.core.answers.accept)(OpenPick {
            folder,
            items: files,
        });
    }

    /// Opens the dialog's place on the first call and remembers a move by the user after; takes
    /// the list's double clicks since the last call: a file's opens the selection it is in.
    pub fn update(&self) {
        self.core.place.update(&self.core.state);
        let events = self.core.state.borrow_mut().take_item_events();
        for event in events {
            let ItemEvent::Activated(entry) = event else {
                continue;
            };
            let selected = self.core.state.borrow().selected_entries();
            let files = match Self::choice(&selected) {
                Choice::Files(files) if selected.iter().any(|s| s.same_row(&entry)) => files,
                _ => vec![entry.item()],
            };
            self.answer(files);
        }
    }

    /// What a page over the controller reports: `bread_crumb_trail`, `summary` and
    /// `accept_enabled`.
    pub fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "bread_crumb_trail",
                self.core.state.borrow().path_names().join("/"),
            ),
            ("summary", self.summary()),
            ("accept_enabled", self.accept_enabled().to_string()),
        ]
    }
}

/// The Open dialog's page (see the module header).
pub struct OpenPage {
    controller: OpenController,
    font: Arc<Font>,
    built: Option<u64>,
    summary: String,
    can_open: bool,
    bounds: Rect,
    /// The heading, the browser page, the summary, the buttons.
    children: Vec<Box<dyn Widget>>,
}

impl OpenPage {
    /// The page opening per `request`, browsing `state` (made with the request's filter and
    /// selection: `SheetDialogs` does so), answering through `answers`; with a `location_key`,
    /// opening where it was last left and remembering where it goes.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        request: OpenRequest,
        answers: OpenAnswers,
        font: Arc<Font>,
        location_key: Option<&str>,
    ) -> Self {
        let mut controller = OpenController::new(state, request, answers);
        if let Some(key) = location_key {
            controller = controller.remembering(key);
        }
        Self::over(controller, font)
    }

    /// The page over `controller` (an app's starting folder set on it).
    pub fn over(controller: OpenController, font: Arc<Font>) -> Self {
        let page = BrowserPage::new(controller.state(), Arc::clone(&font));
        let heading = Label::new(controller.request().title.clone(), Arc::clone(&font))
            .with_font_size(16.0)
            .with_strong(true);
        Self {
            controller,
            font,
            built: None,
            summary: String::new(),
            can_open: false,
            bounds: Rect::default(),
            children: vec![
                Box::new(heading),
                Box::new(page),
                Box::new(SizedBox::new()),
                Box::new(SizedBox::new()),
            ],
        }
    }

    /// The controller the page shows.
    pub fn controller(&self) -> &OpenController {
        &self.controller
    }

    /// The summary and the buttons, for the selection now.
    fn rebuild(&mut self) {
        let (summary, can_open) = (self.controller.summary(), self.controller.accept_enabled());
        let names = self
            .controller
            .state()
            .borrow()
            .config()
            .look
            .names()
            .clone();
        let cancelling = self.controller.clone();
        let cancel = Button::new(tr("Cancel"), Arc::clone(&self.font))
            .on_click(move || cancelling.cancel())
            .with_cancel_action()
            .with_name(names.dialog_cancel.clone());
        let opening = self.controller.clone();
        let open = Button::new(tr("Open"), Arc::clone(&self.font))
            .on_click(move || opening.accept())
            .with_enabled_fn(move || can_open)
            .with_default_action()
            .with_name(names.dialog_confirm.clone());
        let buttons = FlexRow::new()
            .with_gap(8.0)
            .add_flex(Box::new(SizedBox::new().with_height(1.0)), 1.0)
            .add(Box::new(cancel))
            .add(Box::new(open));
        self.children[2] = Box::new(
            Label::new(summary.clone(), Arc::clone(&self.font))
                .with_wrap(true)
                .with_name(OPEN_SUMMARY),
        );
        self.children[3] = Box::new(buttons);
        self.summary = summary;
        self.can_open = can_open;
    }
}

impl Widget for OpenPage {
    fn type_name(&self) -> &'static str {
        "OpenPage"
    }
    fn id(&self) -> Option<&str> {
        Some(OPEN_WINDOW)
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
    /// 16 of padding; the heading at the top; the summary and buttons at the bottom; the browser
    /// between, 8 apart.
    fn layout(&mut self, available: Size) -> Size {
        self.controller.update();
        let generation = self.controller.state().borrow().generation();
        if self.built != Some(generation) {
            self.rebuild();
            self.built = Some(generation);
        }
        const PAD: f64 = 16.0;
        const GAP: f64 = 8.0;
        let width = (available.width - 2.0 * PAD).max(0.0);
        let heading = self.children[0].layout(Size::new(width, available.height));
        let top = available.height - PAD - heading.height;
        self.children[0].set_bounds(Rect::new(PAD, top, width, heading.height));
        let mut bottom = PAD;
        for i in [3, 2] {
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
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            (
                "bread_crumb_trail",
                self.controller.state().borrow().path_names().join("/"),
            ),
            ("summary", self.summary.clone()),
            ("accept_enabled", self.can_open.to_string()),
        ]
    }
}
