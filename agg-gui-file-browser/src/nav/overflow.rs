//! [`OverflowButton`]: the navigation bar's overflow ("File") button, which opens the open
//! folder's whole ordered menu: every row the app's `BrowserActions::menu` gives, the browser's
//! built-ins resolved and greyed when they can't run (`list/list_menu.rs`, `MenuScope::Overflow`),
//! hanging below the button. Its face is a [`BrowserIconButton`] showing the look's
//! `BrowserIcon::Overflow` art (or the glyph); its name is the look's
//! `BrowserNames::overflow_menu`. While the menu is open it takes every event, a click outside
//! closing it, and it is drawn over everything in the global overlay pass. A picked row runs once
//! the menu has closed, with the browser's state let go.
//!
//! What a bar places is [`overflow_control`]: the look's own button when it draws one
//! (`BrowserLook::overflow_button`, handed [`OverflowRows`] that give the same rows this menu
//! shows), else the [`OverflowButton`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::menu::MenuEntry;
use agg_gui::{DrawCtx, Event, EventResult, Point, Rect, Size, Widget, WidgetBase};

use super::buttons::NAV_BUTTON_SIDE;
use crate::actions::BrowserAction;
use crate::icon_button::BrowserIconButton;
use crate::list::list_menu::{menu_actions, ListMenu, MenuScope};
use crate::look::BrowserIcon;
use crate::state::BrowserState;

/// The overflow menu's rows as they are when asked: the open folder's whole menu, built-ins
/// resolved and greyed when they can't run.
pub type OverflowRows = Rc<dyn Fn() -> Vec<BrowserAction>>;

/// The overflow menu's rows for the browser `state` (none once the browser is gone).
pub fn overflow_rows(state: &Rc<RefCell<BrowserState>>) -> OverflowRows {
    let weak = Rc::downgrade(state);
    Rc::new(move || match weak.upgrade() {
        Some(state) => menu_actions(&state, MenuScope::Overflow),
        None => Vec::new(),
    })
}

/// The overflow button as a bar places it: the look's own (`BrowserLook::overflow_button`) when
/// it draws one, else an [`OverflowButton`].
pub fn overflow_control(state: &Rc<RefCell<BrowserState>>, font: &Arc<Font>) -> Box<dyn Widget> {
    let look = Rc::clone(&state.borrow().config().look);
    let name = look.names().overflow_menu.clone();
    match look.overflow_button(&name, overflow_rows(state), font) {
        Some(own) => own,
        None => Box::new(OverflowButton::new(state, Arc::clone(font))),
    }
}

/// The overflow ("File") button (see the module header).
pub struct OverflowButton {
    state: Rc<RefCell<BrowserState>>,
    face: BrowserIconButton,
    menu: ListMenu,
    font: Arc<Font>,
    font_size: f64,
    /// The face asked for the menu with its click.
    asked: Rc<Cell<bool>>,
    name: String,
    base: WidgetBase,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl OverflowButton {
    /// The overflow button of the browser `state`, its menu drawn in `font`.
    pub fn new(state: &Rc<RefCell<BrowserState>>, font: Arc<Font>) -> Self {
        let (look, name) = {
            let s = state.borrow();
            let look = Rc::clone(&s.config().look);
            let name = look.names().overflow_menu.clone();
            (look, name)
        };
        let font_size = look.style().font_size;
        let asked = Rc::new(Cell::new(false));
        let ask = Rc::clone(&asked);
        let face = BrowserIconButton::new(look, BrowserIcon::Overflow, NAV_BUTTON_SIDE)
            .with_icon_size(Size::new(NAV_BUTTON_SIDE, NAV_BUTTON_SIDE))
            .on_click(move || ask.set(true));
        Self {
            state: Rc::clone(state),
            face,
            menu: ListMenu::default(),
            font,
            font_size,
            asked,
            name,
            base: WidgetBase::new(),
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }

    /// Whether the menu is open.
    pub fn menu_is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// The open menu's rows (titles, separators empty) and whether each is enabled.
    pub fn menu_rows(&self) -> Vec<(String, bool)> {
        self.menu.rows()
    }

    /// The open menu's entries as built (icons, shortcuts and submenus included).
    pub fn menu_entries(&self) -> &[MenuEntry] {
        self.menu.entries()
    }
}

impl Widget for OverflowButton {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "OverflowButton"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
        self.face
            .set_bounds(Rect::new(0.0, 0.0, bounds.width, bounds.height));
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
        self.face.layout(available)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.face.paint(ctx);
    }
    /// The open menu, over everything.
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.menu.paint(ctx, Arc::clone(&self.font), self.font_size);
    }
    /// While the menu is open it takes every event: a click outside closes it.
    fn has_active_modal(&self) -> bool {
        self.menu.is_open()
    }
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        self.menu.is_open()
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Some(result) = self.menu.handle_event(event) {
            return result;
        }
        let result = self.face.on_event(event);
        if self.asked.replace(false) {
            self.menu
                .open(&self.state, MenuScope::Overflow, Point::new(0.0, 0.0));
            return EventResult::Consumed;
        }
        result
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        let mut properties = self.face.properties();
        properties.push(("menu_open", self.menu_is_open().to_string()));
        properties
    }
}
