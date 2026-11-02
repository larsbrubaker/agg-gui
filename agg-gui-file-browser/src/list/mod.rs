//! The browser's list views: [`BrowserList`], the list of what the showing folder holds, as rows
//! (`ViewMode::Rows`, `row.rs`) or tiles (the icon modes, `icon_view.rs`), or in their place the
//! folder's message when its listing is only one (`message.rs`), all over the shared
//! [`BrowserState`] and drawn through the config's look and pictures.
//!
//! The list builds its rows again at its next layout whenever what they show changes: the
//! state's generation (a selection, a navigation, new content), a picture arriving (the
//! `ItemPictures` hook calls `PictureCx::picture_arrived`), or the look's art
//! (`BrowserLook::icon_generation`); each build marks the listing the rows were built from
//! (`BrowserState::mark_listed`), so a row's index always means the row under the pointer. A
//! list shown anew goes back to the top; one keeping its place (a selection) keeps its scroll; a
//! row the app selected for the user is scrolled into view (`reveal.rs`).
//!
//! Folder rows and tiles record themselves as drop targets as they paint, after the list forgets
//! last frame's and records its own rectangle, and light up under a drag (`drag/`); a folder the
//! drag waited over opens in the list as the list paints (`drag/spring.rs`), and the drag's
//! picture is drawn in the list's global overlay (`drag/picture.rs`, the list's `drag_picture`
//! properties).
//!
//! The list is also where its menus open (`list_menu.rs`) and its keys and rubber band are
//! handled (`gestures.rs`): a row asks for the selection's menu (a right press, or its menu
//! button, `menu_button.rs`) and the press bubbles here, which opens it where it was pressed or
//! hanging from the button; a right press on empty space, or a click in the list's own top-right
//! corner, opens the list's menu. Rows hold no state of their own beyond a build: hover, the menu
//! button's press and a press that may become a drag live in the state (`press.rs`), so they
//! survive the rows being rebuilt under the pointer. `item_event.rs` is a row's pointer handling,
//! `build.rs` what each row is built from, `tile_art.rs` what a row shows where its picture goes,
//! `overlay.rs` how the app's overlay on a picture is reached, and `section_header.rs` a search's
//! lines between its results.

pub mod build;
pub mod gestures;
pub mod icon_view;
pub mod item_event;
pub mod list_menu;
pub mod menu_button;
pub mod message;
pub mod overlay;
pub mod press;
pub mod reveal;
pub mod row;
pub mod section_header;
pub mod tile_art;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    DrawCtx, Event, EventResult, MouseButton, Point, Rect, ScrollView, Size, Widget, WidgetBase,
};

use crate::look::BrowserLook;
use crate::memory::ViewMode;
use crate::state::BrowserState;

use build::ListCx;
use gestures::{band_pointer_event, list_key_event, paint_band};
use icon_view::TileStack;
use item_event::{press_lost, press_moved, release_rowless};
use list_menu::{ListMenu, MenuScope};
use menu_button::MENU_GLYPH_SIZE;
use message::{warning_icon, FolderMessage};
use row::RowStack;

/// What a build of the rows was made from: when any of it changes, the list builds again.
#[derive(Clone, Copy, PartialEq, Eq)]
struct BuildKey {
    generation: u64,
    arrivals: u64,
    icons: u64,
}

/// Whether `pos` (list-local) is in the list's own top-right corner, a glyph's square, where a
/// click opens the list's menu.
fn in_list_corner(pos: Point, width: f64, height: f64) -> bool {
    pos.x >= width - MENU_GLYPH_SIZE
        && pos.x <= width
        && pos.y >= height - MENU_GLYPH_SIZE
        && pos.y <= height
}

/// The list of what the showing folder holds (see the module header).
pub struct BrowserList {
    state: Rc<RefCell<BrowserState>>,
    look: Rc<dyn BrowserLook>,
    font: Arc<Font>,
    /// Names under every tile, however small (`with_tile_names`).
    tile_names: bool,
    built: Option<BuildKey>,
    /// Bumped by each picture that arrives.
    arrivals: Rc<Cell<u64>>,
    /// The scroll position, kept across builds; the resets it was last put back to the top at.
    offset: Rc<Cell<f64>>,
    resets_seen: u64,
    menu: ListMenu,
    id: String,
    bounds: Rect,
    /// Carries the focus mark the app writes when it gives the list the keys.
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
}

