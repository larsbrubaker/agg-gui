//! Right-click "Add Node" menu construction and the event-translate
//! helper used by the editor's floating overlays.
//!
//! Lives in its own submodule so [`super::mod`] stays under the
//! 800-line guardrail without sacrificing the canvas / state-machine
//! narrative of the parent file.

use agg_gui::{Event, MenuEntry, MenuItem, PopupMenu};

use super::{NodeEditor, NodeId, SharedModel};

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
    pub(super) fn handle_popup_action(&mut self, action: &str) {
        match action {
            "delete" => {
                let to_remove: Vec<NodeId> = self.selected.drain().collect();
                if to_remove.is_empty() {
                    return;
                }
                {
                    let mut model = self.model.lock().unwrap();
                    for id in to_remove {
                        model.remove_node(id);
                    }
                }
                // Removing nodes changes the layout & cached widget
                // tree — neither updates without an explicit
                // invalidate + request_draw.
                self.backbuffer.invalidate();
                agg_gui::animation::request_draw();
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

    /// Rebuild the popup to show the node-context menu — Delete first,
    /// then the Add Node submenu underneath. Called when right-click
    /// lands on an existing node.
    pub(super) fn rebuild_popup_for_node_context(&mut self) {
        let mut items = vec![
            MenuEntry::Item(MenuItem::action("Delete", "delete")),
            MenuEntry::Separator,
        ];
        items.extend(build_add_node_popup_items(&self.model));
        self.popup = PopupMenu::new(items);
    }

    /// Rebuild the popup to show only the Add Node submenu — called
    /// when right-click lands on empty canvas.
    pub(super) fn rebuild_popup_for_empty_canvas(&mut self) {
        let items = build_add_node_popup_items(&self.model);
        self.popup = PopupMenu::new(items);
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
