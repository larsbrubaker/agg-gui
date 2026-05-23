//! Mouse / wheel / keyboard handlers for [`NodeEditor`].
//!
//! Split out of `widget/mod.rs` (which kept `paint` and the `Widget`
//! trait dispatcher) so each file stays under the 800-line guardrail.
//! As a submodule of `widget`, this file retains direct access to
//! `NodeEditor`'s private fields and helper methods.
//!
//! The state machine lives in [`super::CanvasState`]; transitions
//! happen here on mouse down / move / up.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{Color, EventResult, Key, Modifiers, MouseButton, Point};

use crate::draw::SocketSide;
use crate::model::{EditorHint, NodeId, PropertyValue};

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
                    // Color row with the `Color` editor hint opens the
                    // ColorWheelPicker dialog as a floating overlay.
                    if matches!(prop.editor, Some(EditorHint::Color)) {
                        if let PropertyValue::Color(rgba) = prop.current {
                            self.open_color_picker(node_id, prop.name.clone(), rgba);
                            return EventResult::Consumed;
                        }
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
        //
        // Snap-layout's drag path needs the full layout snapshot
        // (every node's canvas rect) to compute alignment / spacing
        // targets.  Grab it BEFORE the match's mutable borrow of
        // `self.interaction` — the snapshot helper takes `&self` and
        // borrow-checker can't see that the immutable read finishes
        // before the closure body runs otherwise.  Cheap to take
        // unconditionally (matches the snap-disabled path below).
        let layouts_snapshot = self.snapshot_layouts();
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
                // Raw new positions (before snap).  `position` is the
                // node's top-left in canvas coords (Y-up: position[1]
                // is the TOP edge).
                let mut new_positions: Vec<[f64; 2]> = start_positions
                    .iter()
                    .map(|p0| [p0[0] + dx, p0[1] + dy])
                    .collect();
                // Snap pass — only for single-node drags.  Multi-node
                // drag would need to snap the bounding box of the
                // selection; that's a future extension.  Skipped
                // entirely when the global snap toggle is off, which
                // keeps the drag path cheap.
                if ids.len() == 1 && agg_gui::snap::is_enabled() {
                    snap_single_node(ids[0], &mut new_positions[0], &layouts_snapshot);
                }
                let mut model = self.model.lock().unwrap();
                for (id, p) in ids.iter().zip(new_positions.iter()) {
                    model.set_node_position(*id, *p);
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
                            .try_add_noodle(out_node, &out_sock, in_node, &in_sock);
                    }
                }
                EventResult::Consumed
            }
            (_, CanvasState::DraggingNode { .. }) => {
                // Drag ended — clear any snap guides the drag handler
                // wrote during the move, so the overlay doesn't keep
                // painting a stale alignment line.
                agg_gui::snap::clear_guides();
                EventResult::Consumed
            }
            (_, CanvasState::PanningCanvas { .. })
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

    /// Spawn the [`agg_gui::ColorWheelPicker`] dialog as a floating
    /// overlay over the canvas.  The picker's callbacks route writes
    /// back through `set_property` for live preview / commit / cancel
    /// and flip a shared close-flag that the editor drains on the next
    /// event or layout pass.
    pub(super) fn open_color_picker(
        &mut self,
        node_id: NodeId,
        prop_name: String,
        initial: [f32; 4],
    ) {
        let Some(font) = agg_gui::font_settings::current_system_font() else {
            return;
        };
        let initial_color = Color::rgba(initial[0], initial[1], initial[2], initial[3]);
        let original = initial; // captured for `on_cancel` revert

        let model_change = Arc::clone(&self.model);
        let model_select = Arc::clone(&self.model);
        let model_cancel = Arc::clone(&self.model);
        let name_change = prop_name.clone();
        let name_select = prop_name.clone();
        let name_cancel = prop_name;
        let close_flag = Rc::new(Cell::new(false));
        let close_select = Rc::clone(&close_flag);
        let close_cancel = Rc::clone(&close_flag);

        let picker = agg_gui::ColorWheelPicker::new(initial_color, font.clone())
            .with_allow_none(false)
            .with_show_alpha(true)
            .on_change(move |c| {
                let value = color_to_property(c, original);
                model_change
                    .lock()
                    .unwrap()
                    .set_property(node_id, &name_change, value);
            })
            .on_select(move |c| {
                let value = color_to_property(c, original);
                model_select
                    .lock()
                    .unwrap()
                    .set_property(node_id, &name_select, value);
                close_select.set(true);
            })
            .on_cancel(move || {
                model_cancel.lock().unwrap().set_property(
                    node_id,
                    &name_cancel,
                    PropertyValue::Color(original),
                );
                close_cancel.set(true);
            });

        let dialog = agg_gui::color_wheel_picker_dialog(picker, "Color Picker");

        // If a host sink is installed (AtomArtist's app shell does
        // this), hand the dialog off so it can live at the
        // screen-level Stack — that's what lets the user drag the
        // picker outside the editor pane. Otherwise fall back to the
        // legacy in-editor overlay path (gallery demo + tests rely
        // on this).
        if let Some(sink) = self.overlay_sink.as_mut() {
            sink(dialog, close_flag);
        } else {
            self.overlay = Some(dialog);
            self.overlay_close_flag = Some(close_flag);
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }
}

/// Pack a picker-side `Option<Color>` back into a `PropertyValue::Color`,
/// falling back to `original.a = 0.0` for the pass-through ("No Color")
/// case so hosts that don't model pass-through still see a sensible
/// zero-alpha colour.
fn color_to_property(c: Option<Color>, original: [f32; 4]) -> PropertyValue {
    match c {
        Some(col) => PropertyValue::Color([col.r, col.g, col.b, col.a]),
        None => PropertyValue::Color([original[0], original[1], original[2], 0.0]),
    }
}

/// Run a single-node drag through the snap engine and overwrite
/// `position` with the snapped top-left corner.
///
/// Node positions are stored as `[x, y]` where `y` is the **top** edge
/// in Y-up canvas coords; the snap engine works in `Rect`s whose `y`
/// is the BOTTOM edge.  Conversion happens at the boundaries here so
/// the rest of the drag path keeps thinking in the node convention.
///
/// Guides are written into the framework's thread-local snap
/// registry; `NodeEditor::paint` reads them inside the canvas
/// transform to render alignment / spacing lines.
fn snap_single_node(
    moving_id: NodeId,
    position: &mut [f64; 2],
    layouts: &[crate::draw::NodeLayoutInfo],
) {
    use agg_gui::{compute_snap, snap, Rect, SnapId, SnapMode};
    let Some(moving_layout) = layouts.iter().find(|l| l.node_id == moving_id) else {
        return;
    };
    let size = moving_layout.size;
    let raw_top_left = *position;
    let moving_rect = Rect::new(
        raw_top_left[0],
        raw_top_left[1] - size[1],
        size[0],
        size[1],
    );
    let targets: Vec<(SnapId, Rect)> = layouts
        .iter()
        .filter(|l| l.node_id != moving_id)
        .map(|l| {
            (
                SnapId(l.node_id.0),
                Rect::new(l.top_left[0], l.top_left[1] - l.size[1], l.size[0], l.size[1]),
            )
        })
        .collect();
    let result = compute_snap(
        moving_rect,
        SnapId(moving_id.0),
        &targets,
        snap::DEFAULT_THRESHOLD,
        SnapMode::Move,
    );
    // Convert the snapped rect back to top-left position.
    *position = [result.rect.x, result.rect.y + result.rect.height];
    snap::set_guides(result.guides);
}
