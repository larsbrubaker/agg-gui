//! The browser's folder tree: [`BrowserTree`], the root's folders ordered by name, each a row
//! (`row.rs`) with an expand arrow (when it may have folders of its own), the folder's icon and
//! its name; opening a folder lists its own folders one level in. A press on a row shows that
//! folder in the list beside the tree (through the shared [`BrowserState`]) and opens it in the
//! tree too. An app places the tree where it likes (the browser's composed form puts it left of
//! the list, in a splitter) and keeps one [`TreeModel`] across rebuilds so folders stay open.
//!
//! The tree follows the browser at each layout (`TreeModel::sync_to`): whenever the list moves to
//! another folder the tree opens down to it and draws its row selected; after a write it lists
//! its open folders again, so a folder renamed or removed shows as it now is; when the showing
//! folder's content changes from outside (a file added to a watched folder, a cloud folder
//! listed again) its row takes the folder's name and, when open, lists its folders again. The
//! rows as data, and which have an arrow, are `model.rs`; which folder a row stands for (its
//! location key, else its name) is `place.rs`; the tree's right-click menu is `menu.rs`, the app's
//! ordered list as the list's selection menu is, opened by the tree itself where the press was.
//!
//! Everything a row draws comes from the config's look: its style's text, selection fill, accent
//! and corner radius, the font size, the icon for each link's `micro_icon` key
//! (`BrowserLook::icon_for_key`, tinted in the accent; no icon where the look has none), and the
//! tree's and rows' names (`BrowserNames::tree`, `BrowserNames::tree_row`). The rows are built
//! again when the model, the browser or the look's art changes.
//!
//! Each row records itself as a drop target as it paints (the tree forgets last frame's and
//! records its own rectangle first, so a drag knows when it is outside the browser), lights up
//! while a drag is over it, and a closed row the drag waits over opens (`drag/spring.rs`).
//!
//! A press on a row gives the tree the keyboard (`BrowserState::tree_focus_id`), where the
//! history's undo and redo keys take back and replay its steps (`history/run.rs`), as they do in
//! the list, so they never reach an app's own undo while the tree has the keys; Delete and
//! Backspace remove the folder drawn selected, through its menu's Remove and its question.

pub mod menu;
pub mod model;
pub mod place;
pub mod row;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::focus::FocusId;
use agg_gui::text::Font;
use agg_gui::{
    Color, DrawCtx, Event, EventResult, Key, MouseButton, Point, Rect, Size, Widget, WidgetBase,
};

use crate::list::list_menu::ListMenu;
use crate::look::BrowserLook;
use crate::state::BrowserState;

pub use model::{TreeModel, TreeNode};
pub use place::{path_keys, PathLevel, TreeKey, TreePlace};
pub use row::{TreeRow, TREE_ICON_SIZE, TREE_ROW_HEIGHT};

use row::{RowSpec, TreeStyle, TREE_TOP};

/// What a build of the rows was made from: when any of it changes, the tree builds again.
#[derive(Clone, Copy, PartialEq, Eq)]
struct BuildKey {
    model: u64,
    state: u64,
    icons: u64,
}

/// The folder tree widget (see the module header).
pub struct BrowserTree {
    model: Rc<RefCell<TreeModel>>,
    state: Rc<RefCell<BrowserState>>,
    look: Rc<dyn BrowserLook>,
    style: TreeStyle,
    /// The accent the rows' icons are tinted in.
    accent: Color,
    built: Option<BuildKey>,
    menu: ListMenu,
    id: String,
    bounds: Rect,
    /// Carries the focus mark the app writes when it gives the tree the keys.
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
}

impl BrowserTree {
    /// The tree showing `model` beside the browser `state`, drawn in its config's look with
    /// `font`.
    pub fn new(
        model: Rc<RefCell<TreeModel>>,
        state: &Rc<RefCell<BrowserState>>,
        font: Arc<Font>,
    ) -> Self {
        let look = Rc::clone(&state.borrow().config().look);
        let style = look.style();
        let id = look.names().tree.clone();
        Self {
            model,
            state: Rc::clone(state),
            style: TreeStyle {
                font,
                font_size: style.font_size,
                text: style.text_color,
                selected: style.selection_fill,
                accent: style.accent,
                corner_radius: style.corner_radius,
            },
            accent: style.accent,
            look,
            built: None,
            menu: ListMenu::default(),
            id,
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
        }
    }

    /// The tree showing a new model of the browser `state`'s root.
    pub fn for_state(state: &Rc<RefCell<BrowserState>>, font: Arc<Font>) -> Self {
        Self::new(TreeModel::for_state(state), state, font)
    }

    /// The model the tree shows.
    pub fn model(&self) -> &Rc<RefCell<TreeModel>> {
        &self.model
    }