impl BrowserList {
    /// The list of the browser `state`, drawn in its config's look with `font`.
    pub fn new(state: &Rc<RefCell<BrowserState>>, font: Arc<Font>) -> Self {
        let look = Rc::clone(&state.borrow().config().look);
        let id = look.names().list_area.clone();
        Self {
            state: Rc::clone(state),
            look,
            font,
            tile_names: false,
            built: None,
            arrivals: Rc::new(Cell::new(0)),
            offset: Rc::new(Cell::new(0.0)),
            resets_seen: 0,
            menu: ListMenu::default(),
            id,
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
        }
    }

    /// Names under every tile, however small: a list whose small tiles must still say which
    /// folder is which (a phone's popup browser).
    pub fn with_tile_names(mut self) -> Self {
        self.tile_names = true;
        self
    }

    /// The open menu's rows (titles, separators empty) and whether each is enabled; empty while
    /// no menu is open.
    pub fn menu_rows(&self) -> Vec<(String, bool)> {
        self.menu.rows()
    }

    /// Whether one of the list's menus is open.
    pub fn menu_is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// What the rows are built from now.
    fn build_key(&self) -> BuildKey {
        BuildKey {
            generation: self.state.borrow().generation(),
            arrivals: self.arrivals.get(),
            icons: self.look.icon_generation(),
        }
    }

    /// Builds the rows, tiles or message again from the state as it is now.
    fn rebuild(&mut self) {
        // Read before building: a picture that lands while the rows are built (an inline
        // spawner, or a hook answering `picture_arrived` at once) bumps `arrivals` after this, so
        // the next layout builds again and shows it.
        let key = self.build_key();
        self.state.borrow_mut().mark_listed();
        let arrivals = Rc::clone(&self.arrivals);
        let arrived: Rc<dyn Fn()> = Rc::new(move || {
            arrivals.set(arrivals.get() + 1);
            agg_gui::animation::request_layout();
        });
        let cx = ListCx::new(
            &self.state,
            Rc::clone(&self.look),
            Arc::clone(&self.font),
            arrived,
        );
        let (message, mode, thumb) = {
            let s = self.state.borrow();
            (s.listing_message(), s.view_mode(), s.thumb_size())
        };
        let child: Box<dyn Widget> = match message {
            Some(message) => {
                let icon = warning_icon(self.look.as_ref(), &cx.style);
                let names = self.look.names().clone();
                let font = Arc::clone(&self.font);
                let view =
                    FolderMessage::new(&self.state, &message, cx.style.clone(), names, font, icon);
                Box::new(view)
            }
            None => {
                let content: Box<dyn Widget> = match mode {
                    ViewMode::Rows => Box::new(RowStack::for_state(&cx)),
                    _ => Box::new(TileStack::for_state(&cx, thumb, self.tile_names)),
                };
                Box::new(ScrollView::new(content).with_offset_cell(Rc::clone(&self.offset)))
            }
        };
        self.children = vec![child];
        self.built = Some(key);
    }

    /// Opens `scope`'s menu at `pos` (list-local); the selection's hangs from the menu button
    /// that asked for it, when one did.
    fn open_menu(&mut self, scope: MenuScope, pos: Point) {
        let anchor = match scope {
            MenuScope::Selection => self.state.borrow_mut().menu_button_mut().take_anchor(),
            MenuScope::Folder => None,
        };
        // A window anchor in the list's coordinates: the list's window origin is the pointer
        // less its list-local position.
        let at = match (anchor, agg_gui::current_mouse_world()) {
            (Some(anchor), Some(world)) => {
                Point::new(anchor.x - (world.x - pos.x), anchor.y - (world.y - pos.y))
            }
            _ => pos,
        };
        self.menu.open(&self.state, scope, at);
    }
}

