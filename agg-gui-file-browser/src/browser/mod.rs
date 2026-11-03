//! [`FileBrowser`]: the whole browser composed from the crate's widgets over one
//! [`BrowserState`], in one of two forms (the config's `BrowserOptions::layout`):
//!
//! - **Tree and list** (an app's browsing tab): one header bar with a one-pixel rule under it
//!   (`parts.rs`'s `BottomBorder`): the breadcrumbs taking the room (the up button, then a link
//!   per folder from the root down), the search box ([`SEARCH_BOX_WIDTH`] wide), Undo and Redo,
//!   the view button, the sorting button and the overflow ("File") button; under it the folder
//!   tree, the splitter's bar and the list under the folder's header markdown.
//! - **List only** (a popup or narrow panel): the breadcrumbs and the overflow button; then a
//!   second bar named `BrowserNames::folders_toolbar` with the search box stretching, the view
//!   and sorting buttons; then the list under the folder's header markdown; then that header
//!   text again, small, in a bar along the bottom (`ProviderMessageBar`).
//!
//! The list-only form has a narrow variant for a phone's panel ([`FileBrowser::with_compact`],
//! while the app's flag is set): the list always shows [`COMPACT_VIEW`]'s tiles with their names
//! under them, whatever view the folder remembers (and without changing it), the toolbar has no
//! view button, and one tap activates a row (a folder opens; an item is the app's), since a
//! finger has no double click.
//!
//! An app's own widgets kept across rebuilds ([`FileBrowser::with_overlay_child`]: a host for
//! its context menus) are laid out with the browser's room but placed at zero size at its
//! origin, after everything else, so they are never under the pointer and paint (their overlay
//! passes) over the rest. The browser's window rectangle from its last paint goes to the app's
//! cell ([`FileBrowser::with_painted_rect`]), for hooks placing menus and drops in window terms.
//!
//! In both the undo notice sits centred [`NOTICE_BOTTOM_GAP`] above the bottom, over the rest.
//! Gaps, paddings and names are the ones MatterCAD's Library uses, so its tests and help
//! articles find the same widgets.
//!
//! The header is built again whenever the showing folder or its header text changes; the tree,
//! the list (and its scroll position) and the notice (and its fade) are kept, and so is the bar
//! holding the search box, around the same box: only what stands beside the box is new
//! (`parts.rs`), so the box keeps its text and focus and stays the widget a pointer is on its
//! way to.
//!
//! A browser given a location key ([`FileBrowser::remembering`]) opens where the config's
//! memory says it was last left under that key, showing that place's breadcrumbs over a loading
//! line while it is found (`state/restore.rs`), and remembers each folder the user moves to (and,
//! when the user ended the restore without moving, the place they stayed, once its answer comes).
//!
//! The drag's picture is drawn once for the whole browser, in agg-gui's global overlay pass after
//! everything else, rather than by the list: a drag from the tree, or one whose list rows a
//! spring-opened folder rebuilt, still carries its picture. The mouse's back button goes up one
//! folder. A folder's live relisting (an outside change reported by the directory monitor the
//! app registered with `agg_gui_files`) is pumped by the list at each layout.

pub mod parts;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{DrawCtx, Event, EventResult, FlexRow, Insets, MouseButton, Rect, Size, Widget};

use crate::bread_crumb::folder_bread_crumb_for;
use crate::config::BrowserLayout;
use crate::history::notice::BrowserNotice;
use crate::list::BrowserList;
use crate::look::BrowserLook;
use crate::memory::ViewMode;
use crate::nav::buttons::{sorting_control, view_options_control, HistoryButtons};
use crate::nav::overflow::overflow_control;
use crate::nav::provider_message::ProviderMessageBar;
use crate::nav::splitter::BrowserSplitter;
use crate::search::controller::SearchController;
use crate::search::search_box::SearchBox;
use crate::state::BrowserState;
use crate::tree::{BrowserTree, TreeModel};
use parts::{holds_search_slot, keep_search_bar, BottomBorder, ListColumn, SearchSlot};