    /// The open menu's rows (titles, separators empty) and whether each is enabled; empty while
    /// no menu is open.
    pub fn menu_rows(&self) -> Vec<(String, bool)> {
        self.menu.rows()
    }

    /// The open menu's entries as built (icons, shortcuts and submenus included).
    pub fn menu_entries(&self) -> &[agg_gui::widgets::menu::MenuEntry] {
        self.menu.entries()
    }

    /// Whether the tree's menu is open.
    pub fn menu_is_open(&self) -> bool {
        self.menu.is_open()
    }

    fn build_key(&self) -> BuildKey {
        BuildKey {
            model: self.model.borrow().generation(),
            state: self.state.borrow().generation(),
            icons: self.look.icon_generation(),
        }
    }

    /// Builds the rows again from the model and the browser as they are now.
    fn rebuild(&mut self) {
        let levels = self.state.borrow().path_levels();
        let focus_id = self.state.borrow().tree_focus_id();
        let names = self.look.names();
        let icon_size = Size::new(TREE_ICON_SIZE, TREE_ICON_SIZE);
        let model = self.model.borrow();
        let showing = model.showing_row(&levels);
        let mut children: Vec<Box<dyn Widget>> = Vec::with_capacity(model.rows().len());
        for (index, node) in model.rows().iter().enumerate() {
            let icon = node
                .micro_icon()
                .and_then(|key| self.look.icon_for_key(key, icon_size, self.accent));
            let spec = RowSpec {
                id: names.tree_row_for(node.name()),
                index,
                name: node.name().to_string(),
                depth: node.depth(),
                expanded: node.is_expanded(),
                has_arrow: node.has_arrow(),
                placeholder: node.is_placeholder(),
                selected: showing == Some(index),
                icon,
            };
            let row = TreeRow::new(
                spec,
                Rc::clone(&self.model),
                Rc::clone(&self.state),
                focus_id,
                self.style.clone(),
            );
            children.push(Box::new(row));
        }
        drop(model);
        self.children = children;
        self.built = Some(self.build_key());
    }
}

impl Widget for BrowserTree {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "TreeView"
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
        self.model.borrow_mut().sync_to(&self.state.borrow());
        if self.built != Some(self.build_key()) {
            self.rebuild();
        }
        let mut top = available.height - TREE_TOP;
        for child in &mut self.children {
            top -= TREE_ROW_HEIGHT;
            child.layout(Size::new(available.width, TREE_ROW_HEIGHT));
            child.set_bounds(Rect::new(0.0, top, available.width, TREE_ROW_HEIGHT));
        }
        available
    }
    /// The rows record themselves as drop targets as they paint, after this forgets last
    /// frame's and records where the tree is, so a drag knows when it is outside the browser.
    /// First, a row the drag has waited over opens (`drag/spring.rs`).
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        crate::drag::spring::spring_tree(&self.state, &self.model);
        let rect = crate::drag::badge::painted_rect(ctx, self.bounds.width, self.bounds.height);
        let mut s = self.state.borrow_mut();
        s.drop_manager_mut().set_tree_rect(rect);
        s.drop_manager_mut()
            .clear_targets(crate::drag::DropTargetKind::Tree);
    }
    /// The open menu, over everything.
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.menu
            .paint(ctx, Arc::clone(&self.style.font), self.style.font_size);
    }
    /// While the menu is open it takes every event: a click outside closes it.
    fn has_active_modal(&self) -> bool {
        self.menu.is_open()
    }
    /// ... and no row under the menu sees the pointer.
    fn claims_pointer_exclusively(&self, _local_pos: Point) -> bool {
        self.menu.is_open()
    }
    /// A press on a row asks for the keys.
    fn is_focusable(&self) -> bool {
        true
    }
    fn focus_id(&self) -> Option<FocusId> {
        Some(self.state.borrow().tree_focus_id())
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        if let Some(result) = self.menu.handle_event(event) {
            return result;
        }
        if let Event::KeyDown { key, modifiers } = event {
            if matches!(key, Key::Delete | Key::Backspace) && !(modifiers.ctrl || modifiers.meta) {
                menu::remove_selected_folder(&self.state, &self.model);
                return EventResult::Consumed;
            }
            return crate::history::run::history_key_event(&self.state, key, *modifiers)
                .unwrap_or(EventResult::Ignored);
        }
        // A right press on a row (which showed its folder and asked for the menu, `row.rs`)
        // opens the folder's menu where it was pressed.
        if let Event::MouseDown {
            button: MouseButton::Right,
            pos,
            ..
        } = event
        {
            let requested = self.model.borrow_mut().take_menu_request();
            if let Some(index) = requested {
                menu::open_tree_menu(&mut self.menu, &self.state, &self.model, index, *pos);
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        let rows = self.model.borrow().indented_names();
        vec![("rows", rows.join("|"))]
    }
}
