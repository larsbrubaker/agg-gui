//! The browser's menus: the selection's menu (a right press on a row, or a click on a row's menu
//! button), the list's own (a right press on empty space, or a click in the list's top-right
//! corner) and the overflow menu (the nav bar's "File" button), each built from the whole ordered
//! list the app's `BrowserActions::menu` gives for a snapshot of the browser.
//!
//! Building a menu (`menu_actions`): the built-ins offered where it opens (the ones acting on the
//! selection in the selection's menu, adding to the folder in the list's, every one in the
//! overflow menu) become rows that run (`builtins/`), a built-in the menu doesn't offer is left
//! out, and `select_rows` keeps the rows the menu shows: the selection's menu shows the rows of
//! its scope that can run now (one that can't is left out, so a right-click menu holds only what
//! can be done), the list's menu the rows of its scope, greyed when they can't run, and the
//! overflow menu every row, greyed when it can't run. A row marked `only_when_enabled` is left out
//! of every menu while it can't run. A separator shows only between two rows kept, never doubled
//! or at an end; submenus are filtered the same way.
//!
//! The menu is an agg-gui `PopupMenu` the opening widget holds open over everything
//! (`paint_global_overlay`) while it routes events to it. A row's icon comes from the look
//! (`BrowserLook::icon_for_key`, in the style's text colour); its shortcut is shown on its right.
//! A picked row runs once the menu has closed, with the browser's state let go. The folder tree
//! opens its menu through the same `ListMenu` (`open_rows`, `tree/menu.rs`).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::widgets::menu::{MenuEntry, MenuItem, MenuResponse};
use agg_gui::{current_viewport, DrawCtx, Event, EventResult, Point, PopupMenu, Size};

use crate::actions::{ActionScope, BrowserAction, BrowserCommand, BuiltinAction, Builtins};
use crate::builtins::builtin_command;
use crate::look::BrowserLook;
use crate::state::BrowserState;

/// The side of a menu row's icon.
pub const MENU_ICON_SIZE: f64 = 16.0;

/// Which menu opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuScope {
    /// The selection's menu.
    Selection,
    /// The list's own menu, for the open folder.
    Folder,
    /// The overflow ("File") menu: every row.
    Overflow,
}

/// The built-ins offered in `scope`'s menu.
pub(crate) fn builtins_for(scope: MenuScope) -> Builtins {
    let offered: Vec<BuiltinAction> = BuiltinAction::ALL
        .iter()
        .copied()
        .filter(|a| match scope {
            MenuScope::Selection => a.acts_on_selection(),
            MenuScope::Folder => !a.acts_on_selection(),
            MenuScope::Overflow => true,
        })
        .collect();
    Builtins::new(&offered)
}

/// `rows` with each offered built-in made a row that runs, for `state` as it is now (submenus
/// too); a built-in `builtins` doesn't offer is left out.
pub(crate) fn resolve_builtins(
    state: &Rc<RefCell<BrowserState>>,
    rows: Vec<BrowserAction>,
    builtins: &Builtins,
) -> Vec<BrowserAction> {
    rows.into_iter()
        .filter_map(|row| match row {
            BrowserAction::Builtin(action) if builtins.offers(action) => {
                Some(BrowserAction::Command(builtin_command(state, action)))
            }
            BrowserAction::Builtin(_) => None,
            BrowserAction::Command(command) if !command.submenu_rows().is_empty() => {
                let inner = resolve_builtins(state, command.submenu_rows().to_vec(), builtins);
                Some(BrowserAction::Command(command.with_submenu_rows(inner)))
            }
            other => Some(other),
        })
        .collect()
}

/// Whether `scope`'s menu shows `command` (the module header's rules).
fn shows(command: &BrowserCommand, scope: MenuScope) -> bool {
    if command.is_only_when_enabled() && !command.is_enabled() {
        return false;
    }
    let in_scope = |wanted: ActionScope| command.scope().is_none_or(|s| s == wanted);
    match scope {
        MenuScope::Selection => in_scope(ActionScope::Selection) && command.is_enabled(),
        MenuScope::Folder => in_scope(ActionScope::Folder),
        MenuScope::Overflow => true,
    }
}

