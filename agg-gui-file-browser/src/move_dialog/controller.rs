//! [`MoveController`]: the Move dialog's logic without its widgets, so an app can wrap it in its
//! own page (its heading, buttons and names) while [`MovePage`](super::MovePage) is the dialog's
//! default page over it.
//!
//! The controller holds the request, the dialog's browser and its answers. It says what accepting
//! does as the user browses and selects ([`MoveController::verdict`]: `plan.rs`'s `move_verdict`),
//! and accepting opens the destination (the selected folder row, or the folder showing, handed
//! over) and answers with it ([`MovePick`]); the browser's Move does the move. The selected row's
//! place and key are asked once while it stays selected, as a page asks the verdict every frame and
//! asking may make the row's folder. A showing folder something else still holds is not handed
//! over: the dialog stays in it and the browser's status says the folder was busy; a selected row
//! that can't be opened moves nothing and says why. Either message also goes to the app's report
//! (`BrowserConfig::report`) when it gives one, since the app's own page may show no status line.
//! Where the dialog opens and what it remembers of where the user goes is `place.rs`'s: the page
//! calls [`MoveController::update`] at each layout.

use std::cell::RefCell;
use std::rc::Rc;

use super::place::DialogPlace;
use super::plan::{destination, row_facts, verdict_with, Destination, MoveVerdict, RowFacts};
use crate::dialogs::{MovePick, MoveRequest};
use crate::memory::LocationFuture;
use crate::state::BrowserState;
use agg_gui_files::FolderLink;

/// What a Move dialog answers through: a pick, or a cancel.
pub struct MoveAnswers {
    pub accept: Rc<dyn Fn(MovePick)>,
    pub cancel: Rc<dyn Fn()>,
}

struct Core {
    state: Rc<RefCell<BrowserState>>,
    request: MoveRequest,
    answers: MoveAnswers,
    place: DialogPlace,
    /// The selected row's place and key, kept while it stays selected: a row's place and key
    /// don't change while it exists, and asking may make its folder, which every frame's
    /// verdict would otherwise do.
    row: RefCell<Option<(Rc<dyn FolderLink>, RowFacts)>>,
}

/// The Move dialog's logic (see the module header). Clones share one dialog.
#[derive(Clone)]
pub struct MoveController {
    core: Rc<Core>,
}

impl MoveController {
    /// The dialog moving `request`'s items, browsing `state`, answering through `answers`.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        request: MoveRequest,
        answers: MoveAnswers,
    ) -> Self {
        Self {
            core: Rc::new(Core {
                state: Rc::clone(state),
                request,
                answers,
                place: DialogPlace::default(),
                row: RefCell::new(None),
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
    pub fn request(&self) -> &MoveRequest {
        &self.core.request
    }

    /// Opens the dialog's place on the first call, and remembers a move by the user after: the
    /// page calls it at each layout.
    pub fn update(&self) {
        self.core.place.update(&self.core.state);
    }

    /// What accepting does now.
    pub fn verdict(&self) -> MoveVerdict {
        let core = &self.core;
        let facts = |link: &Rc<dyn FolderLink>| {
            if let Some((kept, facts)) = &*core.row.borrow() {
                if Rc::ptr_eq(kept, link) {
                    return facts.clone();
                }
            }
            let facts = row_facts(&core.request, link.as_ref());
            *core.row.borrow_mut() = Some((Rc::clone(link), facts.clone()));
            facts
        };
        verdict_with(&core.request, &core.state.borrow(), &facts)
    }

    /// Whether accepting does anything now.
    pub fn accept_enabled(&self) -> bool {
        !self.verdict().is_refused()
    }

    /// The line saying what accepting will do, or why it can't.
    pub fn summary(&self) -> String {
        self.verdict().text().to_string()
    }

    /// The accepting button's label ("Move", "Copy").
    pub fn accept_text(&self) -> String {
        self.verdict().verb()
    }

    /// Whether the dialog has answered with a pick.
    pub fn is_accepted(&self) -> bool {
        self.core.place.accepted.get()
    }

    /// Accepts when the folder can take the items: opens the destination and answers with it.
    pub fn accept(&self) {
        let core = &self.core;
        let state = &core.state;
        let accepted = &core.place.accepted;
        let verdict = self.verdict();
        if verdict.is_refused() || accepted.get() {
            return;
        }
        let copy = matches!(verdict, MoveVerdict::Copy(_));
        let destination = destination(&state.borrow());
        let target = match destination {
            Destination::Row(link) => match link.get_container() {
                Ok(target) => target,
                Err(why) => {
                    let text = crate::drag::execute::couldnt_move_to(link.name(), &why);
                    return tell(state, text);
                }
            },
            Destination::Showing => {
                // The folder showing is handed over: the dialog closes on accept, and stops
                // remembering its place first. Should something still hold the folder, nothing
                // moves, the dialog stays in it and the user is told so.
                if !state.borrow().can_hand_over_active_container() {
                    return busy(state);
                }
                accepted.set(true);
                let Some(taken) = state.borrow_mut().take_active_container() else {
                    accepted.set(false);
                    return;
                };
                match Rc::try_unwrap(taken) {
                    Ok(folder) => folder.into_inner(),
                    Err(held) => {
                        // Still held after all: back on the path, so the dialog stays in it.
                        state.borrow_mut().put_back_active_container(held);
                        accepted.set(false);
                        return busy(state);
                    }
                }
            }
        };
        accepted.set(true);
        (core.answers.accept)(MovePick { target, copy });
    }

    /// Answers as cancelled.
    pub fn cancel(&self) {
        (self.core.answers.cancel)();
    }

    /// What a page over the controller reports: `bread_crumb_trail`, `accept_enabled`, `summary`
    /// and `accept_text`.
    pub fn properties(&self) -> Vec<(&'static str, String)> {
        let verdict = self.verdict();
        vec![
            (
                "bread_crumb_trail",
                self.core.state.borrow().path_names().join("/"),
            ),
            ("accept_enabled", (!verdict.is_refused()).to_string()),
            ("summary", verdict.text().to_string()),
            ("accept_text", verdict.verb()),
        ]
    }
}

/// Nothing moved because the folder was held: the dialog stays, saying so.
fn busy(state: &Rc<RefCell<BrowserState>>) {
    tell(
        state,
        agg_gui_files::tr("The folder was busy, so nothing moved. Try again."),
    );
}

/// Nothing moved: `text` says why, in the dialog's status and through the app's report.
fn tell(state: &Rc<RefCell<BrowserState>>, text: String) {
    let report = state.borrow().config().report.clone();
    state.borrow_mut().after_write(text.clone());
    if let Some(report) = report {
        report(&text);
    }
}
