//! The navigation bar's buttons over a browser's state: the view options button ("View Settings":
//! Show Folders, then the view choices), the sorting button ("Sorting": the keys, then the
//! directions), each a [`NavMenuButton`] whose picks change the state (and through it the
//! memory's view for the folder); and the Undo and Redo buttons over the browser's history
//! ([`HistoryButtons`]). Their names are the look's (`BrowserNames`), their art the look's
//! (`BrowserIcon::ViewOptions`, `Sort`, `Undo`, `Redo`; the Font Awesome glyph without it).
//!
//! What a bar places are [`view_options_control`] and [`sorting_control`]: the look's own menu
//! button when it draws one (`BrowserLook::menu_button`, handed the same rows), else the
//! [`NavMenuButton`].
//!
//! The Undo and Redo buttons are greyed while their list is empty (or an undo is still
//! finishing); each one's tooltip names the step it would take and its key ("Undo - Moved 3 items
//! to Brackets (⌘Z)"), and the key alone is its hint (`BrowserLook::show_hint`, an app's status
//! bar). They are built again whenever the history changes (`BrowserHistory::generation`), so a
//! change made in another view shows here; a button replaced while its hint shows clears it.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    current_platform, DrawCtx, Event, EventResult, MenuShortcut, Platform, Rect, Size, Widget,
};
use agg_gui_files::{tr, SortKey};

use super::menu_button::{NavMenuButton, NavMenuRow, NavMenuRows};
use crate::icon_button::BrowserIconButton;
use crate::look::BrowserIcon;
use crate::memory::ViewMode;
use crate::state::BrowserState;

/// The side of a navigation bar button, and of the art in it.
pub const NAV_BUTTON_SIDE: f64 = 32.0;
/// The room between Undo and Redo: the nav bar's gap between its pieces, so the pair stands as
/// two of the bar's buttons would.
pub const HISTORY_GAP: f64 = 4.0;

/// The face of a menu button showing `which`.
fn face(state: &Rc<RefCell<BrowserState>>, which: BrowserIcon) -> BrowserIconButton {
    let look = Rc::clone(&state.borrow().config().look);
    BrowserIconButton::new(look, which, NAV_BUTTON_SIDE)
        .with_icon_size(Size::new(NAV_BUTTON_SIDE, NAV_BUTTON_SIDE))
}

/// A radio row: `title`, selected when `selected` says so of `state`, running `pick` on it.
fn radio(
    state: &Rc<RefCell<BrowserState>>,
    title: String,
    selected: bool,
    pick: impl Fn(&mut BrowserState) + 'static,
) -> NavMenuRow {
    let state = Rc::clone(state);
    NavMenuRow::Radio {
        title,
        selected,
        run: Rc::new(move || pick(&mut state.borrow_mut())),
    }
}

/// The view choices the menu offers, in order. The tiny and small icon views are left out: three
/// choices keep the menu simple.
fn view_choices() -> [(String, ViewMode); 3] {
    [
        (tr("View List"), ViewMode::Rows),
        (tr("View Icons"), ViewMode::Icons),
        (tr("View Large Icons"), ViewMode::Icons256),
    ]
}

/// The view options menu's rows: "Show Folders" and a separator when `show_folders` (a browser
/// whose folders are its only way to a destination, a Save or Move dialog, leaves it out), then
/// the view choices.
fn view_options_rows(state: &Rc<RefCell<BrowserState>>, show_folders: bool) -> NavMenuRows {
    let menu_state = Rc::clone(state);
    Rc::new(move || {
        let state = &menu_state;
        let (shown, mode) = {
            let s = state.borrow();
            (s.show_containers(), s.view_mode())
        };
        let mut rows = Vec::new();
        if show_folders {
            let set = Rc::clone(state);
            rows.push(NavMenuRow::Check {
                title: tr("Show Folders"),
                checked: shown,
                run: Rc::new(move || {
                    let show = !set.borrow().show_containers();
                    set.borrow_mut().set_show_containers(show);
                }),
            });
            rows.push(NavMenuRow::Separator);
        }
        for (title, choice) in view_choices() {
            rows.push(radio(state, title, mode == choice, move |s| {
                s.set_view_mode(choice)
            }));
        }
        rows
    })
}

/// The sorting menu's rows: the keys, a separator, then the directions; a pick is remembered
/// for the folder.
fn sorting_rows(state: &Rc<RefCell<BrowserState>>) -> NavMenuRows {
    let menu_state = Rc::clone(state);
    Rc::new(move || {
        let state = &menu_state;
        let sort = state.borrow().sort();
        let keys = [
            (tr("Date Created"), SortKey::CreatedDate),
            (tr("Date Modified"), SortKey::ModifiedDate),
            (tr("Name"), SortKey::Name),
        ];
        let mut rows: Vec<NavMenuRow> = keys
            .into_iter()
            .map(|(title, key)| {
                radio(state, title, sort.has_flag(key), move |s| {
                    s.set_user_sort_key(key)
                })
            })
            .collect();
        rows.push(NavMenuRow::Separator);
        for (title, ascending) in [(tr("Ascending"), true), (tr("Descending"), false)] {
            rows.push(radio(state, title, sort.ascending == ascending, move |s| {
                s.set_user_sort_ascending(ascending)
            }));
        }
        rows
    })
}

/// The look's name for a nav button and the look's font size.
fn name_and_size(
    state: &Rc<RefCell<BrowserState>>,
    name: impl Fn(&crate::look::BrowserNames) -> String,
) -> (String, f64) {
    let s = state.borrow();
    let look = &s.config().look;
    (name(look.names()), look.style().font_size)
}