/// The tab form's search box width, in logical units.
pub const SEARCH_BOX_WIDTH: f64 = 200.0;
/// How far above the browser's bottom the undo notice sits.
pub const NOTICE_BOTTOM_GAP: f64 = 16.0;
/// The view the compact form's list shows: tiles of 70, two across a phone's panel.
pub const COMPACT_VIEW: ViewMode = ViewMode::Icons70;
/// The mouse's back button, as agg-gui numbers it (`Other(3)`, the DOM's `MouseEvent.button`).
pub const BACK_MOUSE_BUTTON: u8 = 3;

/// What each of the browser's children is, for its layout and its rebuilds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A bar along the top (the nav bar, the toolbar), stacked down in order.
    Bar,
    /// The tree and list, or the list: the room left.
    Body,
    /// The provider message along the bottom.
    Message,
    /// The undo notice, centred near the bottom over the rest.
    Notice,
    /// An app's own kept child, at zero size at the origin.
    Overlay,
}

/// What a header was built for: when it changes, the header is built again.
#[derive(Clone, PartialEq, Eq)]
struct HeaderKey {
    navigation: u64,
    path: Vec<String>,
    header: String,
    compact: bool,
}

/// The composed browser (see the module header).
pub struct FileBrowser {
    state: Rc<RefCell<BrowserState>>,
    search: Rc<SearchController>,
    look: Rc<dyn BrowserLook>,
    font: Arc<Font>,
    list_only: bool,
    /// Set by the app while the list-only form is a phone's narrow panel.
    compact: Rc<Cell<bool>>,
    /// The compact flag the body was built for.
    body_compact: Option<bool>,
    /// The folder tree's rows (the tree form's), kept for the app to read.
    tree: Option<Rc<RefCell<TreeModel>>>,
    /// The search box while no header holds it (before the first build).
    search_box: Option<Box<dyn Widget>>,
    built: Option<HeaderKey>,
    /// The memory key the browser's place is remembered under, if any.
    location_key: Option<String>,
    /// The navigation count last remembered under `location_key`.
    remembered: Option<u64>,
    /// The app's kept children while no build holds them (before the first).
    overlays: Vec<Box<dyn Widget>>,
    /// Where the browser's window rectangle from its last paint goes.
    painted_rect: Option<Rc<Cell<Rect>>>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    roles: Vec<Role>,
}

impl FileBrowser {
    /// The browser over `state`, in the form its config's options ask for, searching with a
    /// [`SearchController`] of its own, drawn in `font`.
    pub fn new(state: &Rc<RefCell<BrowserState>>, font: Arc<Font>) -> Self {
        Self::with_search(state, SearchController::new(state), font)
    }

    /// [`Self::new`], searching through `search` (an app's controller made `with_maker`, so its
    /// search knows its content files).
    pub fn with_search(
        state: &Rc<RefCell<BrowserState>>,
        search: Rc<SearchController>,
        font: Arc<Font>,
    ) -> Self {
        let (look, list_only) = {
            let s = state.borrow();
            let config = s.config();
            (
                Rc::clone(&config.look),
                config.options.layout == BrowserLayout::ListOnly,
            )
        };
        let width = if list_only { 0.0 } else { SEARCH_BOX_WIDTH };
        let search_box = SearchBox::new(state, &search, Arc::clone(&font), width);
        Self {
            tree: (!list_only).then(|| TreeModel::for_state(state)),
            state: Rc::clone(state),
            search,
            look,
            font,
            list_only,
            compact: Rc::new(Cell::new(false)),
            body_compact: None,
            search_box: Some(Box::new(search_box)),
            built: None,
            location_key: None,
            remembered: None,
            overlays: Vec::new(),
            painted_rect: None,
            bounds: Rect::default(),
            children: Vec::new(),
            roles: Vec::new(),
        }
    }

    /// The browser opening where it was last left under `key` and remembering each folder the
    /// user moves to (see the module header). An app calls `BrowserState::retry_restore` to
    /// look again (once the user signs in).
    pub fn remembering(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        // A restore the user ended without moving (Home clicked while it was found) leaves the
        // browser where they chose: that is remembered once the late answer comes.
        let (weak, remembered) = (Rc::downgrade(&self.state), key.clone());
        self.state
            .borrow_mut()
            .add_restore_finished(Rc::new(move |outcome| {
                if outcome == crate::state::RestoreOutcome::Abandoned {
                    if let Some(state) = weak.upgrade() {
                        BrowserState::remember_location(&state, &remembered);
                    }
                }
            }));
        BrowserState::restore_location(&self.state, &key);
        self.remembered = Some(self.state.borrow().navigation());
        self.location_key = Some(key);
        self
    }