impl Widget for BrowserList {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "BrowserList"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let pending = {
            let mut s = self.state.borrow_mut();
            let pending = s.pump_live_refresh();
            s.sync_content();
            pending
        };
        if pending {
            agg_gui::animation::request_draw();
        }
        if self.built != Some(self.build_key()) {
            self.rebuild();
        }
        let resets = self.state.borrow().list_resets();
        if resets != self.resets_seen {
            self.resets_seen = resets;
            self.offset.set(0.0);
        }
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        let reveal = {
            let mut s = self.state.borrow_mut();
            let names = self.look.names();
            match s.take_reveal_selection() {
                true => s.selected_entries().first().map(|e| e.row_name(names)),
                false => None,
            }
        };
        if let Some(row) = reveal {
            reveal::reveal_row(&self.children, &row, &self.offset);
        }
        available
    }
    /// Records where the list is in the window, for a drag that ends outside it and for drops,
    /// and forgets last frame's folder targets before the rows record them again.
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (mut x, mut y) = (0.0, 0.0);
        agg_gui::logical_root_transform(ctx).transform(&mut x, &mut y);
        let rect = Rect::new(x, y, self.bounds.width, self.bounds.height);
        // A folder in the list the drag has waited over opens, found among last frame's targets.
        crate::drag::spring::spring_list(&self.state);
        let mut s = self.state.borrow_mut();
        s.item_press_mut().set_list_rect(rect);
        // The folders record themselves as drop targets again as they paint, after this.
        s.drop_manager_mut().set_list_rect(rect);
        s.drop_manager_mut()
            .clear_targets(crate::drag::DropTargetKind::Content);
    }
    /// The rubber band over the rows; while a drop on the list's empty space would land in the
    /// folder showing (one the drag opened), the list is lit as a folder is (the accent at alpha
    /// 40, with the "+" badge for a copy).
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let style = self.look.style();
        let size = (self.bounds.width, self.bounds.height);
        crate::drag::badge::paint_list_light(ctx, &self.state, size, &style);
        paint_band(
            ctx,
            &self.state,
            &self.children,
            size,
            style.accent,
            style.corner_radius,
        );
    }
    /// The drag's picture, then the open menu, over everything.
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let style = self.look.style();
        crate::drag::picture::paint_drag_picture(ctx, &self.state, &style, &self.font);
        self.menu
            .paint(ctx, Arc::clone(&self.font), style.font_size);
    }
    /// Whether a drag is under way, what a drop on the list's empty space does while it is lit
    /// (empty otherwise), and what the drag's picture showed in the last frame
    /// (`drag/picture.rs`).
    fn properties(&self) -> Vec<(&'static str, String)> {
        let state = self.state.borrow();
        let drop = state.drop_manager();
        let mut properties = vec![
            ("dragging", drop.is_dragging().to_string()),
            ("drop_label", drop.label_for(crate::drag::LIST_DROP_KEY)),
        ];
        properties.extend(crate::drag::picture::picture_properties(&state));
        properties
    }
    /// While a menu is open it takes every event: a click outside closes it.
    fn has_active_modal(&self) -> bool {
        self.menu.is_open()
    }
    /// ... and no row under the menu sees the pointer.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        self.menu.is_open()
    }
    /// A press in the list asks for the keys.
    fn is_focusable(&self) -> bool {
        true
    }
    fn focus_id(&self) -> Option<agg_gui::focus::FocusId> {
        Some(self.state.borrow().list_focus_id())
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Some(result) = self.menu.handle_event(event) {
            return result;
        }
        if let Event::KeyDown { key, modifiers } = event {
            return list_key_event(&self.state, &self.children, key, *modifiers);
        }
        // A press whose row was rebuilt away still ends here.
        match event {
            Event::MouseMove { pos }
                if self.state.borrow().item_press().is_pressed()
                    && item_event::is_pointer_move(*pos) =>
            {
                if press_moved(&self.state, *pos) {
                    return EventResult::Consumed;
                }
            }
            Event::MouseCaptureLost if press_lost(&self.state, None) => {
                return EventResult::Consumed;
            }
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                modifiers,
            } if release_rowless(&self.state, *pos, *modifiers) => {
                return EventResult::Consumed;
            }
            _ => {}
        }
        if let Some(result) = band_pointer_event(&self.state, &self.children, event) {
            return result;
        }
        let (pos, scope) = match event {
            Event::MouseDown {
                button: MouseButton::Right,
                pos,
                ..
            } => match self.state.borrow_mut().take_item_menu_request() {
                true => (*pos, MenuScope::Selection),
                false => (*pos, MenuScope::Folder),
            },
            // A click: a row's menu button (which asked for its menu), else the list's own
            // top-right corner.
            Event::MouseUp {
                button: MouseButton::Left,
                pos,
                ..
            } => {
                // A press on empty space ends: a band keeps what it selected, a plain click
                // there empties the selection (not one on the list menu's corner).
                let corner = in_list_corner(*pos, self.bounds.width, self.bounds.height);
                if self.state.borrow_mut().end_band(!corner) {
                    return EventResult::Consumed;
                }
                if self.state.borrow_mut().take_item_menu_request() {
                    (*pos, MenuScope::Selection)
                } else if corner {
                    (*pos, MenuScope::Folder)
                } else {
                    return EventResult::Ignored;
                }
            }
            _ => return EventResult::Ignored,
        };
        self.open_menu(scope, pos);
        EventResult::Consumed
    }
}