/// The rows `scope`'s menu shows of `rows` (built-ins already resolved), separators only
/// between two rows kept.
pub fn select_rows(rows: Vec<BrowserAction>, scope: MenuScope) -> Vec<BrowserAction> {
    let mut kept = Vec::new();
    let mut separator_due = false;
    for row in rows {
        match row {
            BrowserAction::Separator => separator_due = !kept.is_empty(),
            BrowserAction::Builtin(_) => {}
            BrowserAction::Command(command) => {
                if !shows(&command, scope) {
                    continue;
                }
                let command = match command.submenu_rows().is_empty() {
                    true => command,
                    false => {
                        let inner = select_rows(command.submenu_rows().to_vec(), scope);
                        if inner.is_empty() {
                            continue;
                        }
                        command.with_submenu_rows(inner)
                    }
                };
                if std::mem::take(&mut separator_due) {
                    kept.push(BrowserAction::Separator);
                }
                kept.push(BrowserAction::Command(command));
            }
        }
    }
    kept
}

/// The rows `scope`'s menu shows for `state` now: the app's ordered list for a snapshot (asked
/// with the state let go), built-ins made rows that run, filtered for the menu.
pub fn menu_actions(state: &Rc<RefCell<BrowserState>>, scope: MenuScope) -> Vec<BrowserAction> {
    let (actions, snapshot) = {
        let s = state.borrow();
        (Rc::clone(&s.config().actions), s.snapshot())
    };
    let builtins = builtins_for(scope);
    let rows = actions.menu(&snapshot, &builtins);
    select_rows(resolve_builtins(state, rows, &builtins), scope)
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
        self.entries()
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item(item) => (item.label.clone(), item.enabled),
                _ => (String::new(), false),
            })
            .collect()
    }

    /// The open menu's entries as built (icons, shortcuts and submenus included); empty while
    /// none is open.
    pub fn entries(&self) -> &[MenuEntry] {
        self.menu.as_ref().map_or(&[], |menu| menu.items.as_slice())
    }

    /// Opens `scope`'s menu at `at` (the opening widget's own coordinates); answers whether it
    /// opened (a menu with no rows does not).
    pub fn open(&mut self, state: &Rc<RefCell<BrowserState>>, scope: MenuScope, at: Point) -> bool {
        let rows = menu_actions(state, scope);
        let look = Rc::clone(&state.borrow().config().look);
        self.open_rows(rows, at, &format!("menu {scope:?}"), look.as_ref())
    }

    /// Opens a menu of `rows` (already resolved and filtered) at `at`, in the opening widget's
    /// own coordinates, its icons from `look`; `what` names it in the trace. A menu with no rows
    /// does not open; answers whether it did.
    pub fn open_rows(
        &mut self,
        rows: Vec<BrowserAction>,
        at: Point,
        what: &str,
        look: &dyn BrowserLook,
    ) -> bool {
        let mut commands = Vec::new();
        let items = menu_entries(rows, look, &mut commands);
        crate::trace(|| format!("{what} opens with {} rows", items.len()));
        if items.is_empty() {
            self.close();
            return false;
        }
        let mut menu = PopupMenu::new(items);
        menu.open_at(at);
        self.menu = Some(menu);
        self.commands = commands;
        agg_gui::animation::request_draw();
        true
    }

    /// Closes the menu.
    pub fn close(&mut self) {
        self.menu = None;
        self.commands.clear();
    }

    /// Routes `event` (the opening widget's own coordinates) to the open menu; `None` while none
    /// is open. A picked command runs once the menu has closed.
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

/// `rows` as agg-gui menu entries, each command's run kept in `commands` under the id its entry
/// carries.
fn menu_entries(
    rows: Vec<BrowserAction>,
    look: &dyn BrowserLook,
    commands: &mut Vec<BrowserCommand>,
) -> Vec<MenuEntry> {
    let tint = look.style().text_color;
    let side = Size::new(MENU_ICON_SIZE, MENU_ICON_SIZE);
    let mut entries = Vec::new();
    for row in rows {
        let command = match row {
            BrowserAction::Separator => {
                entries.push(MenuEntry::Separator);
                continue;
            }
            BrowserAction::Builtin(_) => continue,
            BrowserAction::Command(command) => command,
        };
        let mut item = match command.submenu_rows().is_empty() {
            true => {
                let id = format!("command:{}", commands.len());
                commands.push(command.clone());
                MenuItem::action(command.title().to_string(), id)
            }
            false => {
                let inner = menu_entries(command.submenu_rows().to_vec(), look, commands);
                MenuItem::submenu(command.title().to_string(), inner)
            }
        };
        if !command.is_enabled() {
            item = item.disabled();
        }
        if let Some(icon) = command
            .icon()
            .and_then(|key| look.icon_for_key(key, side, tint))
        {
            item = item.image_icon(icon);
        }
        if let Some(shortcut) = command.shortcut() {
            item = item.shortcut(shortcut);
        }
        if let Some(tooltip) = command.tooltip() {
            item = item.tooltip(tooltip);
        }
        entries.push(item.into());
    }
    entries
}