    /// Keeps `child` among the browser's children across every rebuild, at zero size at its
    /// origin over the rest (see the module header): a host for the app's context menus.
    pub fn with_overlay_child(mut self, child: Box<dyn Widget>) -> Self {
        self.overlays.push(child);
        self
    }

    /// Sets `rect` to the browser's rectangle in window (root, logical) coordinates at each
    /// paint.
    pub fn with_painted_rect(mut self, rect: Rc<Cell<Rect>>) -> Self {
        self.painted_rect = Some(rect);
        self
    }

    /// Remembers where the browser is when the user has moved since it last did.
    fn remember_if_navigated(&mut self) {
        let Some(key) = &self.location_key else {
            return;
        };
        let navigation = self.state.borrow().navigation();
        if self.remembered != Some(navigation) {
            self.remembered = Some(navigation);
            BrowserState::remember_location(&self.state, key);
        }
    }

    /// The list-only form laid out as a phone's narrow panel while `compact` is set (the app's
    /// panel sets it as it lays out; see the module header). No effect on the tree form.
    pub fn with_compact(mut self, compact: Rc<Cell<bool>>) -> Self {
        self.compact = compact;
        self
    }

    /// Whether the browser is laid out as the narrow panel now.
    pub fn is_compact(&self) -> bool {
        self.list_only && self.compact.get()
    }

    /// The browser's state.
    pub fn state(&self) -> &Rc<RefCell<BrowserState>> {
        &self.state
    }

    /// The search run from the search box (an app ends it when its panel closes).
    pub fn search(&self) -> &Rc<SearchController> {
        &self.search
    }

    /// The folder tree's rows, in the tree form.
    pub fn tree_model(&self) -> Option<&Rc<RefCell<TreeModel>>> {
        self.tree.as_ref()
    }

    /// Whether this is the list-only form.
    pub fn is_list_only(&self) -> bool {
        self.list_only
    }

    fn header_key(&self) -> HeaderKey {
        let s = self.state.borrow();
        HeaderKey {
            navigation: s.navigation(),
            path: s.shown_trail(),
            // No header (nor the list-only form's message bar) while a remembered folder is
            // found: the showing folder is the root, not the one being found.
            header: match s.is_restoring() {
                true => String::new(),
                false => s.header_markdown(),
            },
            compact: self.is_compact(),
        }
    }

    /// Builds the bars again, keeping the body, the notice and the search box.
    fn rebuild(&mut self, key: &HeaderKey) {
        let mut old_bars = Vec::new();
        let (mut body, mut notice) = (None, None);
        let mut overlays = std::mem::take(&mut self.overlays);
        let old = std::mem::take(&mut self.children);
        for (child, role) in old.into_iter().zip(std::mem::take(&mut self.roles)) {
            match role {
                Role::Body => body = Some(child),
                Role::Notice => notice = Some(child),
                Role::Overlay => overlays.push(child),
                Role::Bar => old_bars.push(child),
                Role::Message => {}
            }
        }
        // The first build stands the box in the new slot; later ones keep the bar holding it.
        let mut search_box = self.search_box.take();
        // The narrow panel's list differs from the wide one's: built again when that changes.
        let body = body
            .filter(|_| self.body_compact == Some(key.compact))
            .unwrap_or_else(|| self.body(key.compact));
        self.body_compact = Some(key.compact);
        let notice = notice
            .unwrap_or_else(|| Box::new(BrowserNotice::new(&self.state, Arc::clone(&self.font))));
        let mut kept = old_bars
            .into_iter()
            .find(|bar| holds_search_slot(bar.as_ref()));
        let mut keep = |bar: Box<dyn Widget>| match kept.take() {
            Some(old) if holds_search_slot(bar.as_ref()) => keep_search_bar(old, bar),
            old => {
                kept = old;
                bar
            }
        };
        if self.list_only {
            let nav_bar = keep(self.nav_bar(None));
            let toolbar =
                keep(self.toolbar(SearchSlot::new_or_empty(search_box.take()), key.compact));
            self.place(nav_bar, Role::Bar);
            self.place(toolbar, Role::Bar);
            self.place(body, Role::Body);
            let style = self.look.style();
            let name = &self.look.names().provider_message;
            let font = Arc::clone(&self.font);
            let message =
                ProviderMessageBar::for_message(&key.header, name, style.light_text_color, font);
            if let Some(bar) = message {
                self.place(Box::new(bar), Role::Message);
            }
        } else {
            let nav_bar = keep(self.nav_bar(Some(SearchSlot::new_or_empty(search_box.take()))));
            self.place(nav_bar, Role::Bar);
            self.place(body, Role::Body);
        }
        self.place(notice, Role::Notice);
        for overlay in overlays {
            self.place(overlay, Role::Overlay);
        }
    }

