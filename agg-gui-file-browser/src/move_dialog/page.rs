//! The dialog pages' browser and the Move dialog's page.
//!
//! [`BrowserPage`] is the body a dialog that picks a place browses with: a nav bar (named
//! `BrowserNames::page_nav_bar`: the breadcrumbs, then the view button without "Show Folders"
//! and the sorting button) over a list of a browser of its own, built again when the folder
//! changes (the list, and its scroll, kept). [`dialog_options`] are such a browser's options:
//! the list alone, 75-unit pictures, read-only folders hidden (nothing can go in them), item
//! double clicks kept for the page (they do nothing here), and a folder opened by a double click
//! or one tap of a finger. The mouse's back button goes up a folder. A page given a location key
//! ([`BrowserPage::remembering`]) opens where the config's memory says it was last left under
//! that key and remembers each folder the user moves to, until the page is accepted (its folder
//! then handed over, never remembered as the page's place).
//!
//! [`MovePage`] is the Move dialog: "Select a new Destination:", a line saying what accepting
//! will do as the user browses and selects ("Move 11 items to Games." / "Copy ... The originals
//! stay in Downloads." / why the folder can't take them: `plan.rs`), the browser page, then
//! Cancel and Move or Copy as the line says (disabled while the folder can't take the items).
//! Accepting opens the destination (the selected folder row, or hands over the folder showing)
//! and answers with it; the browser's Move does the move.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexRow, Insets, Label, MouseButton, Rect, Size, SizedBox,
    Widget,
};
use agg_gui_files::tr;

use super::plan::{destination, move_verdict, Destination, MoveVerdict};
use crate::bread_crumb::folder_bread_crumb_for;
use crate::browser::parts::BottomBorder;
use crate::browser::BACK_MOUSE_BUTTON;
use crate::config::{BrowserLayout, BrowserOptions, DoubleClickTarget};
use crate::dialogs::{MovePick, MoveRequest};
use crate::list::BrowserList;
use crate::nav::buttons::{sorting_button, view_options_button};
use crate::sheet_dialogs::{CANCEL_BUTTON, CONFIRM_BUTTON};
use crate::state::BrowserState;

/// The Move page's widget name.
pub const MOVE_ITEM_WINDOW: &str = "Move Item Window";
/// The line saying what accepting will do.
pub const MOVE_ITEM_SUMMARY: &str = "Move Item Summary";
/// The picture side a dialog's list opens on.
pub const DIALOG_THUMB_SIZE: i32 = 75;

/// The options of a dialog's browser (see the module header).
pub fn dialog_options() -> BrowserOptions {
    BrowserOptions {
        layout: BrowserLayout::ListOnly,
        double_click: DoubleClickTarget::Host,
        finger_tap_opens_folders: true,
        folder_filter: Some(Rc::new(|link| !link.is_read_only())),
        thumb_size: DIALOG_THUMB_SIZE,
        ..BrowserOptions::default()
    }
}

/// A dialog's browser: its nav bar over its list (see the module header).
pub struct BrowserPage {
    state: Rc<RefCell<BrowserState>>,
    font: Arc<Font>,
    /// The memory key the page's place is remembered under, if any.
    location_key: Option<String>,
    /// Set once the page is accepted: from then on its moves are not remembered.
    accepted: Rc<Cell<bool>>,
    built: Option<u64>,
    bounds: Rect,
    /// The nav bar, then the list.
    children: Vec<Box<dyn Widget>>,
}

impl BrowserPage {
    /// The page over the browser `state`, drawn in `font`.
    pub fn new(state: &Rc<RefCell<BrowserState>>, font: Arc<Font>) -> Self {
        let list = BrowserList::new(state, Arc::clone(&font));
        Self {
            state: Rc::clone(state),
            font,
            location_key: None,
            accepted: Rc::new(Cell::new(false)),
            built: None,
            bounds: Rect::default(),
            children: vec![Box::new(SizedBox::new()), Box::new(list)],
        }
    }

    /// The page opening where it was last left under `key`, and remembering where it goes.
    pub fn remembering(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        BrowserState::restore_location(&self.state, &key);
        self.location_key = Some(key);
        self
    }

    /// Set to stop remembering the page's place (the page's accept, which hands its folder
    /// over).
    pub fn accepted_flag(&self) -> Rc<Cell<bool>> {
        Rc::clone(&self.accepted)
    }

