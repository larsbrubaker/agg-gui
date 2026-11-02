//! The list's menus: the selection's menu (a right press on a row, or a click on a row's menu
//! button) and the list's own (a right press on empty space, or a click in the list's top-right
//! corner), each the whole ordered list the app's `BrowserActions::menu` gives for a snapshot of
//! the browser, the built-ins offered where it opens placed in it: the ones acting on the
//! selection in the selection's menu, adding to the folder in the list's. The menu is an agg-gui
//! `PopupMenu` the list holds open over everything (`paint_global_overlay`) while it routes
//! events to it. An app command runs when picked, with the browser's state let go. The
//! built-ins show greyed until the browser can run them itself.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::menu::{MenuEntry, MenuItem, MenuResponse};
use agg_gui::{current_viewport, DrawCtx, Event, EventResult, Point, PopupMenu};

use crate::actions::{BrowserAction, BrowserCommand, BuiltinAction, Builtins};
use crate::state::BrowserState;

/// Which menu the list opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuScope {
    /// The selection's menu.
    Selection,
    /// The list's own menu, for the open folder.
    Folder,
}

/// The built-ins offered in `scope`'s menu.
fn builtins_for(scope: MenuScope) -> Builtins {
    let offered: Vec<BuiltinAction> = BuiltinAction::ALL
        .iter()
        .copied()
        .filter(|a| a.acts_on_selection() == (scope == MenuScope::Selection))
        .collect();
    Builtins::new(&offered)
}

/// The list's open menu, if one is open, and the commands its rows run.
#[derive(Default)]
pub struct ListMenu {
    menu: Option<PopupMenu>,
    commands: Vec<BrowserCommand>,
}

impl ListMenu {
    /// Whether a menu is open.
    pub fn is_open(&self) -> bool {
        self.menu.as_ref().is_some_and(PopupMenu::is_open)
    }

    /// The titles of the open menu's rows, separators as empty strings, and whether each is
    /// enabled.
    pub fn rows(&self) -> Vec<(String, bool)> {
        let Some(menu) = &self.menu else {
            return Vec::new();
        };
        menu.items
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item(item) => (item.label.clone(), item.enabled),
                _ => (String::new(), false),
            })
            .collect()
    }

    /// Opens `scope`'s menu at `at` (the list's own coordinates), asking the app for its rows
    /// with the state let go.
    pub fn open(&mut self, state: &Rc<RefCell<BrowserState>>, scope: MenuScope, at: Point) {
        let (actions, snapshot) = {
            let s = state.borrow();
            (Rc::clone(&s.config().actions), s.snapshot())
        };
        let rows = actions.menu(&snapshot, &builtins_for(scope));
        let mut commands = Vec::new();
        let mut items = Vec::new();
        for row in rows {
            match row {
                BrowserAction::Separator => items.push(MenuEntry::Separator),
                BrowserAction::Builtin(action) => {
                    let id = format!("builtin:{action:?}");
                    items.push(MenuItem::action(action.title(), id).disabled().into());
                }
                BrowserAction::Command(command) => {
                    let id = format!("command:{}", commands.len());
                    let mut item = MenuItem::action(command.title().to_string(), id);
                    if !command.is_enabled() {
                        item = item.disabled();
                    }
                    items.push(item.into());
                    commands.push(command);
                }
            }
        }
        crate::trace(|| format!("list menu {scope:?} opens with {} rows", items.len()));
        let mut menu = PopupMenu::new(items);
        menu.open_at(at);
        self.menu = Some(menu);
        self.commands = commands;
        agg_gui::animation::request_draw();
    }

    /// Closes the menu.
    pub fn close(&mut self) {
        self.menu = None;
        self.commands.clear();
    }

    /// Routes `event` (the list's own coordinates) to the open menu; `None` while none is open.
    /// A picked command runs once the menu has closed.
    pub fn handle_event(&mut self, event: &Event) -> Option<EventResult> {
        let menu = self.menu.as_mut()?;
        let (result, response) = menu.handle_event(event, current_viewport());
        let picked = match response {
            MenuResponse::Action(id) => {
                let command = id
                    .strip_prefix("command:")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| self.commands.get(i).cloned());
                self.close();
                command
            }
            MenuResponse::Closed => {
                self.close();
                None
            }
            MenuResponse::None => None,
        };
        if let Some(command) = picked {
            command.run();
        }
        agg_gui::animation::request_draw();
        Some(result)
    }

    /// Paints the open menu.
    pub fn paint(&mut self, ctx: &mut dyn DrawCtx, font: Arc<Font>, font_size: f64) {
        if let Some(menu) = self.menu.as_mut() {
            menu.paint(ctx, font, font_size, current_viewport());
        }
    }
}
