//! Right-click menu construction — the built-in "Add Node" / node-context
//! menus, or the host's own node menu (`NodeGraphModel::node_context_menu`,
//! whose chosen items go back through `on_node_context_action`) — and the
//! event-translate helper used by the editor's floating overlays.
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
            self.backbuffer.invalidate();
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
                    self.backbuffer.invalidate();
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
            return open;
        }
        self.popup_host_node = None;
        let mut items = vec![
            MenuEntry::Item(MenuItem::action("Delete", "delete")),
            MenuEntry::Separator,
        ];
        items.extend(build_add_node_popup_items(&self.model));
        self.popup = PopupMenu::new(items);
        true
    }

    /// Rebuild the popup to show only the Add Node submenu — called
    /// when right-click lands on empty canvas.
    pub(super) fn rebuild_popup_for_empty_canvas(&mut self) {
        let items = build_add_node_popup_items(&self.model);
        self.popup = PopupMenu::new(items);
        self.popup_host_node = None;
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