    fn nav_bar(&self) -> Box<dyn Widget> {
        let (names, look) = {
            let s = self.state.borrow();
            (s.path_names(), Rc::clone(&s.config().look))
        };
        let weak = Rc::downgrade(&self.state);
        let go_to: Rc<dyn Fn(usize)> = Rc::new(move |depth| {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().go_to(depth);
            }
        });
        let crumbs = folder_bread_crumb_for(&names, go_to, &look, &self.font);
        let row = FlexRow::new()
            .with_gap(4.0)
            .with_inner_padding(Insets::symmetric(3.0, 1.0))
            .add_flex(Box::new(crumbs), 1.0)
            .add(Box::new(view_options_button(
                &self.state,
                &self.font,
                false,
            )))
            .add(Box::new(sorting_button(&self.state, &self.font)));
        let bar = BottomBorder::new(Box::new(row), look.style().border_color);
        Box::new(bar.with_name(look.names().page_nav_bar.clone()))
    }
}

impl Widget for BrowserPage {
    fn type_name(&self) -> &'static str {
        "BrowserPage"
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
    fn layout(&mut self, available: Size) -> Size {
        // Item double clicks are the page's, and do nothing here.
        let _ = self.state.borrow_mut().take_item_events();
        let navigation = self.state.borrow().navigation();
        if self.built != Some(navigation) {
            // A move by the user (not the first build): remember where the page is.
            if let Some(key) = self.location_key.as_ref().filter(|_| self.built.is_some()) {
                if !self.accepted.get() {
                    BrowserState::remember_location(&self.state, key);
                }
            }
            self.children[0] = self.nav_bar();
            self.built = Some(navigation);
        }
        let bar = self.children[0].layout(available);
        self.children[0].set_bounds(Rect::new(
            0.0,
            available.height - bar.height,
            available.width,
            bar.height,
        ));
        let room = Size::new(available.width, (available.height - bar.height).max(0.0));
        self.children[1].layout(room);
        self.children[1].set_bounds(Rect::new(0.0, 0.0, room.width, room.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    /// The mouse's back button goes up one folder.
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown {
            button: MouseButton::Other(BACK_MOUSE_BUTTON),
            ..
        } = event
        {
            let depth = self.state.borrow().depth();
            if depth > 1 {
                self.state.borrow_mut().go_to(depth - 2);
            }
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

/// What the Move page answers through: a pick, or a cancel.
pub struct MoveAnswers {
    pub accept: Rc<dyn Fn(MovePick)>,
    pub cancel: Rc<dyn Fn()>,
}

/// The Move dialog's page (see the module header).
pub struct MovePage {
    state: Rc<RefCell<BrowserState>>,
    request: Rc<MoveRequest>,
    answers: Rc<MoveAnswers>,
    /// The browser page's: set on accept, so the folder handed over isn't remembered.
    accepted: Rc<Cell<bool>>,
    font: Arc<Font>,
    verdict: Rc<RefCell<MoveVerdict>>,
    built: Option<u64>,
    bounds: Rect,
    /// The heading, the summary, the browser page, the buttons.
    children: Vec<Box<dyn Widget>>,
}

impl MovePage {
    /// The page moving `request`'s items, browsing `state`, answering through `answers`; with a
    /// `location_key`, opening where it was last left and remembering where it goes
    /// (`BrowserPage::remembering`).
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        request: MoveRequest,
        answers: MoveAnswers,
        font: Arc<Font>,
        location_key: Option<&str>,
    ) -> Self {
        let heading = Label::new(
            format!("{}:", tr("Select a new Destination")),
            Arc::clone(&font),
        )
        .with_font_size(16.0)
        .with_strong(true);
        let mut page = BrowserPage::new(state, Arc::clone(&font));
        if let Some(key) = location_key {
            page = page.remembering(key);
        }
        let accepted = page.accepted_flag();
        Self {
            state: Rc::clone(state),
            request: Rc::new(request),
            answers: Rc::new(answers),
            accepted,
            font,
            verdict: Rc::new(RefCell::new(MoveVerdict::Refused(String::new()))),
            built: None,
            bounds: Rect::default(),
            children: vec![
                Box::new(heading),
                Box::new(SizedBox::new()),
                Box::new(page),
                Box::new(SizedBox::new()),
            ],
        }
    }

    /// What accepting does now.
    pub fn verdict(&self) -> MoveVerdict {
        self.verdict.borrow().clone()
    }

    /// The summary and the buttons, for the verdict now.
    fn rebuild(&mut self) {
        let verdict = move_verdict(&self.request, &self.state.borrow());
        let summary = Label::new(verdict.text().to_string(), Arc::clone(&self.font))
            .with_wrap(true)
            .with_name(MOVE_ITEM_SUMMARY);
        let cancel = Rc::clone(&self.answers);
        let cancel_button = Button::new(tr("Cancel"), Arc::clone(&self.font))
            .on_click(move || (cancel.cancel)())
            .with_cancel_action()
            .with_name(CANCEL_BUTTON);
        let (state, request, answers) = (
            Rc::clone(&self.state),
            Rc::clone(&self.request),
            Rc::clone(&self.answers),
        );
        let accepted = Rc::clone(&self.accepted);
        let accept_button = Button::new(verdict.verb(), Arc::clone(&self.font))
            .on_click(move || accept(&state, &request, &answers, &accepted))
            .with_enabled_fn({
                let enabled = !verdict.is_refused();
                move || enabled
            })
            .with_default_action()
            .with_name(CONFIRM_BUTTON);
        let buttons = FlexRow::new()
            .with_gap(8.0)
            .add_flex(Box::new(SizedBox::new().with_height(1.0)), 1.0)
            .add(Box::new(cancel_button))
            .add(Box::new(accept_button));
        self.children[1] = Box::new(summary);
        self.children[3] = Box::new(buttons);
        *self.verdict.borrow_mut() = verdict;
    }
}

/// Accepts the dialog when its folder can take the items: opens the destination and answers.
fn accept(
    state: &Rc<RefCell<BrowserState>>,
    request: &MoveRequest,
    answers: &MoveAnswers,
    accepted: &Cell<bool>,
) {
    let verdict = move_verdict(request, &state.borrow());
    if verdict.is_refused() || accepted.get() {
        return;
    }
    let copy = matches!(verdict, MoveVerdict::Copy(_));
    let destination = destination(&state.borrow());
    let target = match destination {
        Destination::Row(link) => match link.get_container() {
            Ok(target) => target,
            Err(why) => {
                state.borrow_mut().after_write(why);
                return;
            }
        },
        Destination::Showing => {
            // The folder showing is handed over: the dialog closes on accept, and the page stops
            // remembering its place first. Should something still hold the folder, nothing
            // moves and the user is told so.
            accepted.set(true);
            let Some(taken) = state.borrow_mut().take_active_container() else {
                accepted.set(false);
                return;
            };
            match Rc::try_unwrap(taken) {
                Ok(folder) => folder.into_inner(),
                Err(_) => {
                    accepted.set(false);
                    let text = tr("The folder was busy, so nothing moved. Try again.");
                    state.borrow_mut().after_write(text);
                    return;
                }
            }
        }
    };
    accepted.set(true);
    (answers.accept)(MovePick { target, copy });
}

impl Widget for MovePage {
    fn type_name(&self) -> &'static str {
        "MovePage"
    }
    fn id(&self) -> Option<&str> {
        Some(MOVE_ITEM_WINDOW)
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
    /// 16 of padding; the heading and summary at the top, the buttons at the bottom, the browser
    /// between, 8 apart.
    fn layout(&mut self, available: Size) -> Size {
        let generation = self.state.borrow().generation();
        if self.built != Some(generation) {
            self.rebuild();
            self.built = Some(generation);
        }
        const PAD: f64 = 16.0;
        const GAP: f64 = 8.0;
        let width = (available.width - 2.0 * PAD).max(0.0);
        let mut top = available.height - PAD;
        for i in [0, 1] {
            let size = self.children[i].layout(Size::new(width, top));
            top -= size.height;
            self.children[i].set_bounds(Rect::new(PAD, top, width, size.height));
            top -= GAP;
        }
        let buttons = self.children[3].layout(Size::new(width, top));
        self.children[3].set_bounds(Rect::new(PAD, PAD, width, buttons.height));
        let bottom = PAD + buttons.height + GAP;
        let room = Size::new(width, (top - bottom).max(0.0));
        self.children[2].layout(room);
        self.children[2].set_bounds(Rect::new(PAD, bottom, room.width, room.height));
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        let verdict = self.verdict.borrow();
        vec![
            (
                "bread_crumb_trail",
                self.state.borrow().path_names().join("/"),
            ),
            ("accept_enabled", (!verdict.is_refused()).to_string()),
            ("summary", verdict.text().to_string()),
            ("accept_text", verdict.verb()),
        ]
    }
}