    fn place(&mut self, child: Box<dyn Widget>, role: Role) {
        self.children.push(child);
        self.roles.push(role);
    }

    /// The tree, splitter and list; or the list.
    fn body(&self, compact: bool) -> Box<dyn Widget> {
        let mut list = BrowserList::new(&self.state, Arc::clone(&self.font)).without_drag_picture();
        if compact {
            list = list.with_view(COMPACT_VIEW).with_tile_names();
        }
        let name = self.look.names().header_markdown.clone();
        let font = Arc::clone(&self.font);
        let list = ListColumn::new(&self.state, Box::new(list), font, name);
        let Some(model) = &self.tree else {
            return Box::new(list);
        };
        let tree = BrowserTree::new(Rc::clone(model), &self.state, Arc::clone(&self.font));
        let memory = Rc::clone(&self.state.borrow().config().memory);
        let style = self.look.style();
        Box::new(BrowserSplitter::new(
            Box::new(tree),
            Box::new(list),
            style.splitter_background,
            memory,
            &self.look.names().splitter,
        ))
    }

    /// The breadcrumbs, then (the tree form's) the search box, Undo and Redo, the view and
    /// sorting buttons, then the overflow button; over the bar's rule.
    fn nav_bar(&self, search_slot: Option<SearchSlot>) -> Box<dyn Widget> {
        let names = self.state.borrow().shown_trail();
        let weak = Rc::downgrade(&self.state);
        let go_to: Rc<dyn Fn(usize)> = Rc::new(move |depth| {
            if let Some(state) = weak.upgrade() {
                state.borrow_mut().go_to_crumb(depth);
            }
        });
        let crumbs = folder_bread_crumb_for(&names, go_to, &self.look, &self.font);
        // 3 left and right, 1 top and bottom.
        let mut row = FlexRow::new()
            .with_gap(4.0)
            .with_inner_padding(Insets::symmetric(3.0, 1.0))
            .add_flex(Box::new(crumbs), 1.0);
        if let Some(search_slot) = search_slot {
            row = row.add(Box::new(search_slot));
        }
        if !self.list_only {
            let font = Arc::clone(&self.font);
            row = row
                .add(Box::new(HistoryButtons::new(&self.state, font)))
                .add(view_options_control(&self.state, &self.font, true))
                .add(sorting_control(&self.state, &self.font));
        }
        let row = row.add(overflow_control(&self.state, &self.font));
        Box::new(BottomBorder::new(
            Box::new(row),
            self.look.style().border_color,
        ))
    }

    /// The list-only form's toolbar: the search box taking the room, the view (not on the narrow
    /// panel, whose view is its own) and sorting buttons, over the bar's rule.
    fn toolbar(&self, search_slot: SearchSlot, compact: bool) -> Box<dyn Widget> {
        let mut row = FlexRow::new()
            .with_gap(4.0)
            .with_padding(2.0)
            .add_flex(Box::new(search_slot), 1.0);
        if !compact {
            row = row.add(view_options_control(&self.state, &self.font, true));
        }
        let row = row.add(sorting_control(&self.state, &self.font));
        let name = self.look.names().folders_toolbar.clone();
        let bar = BottomBorder::new(Box::new(row), self.look.style().border_color);
        Box::new(bar.with_name(name))
    }
}

