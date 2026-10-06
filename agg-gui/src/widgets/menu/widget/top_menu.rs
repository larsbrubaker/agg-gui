//! `TopMenu` — one titled menu of a [`super::MenuBar`] — and `MenuTitle`,
//! the child widget the bar keeps per title.
//!
//! The bar paints its titles itself (`bar_widget.rs` + `labels.rs`), but
//! apps, inspectors and headless UI tests need to *find* a title: its
//! screen rect, its name.  So the bar keeps one [`MenuTitle`] child per
//! [`TopMenu`], positioned on the title's rect and carrying its id
//! (default `"{label} Menu"`, override with [`TopMenu::with_id`]).  The
//! children paint nothing and never claim the pointer, so every click and
//! hover still reaches the bar exactly as before; clicking at a title
//! child's centre opens that menu.
//!
//! A `TopMenu` may also carry an items provider
//! ([`TopMenu::with_items_provider`]): the bar calls it each time the menu
//! opens (and before matching keyboard shortcuts), so apps whose entries
//! depend on live state (enable gates, check marks) build them only when
//! they are about to be shown.

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect};
use crate::widget::Widget;

use super::super::model::MenuEntry;

/// Supplies a menu's entries at the moment it opens.
type ItemsProvider = Box<dyn FnMut() -> Vec<MenuEntry>>;

pub struct TopMenu {
    pub label: String,
    pub items: Vec<MenuEntry>,
    pub(super) rect: Rect,
    /// Explicit id for the title's child widget; `None` → `"{label} Menu"`.
    id: Option<String>,
    provider: Option<ItemsProvider>,
}

impl TopMenu {
    pub fn new(label: impl Into<String>, items: Vec<MenuEntry>) -> Self {
        Self {
            label: label.into(),
            items,
            rect: Rect::default(),
            id: None,
            provider: None,
        }
    }

    /// Name the title's child widget (what [`Widget::id`] returns for it).
    /// Defaults to `"{label} Menu"`.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The id the title's child widget carries.
    pub fn title_id(&self) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| format!("{} Menu", self.label))
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

    /// Re-run the items provider, if any, replacing `items`.
    pub(super) fn refresh_items(&mut self) {
        if let Some(provider) = self.provider.as_mut() {
            self.items = provider();
        }
    }
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
