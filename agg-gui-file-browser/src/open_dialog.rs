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
//! The page takes the list's double clicks before laying its browser page out; its line and
//! buttons are built again whenever the browser's view changes (a selection, a folder).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{Button, DrawCtx, Event, EventResult, FlexRow, Label, Rect, Size, SizedBox, Widget};
use agg_gui_files::{tr, FolderLink, Item};

use crate::dialogs::{OpenPick, OpenRequest};
use crate::move_dialog::BrowserPage;
use crate::sheet_dialogs::{CANCEL_BUTTON, CONFIRM_BUTTON};
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

/// What the page's buttons share.
struct Core {
    state: Rc<RefCell<BrowserState>>,
    request: OpenRequest,
    answers: OpenAnswers,
    /// The browser page's: set once the page has answered.
    accepted: Rc<Cell<bool>>,
}

impl Core {
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
        Self::choice(&self.state.borrow().selected_entries())
    }

    /// The Open button.
    fn open(&self) {
        match self.current() {
            Choice::Files(files) => self.answer(files),
            Choice::Folder(link) => self.state.borrow_mut().open(link.as_ref()),
            Choice::Nothing => {}
        }
    }

    /// Answers with `files` and the folder listing them, once.
    fn answer(&self, files: Vec<Rc<dyn Item>>) {
        if self.accepted.get() {
            return;
        }
        let Some(folder) = self.state.borrow().open_path().last().cloned() else {
            return;
        };
        self.accepted.set(true);
        (self.answers.accept)(OpenPick {
            folder,
            items: files,
        });
    }

    /// The list's double clicks since the last layout: a file's opens the selection it is in.
    fn apply_item_events(&self) {
        let events = self.state.borrow_mut().take_item_events();
        for event in events {
            let ItemEvent::Activated(entry) = event else {
                continue;
            };
            let selected = self.state.borrow().selected_entries();
            let files = match Self::choice(&selected) {
                Choice::Files(files) if selected.iter().any(|s| s.same_row(&entry)) => files,
                _ => vec![entry.item()],
            };
            self.answer(files);
        }
    }
}

/// The Open dialog's page (see the module header).
pub struct OpenPage {
    core: Rc<Core>,
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
        let mut page = BrowserPage::new(state, Arc::clone(&font));
        if let Some(key) = location_key {
            page = page.remembering(key);
        }
        let heading = Label::new(request.title.clone(), Arc::clone(&font))
            .with_font_size(16.0)
            .with_strong(true);
        let core = Rc::new(Core {
            state: Rc::clone(state),
            accepted: page.accepted_flag(),
            request,
            answers,
        });
        Self {
            core,
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

    /// The summary and the buttons, for the selection now.
    fn rebuild(&mut self) {
        let (summary, can_open) = match self.core.current() {
            Choice::Files(files) => {
                let names: Vec<&str> = files.iter().map(|f| f.name()).collect();
                (names.join(", "), true)
            }
            Choice::Folder(_) => (self.ask_text(), true),
            Choice::Nothing => (self.ask_text(), false),
        };
        let cancelling = Rc::clone(&self.core);
        let cancel = Button::new(tr("Cancel"), Arc::clone(&self.font))
            .on_click(move || (cancelling.answers.cancel)())
            .with_cancel_action()
            .with_name(CANCEL_BUTTON);
        let opening = Rc::clone(&self.core);
        let open = Button::new(tr("Open"), Arc::clone(&self.font))
            .on_click(move || opening.open())
            .with_enabled_fn(move || can_open)
            .with_default_action()
            .with_name(CONFIRM_BUTTON);
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

    /// What the line says while no file is selected.
    fn ask_text(&self) -> String {
        match self.core.request.multiple {
            true => tr("Choose files to open."),
            false => tr("Choose a file to open."),
        }
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
        self.core.apply_item_events();
        let generation = self.core.state.borrow().generation();
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
                self.core.state.borrow().path_names().join("/"),
            ),
            ("summary", self.summary.clone()),
            ("accept_enabled", self.can_open.to_string()),
        ]
    }
}
