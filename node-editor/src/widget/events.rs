//! Mouse / wheel / keyboard handlers for [`NodeEditor`].
//!
//! Split out of `widget/mod.rs` (which kept `paint` and the `Widget`
//! trait dispatcher) so each file stays under the 800-line guardrail.
//! As a submodule of `widget`, this file retains direct access to
//! `NodeEditor`'s private fields and helper methods.
//!
//! The state machine lives in [`super::CanvasState`]; transitions
//! happen here on mouse down / move / up.

use agg_gui::{EventResult, Key, Modifiers, MouseButton, Point};

use crate::draw::SocketSide;
use crate::model::{NodeId, PropertyValue};

use super::{CanvasState, NodeEditor, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP};

impl NodeEditor {
    pub(super) fn on_mouse_down(
        &mut self,
        pos: Point,
        button: MouseButton,
        modifiers: Modifiers,
    ) -> EventResult {
        let canvas_pos = self.local_to_canvas(pos);
        let layouts = self.snapshot_layouts();

        match button {
            MouseButton::Left => {
                if self.space_held {
                    self.interaction = CanvasState::PanningCanvas {
                        start_offset: self.canvas_offset,
                        start_local: pos,
                    };
                    return EventResult::Consumed;
                }
                if let Some((node_id, socket)) = self.hit_socket(&layouts, canvas_pos) {
                    self.interaction = CanvasState::DrawingConnection {
                        from_node: node_id,
                        from_socket: socket.name.clone(),
                        from_canvas: socket.center,
                        cursor_canvas: canvas_pos,
                        from_socket_type: socket.socket_type,
                        from_side: socket.side,
                    };
                    return EventResult::Consumed;
                }
                // Property row?
                if let Some((node_id, prop)) = self.hit_property(&layouts, canvas_pos) {
                    if let PropertyValue::Number(start) = prop.current {
                        self.selected.clear();
                        self.selected.insert(node_id);
                        self.notify_primary_selection(Some(node_id));
                        self.interaction = CanvasState::DraggingProperty {
                            node_id,
                            prop_name: prop.name.clone(),
                            start_value: start,
                            start_local_x: pos.x,
                            min: prop.min,
                            max: prop.max,
                        };
                        return EventResult::Consumed;
                    }
                    if let PropertyValue::Bool(b) = prop.current {
                        self.model.lock().unwrap().set_property(
                            node_id,
                            &prop.name,
                            PropertyValue::Bool(!b),
                        );
                        return EventResult::Consumed;
                    }
                }
                if let Some(node_id) = self.hit_node(&layouts, canvas_pos) {
                    if !modifiers.shift && !self.selected.contains(&node_id) {
                        self.selected.clear();
                    }
                    self.selected.insert(node_id);
                    self.notify_primary_selection(Some(node_id));
                    self.begin_drag_node(node_id, canvas_pos);
                    return EventResult::Consumed;
                }
                if !modifiers.shift {
                    self.selected.clear();
                    self.notify_primary_selection(None);
                }
                EventResult::Consumed
            }
            MouseButton::Middle => {
                self.interaction = CanvasState::PanningCanvas {
                    start_offset: self.canvas_offset,
                    start_local: pos,
                };
                EventResult::Consumed
            }
            MouseButton::Right => {
                self.popup_canvas_pos = canvas_pos;
                self.popup.open_at(pos);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    pub(super) fn on_mouse_move(&mut self, pos: Point) -> EventResult {
        let canvas_pos = self.local_to_canvas(pos);
        // Every non-Idle branch below mutates visible state — pan
        // offset, dragged node positions, in-flight wire endpoint,
        // or a property value displayed inside a node.  Reactive
        // hosts (AtomArtist's native shell, the agg-gui demo) sleep
        // the event loop until `animation::wants_draw()` returns
        // true, so each handler must claim a redraw or its mutation
        // will never appear on screen between mouse events.  Hover
        // (`Idle`) deliberately does NOT claim — keeping plain
        // pointer motion free of repaints matches agg-gui's demo.
        let result = match &mut self.interaction {
            CanvasState::PanningCanvas {
                start_offset,
                start_local,
            } => {
                self.canvas_offset = [
                    start_offset[0] + (pos.x - start_local.x),
                    start_offset[1] + (pos.y - start_local.y),
                ];
                EventResult::Consumed
            }
            CanvasState::DraggingNode {
                ids,
                start_positions,
                start_canvas,
            } => {
                let dx = canvas_pos[0] - start_canvas[0];
                let dy = canvas_pos[1] - start_canvas[1];
                let mut model = self.model.lock().unwrap();
                for (id, p0) in ids.iter().zip(start_positions.iter()) {
                    model.set_node_position(*id, [p0[0] + dx, p0[1] + dy]);
                }
                EventResult::Consumed
            }
            CanvasState::DrawingConnection { cursor_canvas, .. } => {
                *cursor_canvas = canvas_pos;
                EventResult::Consumed
            }
            CanvasState::DraggingProperty {
                node_id,
                prop_name,
                start_value,
                start_local_x,
                min,
                max,
            } => {
                let dx = pos.x - *start_local_x;
                let mut new_value = *start_value + dx;
                if let Some(mn) = *min {
                    if new_value < mn {
                        new_value = mn;
                    }
                }
                if let Some(mx) = *max {
                    if new_value > mx {
                        new_value = mx;
                    }
                }
                let id = *node_id;
                let name = prop_name.clone();
                self.model.lock().unwrap().set_property(
                    id,
                    &name,
                    PropertyValue::Number(new_value),
                );
                EventResult::Consumed
            }
            CanvasState::Idle => EventResult::Ignored,
        };
        if result == EventResult::Consumed {
            agg_gui::animation::request_draw();
        }
        result
    }

    pub(super) fn on_mouse_up(
        &mut self,
        pos: Point,
        button: MouseButton,
        _modifiers: Modifiers,
    ) -> EventResult {
        let canvas_pos = self.local_to_canvas(pos);
        match (
            button,
            std::mem::replace(&mut self.interaction, CanvasState::Idle),
        ) {
            (
                MouseButton::Left,
                CanvasState::DrawingConnection {
                    from_node,
                    from_socket,
                    from_socket_type,
                    from_side,
                    ..
                },
            )
            | (
                MouseButton::Middle,
                CanvasState::DrawingConnection {
                    from_node,
                    from_socket,
                    from_socket_type,
                    from_side,
                    ..
                },
            ) => {
                let layouts = self.snapshot_layouts();
                if let Some((target_node, target_socket)) = self.hit_socket(&layouts, canvas_pos) {
                    let model = self.model.lock().unwrap();
                    let compatible =
                        model.sockets_compatible(from_socket_type, target_socket.socket_type);
                    drop(model);
                    if target_node != from_node && compatible {
                        let (out_node, out_sock, in_node, in_sock) =
                            match (from_side, target_socket.side) {
                                (SocketSide::Output, SocketSide::Input) => (
                                    from_node,
                                    from_socket.clone(),
                                    target_node,
                                    target_socket.name.clone(),
                                ),
                                (SocketSide::Input, SocketSide::Output) => (
                                    target_node,
                                    target_socket.name.clone(),
                                    from_node,
                                    from_socket.clone(),
                                ),
                                _ => return EventResult::Consumed,
                            };
                        let _ = self
                            .model
                            .lock()
                            .unwrap()
                            .try_add_edge(out_node, &out_sock, in_node, &in_sock);
                    }
                }
                EventResult::Consumed
            }
            (_, CanvasState::DraggingNode { .. })
            | (_, CanvasState::PanningCanvas { .. })
            | (_, CanvasState::DraggingProperty { .. }) => EventResult::Consumed,
            (_, _) => EventResult::Ignored,
        }
    }

    pub(super) fn on_wheel(
        &mut self,
        pos: Point,
        delta_y: f64,
        _modifiers: Modifiers,
    ) -> EventResult {
        if delta_y == 0.0 {
            return EventResult::Ignored;
        }
        let canvas_before = self.local_to_canvas(pos);
        let factor = if delta_y > 0.0 {
            ZOOM_STEP
        } else {
            1.0 / ZOOM_STEP
        };
        let new_scale = (self.canvas_scale * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        if (new_scale - self.canvas_scale).abs() < 1e-9 {
            // Zoom clamped — nothing visible changed, so don't
            // claim a redraw.  Still Consumed so the host's
            // outer scroll handler doesn't bubble the wheel up.
            return EventResult::Consumed;
        }
        self.canvas_offset = [
            pos.x - canvas_before[0] * new_scale,
            pos.y - canvas_before[1] * new_scale,
        ];
        self.canvas_scale = new_scale;
        self.model.lock().unwrap().on_canvas_zoom_changed(new_scale);
        agg_gui::animation::request_draw();
        EventResult::Consumed
    }

    pub(super) fn on_key_down(&mut self, key: &Key, _mods: Modifiers) -> EventResult {
        match key {
            Key::Char(' ') => {
                self.space_held = true;
                EventResult::Consumed
            }
            Key::Delete => {
                if self.selected.is_empty() {
                    return EventResult::Ignored;
                }
                let to_remove: Vec<NodeId> = self.selected.drain().collect();
                let mut model = self.model.lock().unwrap();
                for id in to_remove {
                    model.remove_node(id);
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    pub(super) fn on_key_up(&mut self, key: &Key, _mods: Modifiers) -> EventResult {
        if let Key::Char(' ') = key {
            self.space_held = false;
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    pub(super) fn notify_primary_selection(&self, id: Option<NodeId>) {
        self.model.lock().unwrap().on_primary_selection_changed(id);
    }
}