impl Widget for FileBrowser {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "FileBrowser"
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
    /// The bars down from the top, the message along the bottom, the body in between, and the
    /// notice centred near the bottom.
    fn layout(&mut self, available: Size) -> Size {
        self.remember_if_navigated();
        let key = self.header_key();
        if self.built.as_ref().map(|b| b.compact) != Some(key.compact) {
            // One tap activates on the narrow panel; elsewhere the config says.
            let mut s = self.state.borrow_mut();
            let tap = key.compact || s.config().options.tap_activates;
            s.set_tap_activates(tap);
        }
        if self.built.as_ref() != Some(&key) {
            self.rebuild(&key);
            self.built = Some(key);
        }
        let (width, height) = (available.width, available.height);
        let mut top = height;
        let mut bottom = 0.0;
        for (child, role) in self.children.iter_mut().zip(&self.roles) {
            match role {
                Role::Bar => {
                    let size = child.layout(Size::new(width, top));
                    top -= size.height;
                    child.set_bounds(Rect::new(0.0, top, width, size.height));
                }
                Role::Message => {
                    let size = child.layout(Size::new(width, top));
                    child.set_bounds(Rect::new(0.0, 0.0, width, size.height));
                    bottom = size.height;
                }
                Role::Body | Role::Notice | Role::Overlay => {}
            }
        }
        for (child, role) in self.children.iter_mut().zip(&self.roles) {
            match role {
                Role::Body => {
                    let room = Size::new(width, (top - bottom).max(0.0));
                    child.layout(room);
                    child.set_bounds(Rect::new(0.0, bottom, room.width, room.height));
                }
                Role::Notice => {
                    let size = child.layout(available);
                    let x = ((width - size.width) / 2.0).max(0.0);
                    child.set_bounds(Rect::new(x, NOTICE_BOTTOM_GAP, size.width, size.height));
                }
                Role::Overlay => {
                    child.layout(available);
                    child.set_bounds(Rect::new(0.0, 0.0, 0.0, 0.0));
                }
                Role::Bar | Role::Message => {}
            }
        }
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if let Some(rect) = &self.painted_rect {
            let (mut x, mut y) = (0.0, 0.0);
            agg_gui::widget::logical_root_transform(ctx).transform(&mut x, &mut y);
            rect.set(Rect::new(x, y, self.bounds.width, self.bounds.height));
        }
    }
    /// The drag's picture by the pointer, over everything (`drag/picture.rs`).
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let style = self.look.style();
        crate::drag::picture::paint_drag_picture(ctx, &self.state, &style, &self.font);
    }
    /// The mouse's back button goes up one folder, as the up button does (along the trail
    /// being found, while restoring).
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Event::MouseDown {
            button: MouseButton::Other(BACK_MOUSE_BUTTON),
            ..
        } = event
        {
            self.state.borrow_mut().go_back();
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
    /// What tests and automation read: the path and its trail (the trail being found, while
    /// restoring), whether a restore shows, the selection, the showing
    /// folder's counts, header and status, the view, the search, and the drag's picture.
    fn properties(&self) -> Vec<(&'static str, String)> {
        let state = self.state.borrow();
        let selected: Vec<String> = state
            .selected_entries()
            .iter()
            .map(|e| e.name().to_string())
            .collect();
        let (folders, items) = state
            .active_container()
            .map_or((0, 0), |c| (c.child_containers().len(), c.items().len()));
        let results = self.search.current_results();
        let sections = results.as_ref().map_or_else(String::new, |r| {
            let names: Vec<String> = r.sections().iter().map(|s| s.name().to_string()).collect();
            names.join("\n")
        });
        let mut properties = vec![
            ("path", state.path_names().join(" > ")),
            ("bread_crumb_trail", state.shown_trail().join("/")),
            ("restoring", state.is_restoring().to_string()),
            ("selected", selected.join(", ")),
            ("child_container_count", folders.to_string()),
            ("item_count", items.to_string()),
            ("status", state.status()),
            ("header", state.header_markdown()),
            ("view_mode", self.look.view_mode_name(state.view_mode())),
            ("search_active", self.search.is_search_active().to_string()),
            (
                "search_searching",
                results
                    .as_ref()
                    .is_some_and(|r| r.is_searching())
                    .to_string(),
            ),
            ("search_sections", sections),
            (
                "search_query",
                results.map(|r| r.query().to_string()).unwrap_or_default(),
            ),
            ("dragging", state.drop_manager().is_dragging().to_string()),
        ];
        properties.extend(crate::drag::picture::picture_properties(&state));
        properties
    }
}