/// The view options button (its rows: `view_options_rows`).
pub fn view_options_button(
    state: &Rc<RefCell<BrowserState>>,
    font: &Arc<Font>,
    show_folders: bool,
) -> NavMenuButton {
    let (name, size) = name_and_size(state, |n| n.view_options_button.clone());
    let rows = view_options_rows(state, show_folders);
    let face = face(state, BrowserIcon::ViewOptions);
    NavMenuButton::new(
        face,
        &name,
        &tr("View Settings"),
        rows,
        (Arc::clone(font), size),
    )
}

/// The sorting button (its rows: `sorting_rows`).
pub fn sorting_button(state: &Rc<RefCell<BrowserState>>, font: &Arc<Font>) -> NavMenuButton {
    let (name, size) = name_and_size(state, |n| n.sort_button.clone());
    let rows = sorting_rows(state);
    let face = face(state, BrowserIcon::Sort);
    NavMenuButton::new(face, &name, &tr("Sorting"), rows, (Arc::clone(font), size))
}

/// The view options button as a bar places it: the look's own (`BrowserLook::menu_button`)
/// when it draws one, else [`view_options_button`].
pub fn view_options_control(
    state: &Rc<RefCell<BrowserState>>,
    font: &Arc<Font>,
    show_folders: bool,
) -> Box<dyn Widget> {
    let look = Rc::clone(&state.borrow().config().look);
    let name = look.names().view_options_button.clone();
    let rows = view_options_rows(state, show_folders);
    let tooltip = tr("View Settings");
    match look.menu_button(BrowserIcon::ViewOptions, &name, &tooltip, rows, font) {
        Some(own) => own,
        None => Box::new(view_options_button(state, font, show_folders)),
    }
}

/// The sorting button as a bar places it: the look's own (`BrowserLook::menu_button`) when it
/// draws one, else [`sorting_button`].
pub fn sorting_control(state: &Rc<RefCell<BrowserState>>, font: &Arc<Font>) -> Box<dyn Widget> {
    let look = Rc::clone(&state.borrow().config().look);
    let name = look.names().sort_button.clone();
    let tooltip = tr("Sorting");
    match look.menu_button(
        BrowserIcon::Sort,
        &name,
        &tooltip,
        sorting_rows(state),
        font,
    ) {
        Some(own) => own,
        None => Box::new(sorting_button(state, font)),
    }
}

/// The key that undoes (`redo`: redoes), as the platform writes it: ⌘Z and ⇧⌘Z on a Mac,
/// Ctrl+Z and Ctrl+Y elsewhere.
pub fn history_key_text(redo: bool, font: &Arc<Font>) -> String {
    let mac = current_platform() == Platform::MacOS;
    let text = match (redo, mac) {
        (false, _) => "Ctrl+Z",
        (true, true) => "Ctrl+Shift+Z",
        (true, false) => "Ctrl+Y",
    };
    match MenuShortcut::parse(text) {
        Some(shortcut) => shortcut.display_text_for_font(current_platform(), font),
        None => text.to_string(),
    }
}

/// The Undo and Redo buttons over a browser's history (see the module header), side by side
/// [`HISTORY_GAP`] apart.
pub struct HistoryButtons {
    state: Rc<RefCell<BrowserState>>,
    font: Arc<Font>,
    /// The history generation the buttons were built for.
    built: Option<u64>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl HistoryButtons {
    /// The buttons of the browser `state`, their keys written in `font`.
    pub fn new(state: &Rc<RefCell<BrowserState>>, font: Arc<Font>) -> Self {
        Self {
            state: Rc::clone(state),
            font,
            built: None,
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }

    /// One button: Undo, or Redo when `redo`.
    fn button(&self, redo: bool) -> BrowserIconButton {
        let (look, history) = {
            let s = self.state.borrow();
            (Rc::clone(&s.config().look), s.history())
        };
        let (which, title, step, enabled, name) = match redo {
            false => (
                BrowserIcon::Undo,
                tr("Undo"),
                history.undo_description(),
                history.can_undo(),
                look.names().undo_button.clone(),
            ),
            true => (
                BrowserIcon::Redo,
                tr("Redo"),
                history.redo_description(),
                history.can_redo(),
                look.names().redo_button.clone(),
            ),
        };
        let key = history_key_text(redo, &self.font);
        let tip = match &step {
            Some(step) => format!("{title} - {step} ({key})"),
            None => format!("{title} ({key})"),
        };
        let state = Rc::clone(&self.state);
        BrowserIconButton::new(look, which, NAV_BUTTON_SIDE)
            .with_name(name)
            .with_tooltip_text(&tip)
            .with_hint(&key)
            .with_enabled(enabled && step.is_some())
            .on_click(move || {
                match redo {
                    true => BrowserState::redo(&state),
                    false => BrowserState::undo(&state),
                };
                agg_gui::animation::request_layout();
            })
    }
}

impl Widget for HistoryButtons {
    fn type_name(&self) -> &'static str {
        "HistoryButtons"
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
    fn layout(&mut self, _available: Size) -> Size {
        let generation = self.state.borrow().history().generation();
        if self.built != Some(generation) {
            // A button under the pointer showing its hint is replaced: its hint goes with it.
            let hinting = self.children.iter().any(|c| {
                c.as_any()
                    .and_then(|a| a.downcast_ref::<BrowserIconButton>())
                    .is_some_and(BrowserIconButton::is_hovered)
            });
            if hinting {
                self.state.borrow().config().look.show_hint(None);
            }
            self.children = vec![Box::new(self.button(false)), Box::new(self.button(true))];
            self.built = Some(generation);
        }
        let side = NAV_BUTTON_SIDE;
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(side, side));
            child.set_bounds(Rect::new(i as f64 * (side + HISTORY_GAP), 0.0, side, side));
        }
        Size::new(2.0 * side + HISTORY_GAP, side)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
