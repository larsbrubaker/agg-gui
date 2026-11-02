//! [`SearchBox`]: a browser's search box, a text field ("Search" while empty) with a small clear
//! button inside its right end. Typing hands the text to the [`SearchController`], Enter runs the
//! search at once, the clear button ("Clear") or Escape blanks the box and clears the search, and
//! a search that ends any other way (the user walked away from the results) blanks the box. The
//! box polls its controller at each layout, so the debounced search runs in the frame its wake
//! brings. It does not take the keyboard when it appears: taking focus would steal keys meant for
//! the rest of the window.
//!
//! Its names are the look's (`BrowserNames::search_panel`, `search_input`, `close_search`), so
//! two browsers' boxes on screen at once are told apart.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::Named;
use agg_gui::{DrawCtx, Event, EventResult, Key, Rect, Size, TextField, Widget};
use agg_gui_files::tr;

use super::controller::SearchController;
use crate::icon_button::BrowserIconButton;
use crate::look::BrowserIcon;
use crate::state::BrowserState;

/// The box's margins, left and right.
const MARGIN_LEFT: f64 = 10.0;
const MARGIN_RIGHT: f64 = 5.0;
/// The clear button's side.
const CLEAR_SIDE: f64 = 20.0;

/// The search box (see the module header).
pub struct SearchBox {
    name: String,
    width: f64,
    controller: Rc<SearchController>,
    text: Rc<RefCell<String>>,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl SearchBox {
    /// The box searching the browser `state` through `controller`, `width` wide (0 stretches to
    /// the room it is given), its text in `font`.
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        controller: &Rc<SearchController>,
        font: Arc<Font>,
        width: f64,
    ) -> Self {
        let (look, names) = {
            let s = state.borrow();
            let look = Rc::clone(&s.config().look);
            let names = look.names().clone();
            (look, names)
        };
        let text = Rc::new(RefCell::new(String::new()));
        let typing = Rc::clone(controller);
        let entering = Rc::clone(controller);
        let field = TextField::new(Arc::clone(&font))
            .with_font_size(look.style().font_size)
            .with_placeholder(tr("Search"))
            .with_text_cell(Rc::clone(&text))
            .on_change(move |t| typing.set_query(t))
            .on_enter(move |_| entering.flush());
        let clearing = Rc::clone(controller);
        let clear_text = Rc::clone(&text);
        let clear = BrowserIconButton::new(look, BrowserIcon::ClearSearch, CLEAR_SIDE)
            .with_name(names.close_search.clone())
            .with_tooltip_text(&tr("Clear"))
            .on_click(move || {
                // Blanking the cell raises no change, so it starts no search.
                clear_text.borrow_mut().clear();
                clearing.clear();
            });
        let ended_text = Rc::clone(&text);
        controller.subscribe_search_ended(Rc::new(move || {
            ended_text.borrow_mut().clear();
            agg_gui::animation::request_layout();
        }));
        Self {
            name: names.search_panel.clone(),
            width,
            controller: Rc::clone(controller),
            text,
            bounds: Rect::default(),
            children: vec![
                Box::new(Named::new(names.search_input.clone(), Box::new(field))),
                Box::new(clear),
            ],
        }
    }

    /// What the box holds.
    pub fn text(&self) -> String {
        self.text.borrow().clone()
    }
}

impl Widget for SearchBox {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "TextEditWithInlineCancel"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.name)
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
    /// Runs a search whose pause is over, then lays the field out with the clear button over
    /// its right end.
    fn layout(&mut self, available: Size) -> Size {
        self.controller.poll();
        let outer = match self.width > 0.0 {
            true => self.width + MARGIN_LEFT + MARGIN_RIGHT,
            false => available.width,
        };
        let field_w = (outer - MARGIN_LEFT - MARGIN_RIGHT).max(0.0);
        let field = self.children[0].layout(Size::new(field_w, available.height));
        let clear = self.children[1].layout(Size::new(CLEAR_SIDE, CLEAR_SIDE));
        let height = field.height.max(clear.height);
        self.children[0].set_bounds(Rect::new(
            MARGIN_LEFT,
            (height - field.height) / 2.0,
            field_w,
            field.height,
        ));
        self.children[1].set_bounds(Rect::new(
            MARGIN_LEFT + field_w - clear.width - 2.0,
            (height - clear.height) / 2.0,
            clear.width,
            clear.height,
        ));
        Size::new(outer, height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    /// Escape in the field (which leaves it to its parent) blanks the box and clears the search.
    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::KeyDown {
                key: Key::Escape, ..
            } if !self.text.borrow().is_empty() || self.controller.is_search_active() => {
                self.text.borrow_mut().clear();
                self.controller.clear();
                agg_gui::animation::request_layout();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("text", self.text.borrow().clone())]
    }
}
