//! [`NavMenuButton`]: a navigation bar button that opens a menu of choices, the view options and
//! sorting buttons (`buttons.rs`). Its face is a [`BrowserIconButton`] drawing the look's art (or
//! the icon's Font Awesome glyph); a click opens agg-gui's `PopupMenu` hanging below it, its rows
//! asked for anew each time it opens (so the marks show the state as it is now): check and radio
//! rows and separators ([`NavMenuRow`]). Picking a row runs it and closes the menu; a click
//! outside closes it. The menu is drawn over everything in the global overlay pass, and while it
//! is open it takes every event.
//!
//! The button carries its own name and tooltip (the look's names, "View Settings"), and its face
//! is driven through it rather than being a child, so a press on the face and the menu it opens
//! are one widget's.

use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::menu::{MenuEntry, MenuItem, MenuResponse, MenuSelection};
use agg_gui::{
    current_viewport, DrawCtx, Event, EventResult, Point, PopupMenu, Rect, Size, Widget, WidgetBase,
};

use crate::icon_button::BrowserIconButton;

/// One row of a navigation menu.
pub enum NavMenuRow {
    /// A line between groups.
    Separator,
    /// A choice that is on or off, run when picked.
    Check {
        title: String,
        checked: bool,
        run: Rc<dyn Fn()>,
    },
    /// One choice of a group, the one selected marked, run when picked.
    Radio {
        title: String,
        selected: bool,
        run: Rc<dyn Fn()>,
    },
}

/// The rows a menu shows, asked for each time it opens.
pub type NavMenuRows = Rc<dyn Fn() -> Vec<NavMenuRow>>;

/// A button opening a menu of choices (see the module header).
pub struct NavMenuButton {
    face: BrowserIconButton,
    rows: NavMenuRows,
    menu: Option<PopupMenu>,
    runs: Vec<Rc<dyn Fn()>>,
    font: Arc<Font>,
    font_size: f64,
    /// The face asked for the menu with its click.
    asked: Rc<std::cell::Cell<bool>>,
    name: String,
    base: WidgetBase,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl NavMenuButton {
    /// The button named `name` with `tooltip`, its face `face`, opening `rows` in `font`.
    pub fn new(
        face: BrowserIconButton,
        name: &str,
        tooltip: &str,
        rows: NavMenuRows,
        font: (Arc<Font>, f64),
    ) -> Self {
        let asked = Rc::new(std::cell::Cell::new(false));
        let ask = Rc::clone(&asked);
        let face = face.on_click(move || ask.set(true));
        let mut base = WidgetBase::new();
        base.tooltip = Some(tooltip.to_string());
        Self {
            face,
            rows,
            menu: None,
            runs: Vec::new(),
            font: font.0,
            font_size: font.1,
            asked,
            name: name.to_string(),
            base,
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }

    /// Whether the menu is open.
    pub fn menu_is_open(&self) -> bool {
        self.menu.as_ref().is_some_and(PopupMenu::is_open)
    }

    /// The open menu's rows: title (separators empty) and whether each is marked.
    pub fn menu_rows(&self) -> Vec<(String, bool)> {
        let Some(menu) = &self.menu else {
            return Vec::new();
        };
        menu.items
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item(item) => {
                    let marked = matches!(
                        item.selection,
                        MenuSelection::Check { selected: true }
                            | MenuSelection::Radio { selected: true }
                    );
                    (item.label.clone(), marked)
                }
                _ => (String::new(), false),
            })
            .collect()
    }

    /// Opens the menu below the button with the rows as they are now.
    fn open(&mut self) {
        let mut items = Vec::new();
        self.runs.clear();
        for row in (self.rows)() {
            match row {
                NavMenuRow::Separator => items.push(MenuEntry::Separator),
                NavMenuRow::Check {
                    title,
                    checked,
                    run,
                } => {
                    let id = format!("row:{}", self.runs.len());
                    items.push(MenuItem::action(title, id).checked(checked).into());
                    self.runs.push(run);
                }
                NavMenuRow::Radio {
                    title,
                    selected,
                    run,
                } => {
                    let id = format!("row:{}", self.runs.len());
                    items.push(MenuItem::action(title, id).radio(selected).into());
                    self.runs.push(run);
                }
            }
        }
        crate::trace(|| format!("{} opens with {} rows", self.name, items.len()));
        let mut menu = PopupMenu::new(items);
        menu.open_at(Point::new(0.0, 0.0));
        self.menu = Some(menu);
        agg_gui::animation::request_draw();
    }

    /// Routes `event` to the open menu; `None` while none is open. A picked row runs once the
    /// menu has closed.
    fn menu_event(&mut self, event: &Event) -> Option<EventResult> {
        let menu = self.menu.as_mut()?;
        let (result, response) = menu.handle_event(event, current_viewport());
        let picked = match response {
            MenuResponse::Action(id) => {
                let run = id
                    .strip_prefix("row:")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| self.runs.get(i).cloned());
                self.menu = None;
                run
            }
            MenuResponse::Closed => {
                self.menu = None;
                None
            }
            MenuResponse::None => None,
        };
        if let Some(run) = picked {
            run();
            agg_gui::animation::request_layout();
        }
        agg_gui::animation::request_draw();
        Some(result)
    }
}

impl Widget for NavMenuButton {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "NavMenuButton"
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
        if let Some(menu) = self.menu.as_mut() {
            menu.paint(
                ctx,
                Arc::clone(&self.font),
                self.font_size,
                current_viewport(),
            );
        }
    }
    /// While the menu is open it takes every event: a click outside closes it.
    fn has_active_modal(&self) -> bool {
        self.menu_is_open()
    }
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        self.menu_is_open()
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Some(result) = self.menu_event(event) {
            return result;
        }
        let result = self.face.on_event(event);
        if self.asked.replace(false) {
            self.open();
            return EventResult::Consumed;
        }
        result
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        let mut properties = self.face.properties();
        properties.retain(|(k, _)| *k != "tooltip");
        properties.push(("tooltip", self.base.tooltip.clone().unwrap_or_default()));
        properties.push(("menu_open", self.menu_is_open().to_string()));
        properties
    }
}
