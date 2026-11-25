//! `TopMenu` — one titled menu of a [`super::MenuBar`] — and `MenuTitle`,
//! the child widget the bar keeps per title.
//!
//! The bar paints its titles itself (`bar_widget.rs` + `labels.rs`), but
//! apps, inspectors and headless UI tests need to *find* a title: its
//! screen rect, its name.  So the bar keeps one [`MenuTitle`] child per
//! [`TopMenu`], positioned on the title's rect and carrying its id
//! (default `"{label} Menu"` with a leading icon glyph dropped, override
//! with [`TopMenu::with_id`]).  The
//! children paint nothing and never claim the pointer, so every click and
//! hover still reaches the bar exactly as before; clicking at a title
//! child's centre opens that menu.
//!
//! A `TopMenu` may also carry an items provider
//! ([`TopMenu::with_items_provider`]): the bar calls it each time the menu
//! opens (and before matching keyboard shortcuts), so apps whose entries
//! depend on live state (enable gates, check marks) build them only when
//! they are about to be shown. A menu whose rows host widgets supplies them
//! with [`TopMenu::with_row_widgets`], made as it opens, and one whose rows
//! change while it is open (a search field's results) takes them through
//! [`TopMenu::with_items_update`].

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widget::Widget;

use super::super::model::MenuEntry;
use super::items_update::MenuItemsUpdate;

/// Supplies a menu's entries at the moment it opens.
type ItemsProvider = Box<dyn FnMut() -> Vec<MenuEntry>>;

/// Makes the widgets a menu's [`crate::widgets::menu::MenuItem::widget_row`]
/// rows host, by row id, as it opens.
type RowWidgetsProvider = Box<dyn FnMut() -> Vec<(usize, Box<dyn Widget>)>>;

pub struct TopMenu {
    pub label: String,
    pub items: Vec<MenuEntry>,
    pub(super) rect: Rect,
    /// Explicit id for the title's child widget; `None` → `"{label} Menu"`
    /// (see [`TopMenu::title_id`]).
    id: Option<String>,
    provider: Option<ItemsProvider>,
    row_widgets: Option<RowWidgetsProvider>,
    /// Rows the menu takes while it is open ([`Self::with_items_update`]).
    pub(super) update: Option<MenuItemsUpdate>,
}

impl TopMenu {
    pub fn new(label: impl Into<String>, items: Vec<MenuEntry>) -> Self {
        Self {
            label: label.into(),
            items,
            rect: Rect::default(),
            id: None,
            provider: None,
            row_widgets: None,
            update: None,
        }
    }

    /// Name the title's child widget (what [`Widget::id`] returns for it).
    /// Defaults to `"{label} Menu"`, see [`TopMenu::title_id`].
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The id the title's child widget carries: the explicit id, else
    /// `"{label} Menu"`.  A label that leads with an icon glyph (a
    /// private-use code point, as Font Awesome's are) drops it and the space
    /// after it, so `"\u{F009} Demos"` is `"Demos Menu"`: C# keeps the icon
    /// out of the text (`MenuItemModel.IconGlyph`), so its name never had one.
    pub fn title_id(&self) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| format!("{} Menu", strip_leading_icon(&self.label)))
    }

    /// Build the menu's entries when it opens instead of using the static
    /// `items`.  The bar calls `provider` every time this menu opens and
    /// before it matches keyboard shortcuts while closed; the result
    /// replaces `items`.
    pub fn with_items_provider(
        mut self,
        provider: impl FnMut() -> Vec<MenuEntry> + 'static,
    ) -> Self {
        self.provider = Some(Box::new(provider));
        self
    }

    /// Supply the widgets this menu's widget rows host: the bar calls
    /// `provider` each time the menu opens, after its items, and registers
    /// each widget under its row id on the popup.
    pub fn with_row_widgets(
        mut self,
        provider: impl FnMut() -> Vec<(usize, Box<dyn Widget>)> + 'static,
    ) -> Self {
        self.row_widgets = Some(Box::new(provider));
        self
    }

    /// Let the open menu's rows be replaced through `update` (a search
    /// field's results), the menu and its row widgets staying as they are.
    pub fn with_items_update(mut self, update: MenuItemsUpdate) -> Self {
        self.update = Some(update);
        self
    }

    /// The widgets the row widgets provider makes now; none without one.
    pub(super) fn make_row_widgets(&mut self) -> Vec<(usize, Box<dyn Widget>)> {
        self.row_widgets.as_mut().map(|p| p()).unwrap_or_default()
    }

    /// Re-run the items provider, if any, replacing `items`.
    pub(super) fn refresh_items(&mut self) {
        if let Some(provider) = self.provider.as_mut() {
            self.items = provider();
        }
    }
}

/// `label` without a leading private-use glyph and the whitespace after it.
/// A label that is nothing but the glyph keeps it, so the id still names it.
fn strip_leading_icon(label: &str) -> &str {
    let mut chars = label.chars();
    match chars.next() {
        Some(c) if is_private_use(c) => match chars.as_str().trim_start() {
            "" => label,
            rest => rest,
        },
        _ => label,
    }
}

/// Unicode's private-use areas: the BMP block and planes 15 and 16.
fn is_private_use(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}')
}

/// The child widget standing for one bar title.  Invisible and
/// pointer-transparent: the bar draws and handles the title.
pub struct MenuTitle {
    bounds: Rect,
    id: String,
    label: String,
    children: Vec<Box<dyn Widget>>,
}

impl MenuTitle {
    pub(super) fn new(id: String, label: String) -> Self {
        Self {
            bounds: Rect::default(),
            id,
            label,
            children: Vec::new(),
        }
    }

    /// The title text this child stands for.
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl Widget for MenuTitle {
    fn type_name(&self) -> &'static str {
        "MenuTitle"
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

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    /// Never claim the pointer: the bar owns hover, click and drag over
    /// its titles.
    fn hit_test(&self, _local_pos: Point) -> bool {
        false
    }

    fn layout(&mut self, available: crate::geometry::Size) -> crate::geometry::Size {
        available
    }

    /// Events over a title belong to the bar.
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    /// The bar paints the title (backgrounds and label) itself.
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("label", self.label.clone())]
    }
}
