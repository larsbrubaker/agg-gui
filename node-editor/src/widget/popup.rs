//! Right-click menu construction — the built-in "Add Node" / node-context
//! menus (the add menu's sources are chosen in `add_menu`), or the host's
//! own node menu (`NodeGraphModel::node_context_menu`,
//! whose chosen items go back through `on_node_context_action`) — and the
//! event-translate helper used by the editor's floating overlays. While the
//! menu is open the editor claims the pointer over it (`popup_contains`), so
//! the hosted cards under it see none of the presses and moves meant for it.
//!
//! Lives in its own submodule so [`super::mod`] stays under the
//! 800-line guardrail without sacrificing the canvas / state-machine
//! narrative of the parent file.

use agg_gui::{Event, MenuEntry, MenuItem, PopupMenu};

use super::{NodeEditor, SharedModel};
use crate::model::NodeId;

/// Subtract `(dx, dy)` from any mouse-position field on `event` so an
/// overlay positioned at `(dx, dy)` in editor-local space sees events
/// in its own local space (mirrors what `dispatch_event` does for the
/// children Vec).  Returns the original event for non-mouse variants.
pub(super) fn translate_event_into(event: &Event, dx: f64, dy: f64) -> Event {
    use agg_gui::Point;
    match event {
        Event::MouseDown {
            pos,
            button,
            modifiers,
        } => Event::MouseDown {
            pos: Point::new(pos.x - dx, pos.y - dy),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseUp {
            pos,
            button,
            modifiers,
        } => Event::MouseUp {
            pos: Point::new(pos.x - dx, pos.y - dy),
            button: *button,
            modifiers: *modifiers,
        },
        Event::MouseMove { pos } => Event::MouseMove {
            pos: Point::new(pos.x - dx, pos.y - dy),
        },
        Event::MouseWheel {
            pos,
            delta_x,
            delta_y,
            modifiers,
        } => Event::MouseWheel {
            pos: Point::new(pos.x - dx, pos.y - dy),
            delta_x: *delta_x,
            delta_y: *delta_y,
            modifiers: *modifiers,
        },
        other => other.clone(),
    }
}

impl NodeEditor {
    /// Action callback for the right-click popup — handles
    /// `"add.{type_id}"` and `"delete"` entries by routing through
    /// the model.
    ///
    /// When the open menu came from the host
    /// (`NodeGraphModel::node_context_menu`), every action goes to
    /// `NodeGraphModel::on_node_context_action` instead, and the command it
    /// returns (if any) is applied once the model lock is released.
    pub(super) fn handle_popup_action(&mut self, action: &str) {
        if let Some(node) = self.popup_host_node {
            let command = self
                .model
                .lock()
                .unwrap()
                .on_node_context_action(node, action);
            if let Some(command) = command {
                self.apply_command(command);
            }
            self.invalidate_canvas();
            agg_gui::animation::request_draw();
            return;
        }
        if self.add_menu.host_popup {
            // The host's add menu (`NodeGraphModel::add_menu`).
            let pos = self.popup_canvas_pos;
            let command = self.model.lock().unwrap().on_add_menu_action(action, pos);
            if let Some(command) = command {
                self.apply_command(command);
            }
            self.invalidate_canvas();
            agg_gui::animation::request_draw();
            return;
        }
        match action {
            "delete" => {
                // Shares `NodeEditor::delete_selection` with the Delete
                // key and the host command queue, so all three paths
                // remove, invalidate, and repaint identically.
                self.delete_selection();
            }
            other => {
                if let Some(type_id) = other.strip_prefix("add.") {
                    let pos = self.popup_canvas_pos;
                    {
                        let mut model = self.model.lock().unwrap();
                        let _ = model.add_node(type_id, pos);
                    }
                    self.invalidate_canvas();
                    agg_gui::animation::request_draw();
                }
            }
        }
    }

    /// Rebuild the popup to show the context menu of `node` — the host's
    /// (`NodeGraphModel::node_context_menu`) when it supplies one, else
    /// Delete first, then the Add Node submenu underneath. Called when
    /// right-click lands on an existing node. Returns `false` when the
    /// menu is empty (the host returned no items): no menu opens.
    pub(super) fn rebuild_popup_for_node_context(&mut self, node: NodeId) -> bool {
        let host_menu = self.model.lock().unwrap().node_context_menu(node);
        if let Some(items) = host_menu {
            let open = !items.is_empty();
            self.popup = PopupMenu::new(items);
            self.popup_host_node = Some(node);
            self.add_menu.host_popup = false;
            self.add_menu.in_popup = false;
            return open;
        }
        self.popup_host_node = None;
        self.add_menu.host_popup = false;
        self.add_menu.in_popup = false;
        let mut items = vec![
            MenuEntry::Item(MenuItem::action("Delete", "delete")),
            MenuEntry::Separator,
        ];
        items.extend(build_add_node_popup_items(&self.model));
        self.popup = PopupMenu::new(items);
        true
    }

    /// Whether `pos` (editor-local, Y up) lies on the open right-click menu:
    /// any of its panels, the open submenus' included. The editor claims the
    /// pointer there (`claims_pointer_exclusively`), so a press or move over a
    /// menu row reaches the menu, never the hosted card's widgets drawn under
    /// it (a field the click would focus, a button it would fire).
    pub(super) fn popup_contains(&self, pos: agg_gui::Point) -> bool {
        if !self.popup.is_open() {
            return false;
        }
        let viewport = agg_gui::Size::new(self.bounds.width, self.bounds.height);
        let layouts = self.popup.state.layouts(&self.popup.items, viewport);
        agg_gui::widgets::menu::geometry::hit_test(&layouts, pos).is_some()
    }

    /// The right-click menu while it is open (editor-local coordinates,
    /// Y up), so a host's automation can find a row by its action string
    /// (the name a host gives its items) and reach its rectangle.
    pub fn open_context_menu(&self) -> Option<&PopupMenu> {
        self.popup.is_open().then_some(&self.popup)
    }
}

/// Build the right-click "Add Node" menu — category-grouped submenus
/// containing every type the model exposes.  Action ids are
/// `"add.{type_id}"`.
pub(super) fn build_add_node_popup_items(model: &SharedModel) -> Vec<MenuEntry> {
    let m = model.lock().unwrap();
    let mut out = Vec::new();
    for (cat, defs) in m.node_types_by_category() {
        if defs.is_empty() {
            continue;
        }
        let items = defs
            .iter()
            .map(|d| {
                MenuEntry::Item(MenuItem::action(
                    d.display_name.clone(),
                    format!("add.{}", d.type_id),
                ))
            })
            .collect();
        out.push(MenuEntry::Item(MenuItem::submenu(cat, items)));
    }
    out
}
