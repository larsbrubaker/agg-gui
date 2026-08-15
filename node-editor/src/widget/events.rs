//! Mouse / wheel / keyboard handlers for [`NodeEditor`].
//!
//! Split out of `widget/mod.rs` (which kept `paint` and the `Widget`
//! trait dispatcher) so each file stays under the 800-line guardrail.
//! As a submodule of `widget`, this file retains direct access to
//! `NodeEditor`'s private fields and helper methods.
//!
//! The state machine lives in [`super::CanvasState`]; transitions
//! happen here on mouse down / move / up.

use agg_gui::widgets::EditorKind;
use agg_gui::{EventResult, Key, Modifiers, MouseButton, Point};

use crate::draw::{NodeLayoutInfo, SocketSide, TITLE_HEIGHT};
use crate::model::{EditorHint, NodeId, PropertyValue};

use super::overlay_editors::scrub_value;

/// Window for double-click detection in milliseconds — matches the
/// constant `agg_gui::widgets::window::DBL_CLICK_MS` so a click in a
/// Window title bar and a click in a node title bar feel identical.
const DBL_CLICK_MS: u128 = 500;

/// Horizontal distance (logical px) a NumberDrag press must travel
/// before it counts as a scrub rather than a click. Mirrors
/// `DragValue::DRAG_THRESHOLD` so the row control feels identical to the
/// standalone widget.
const PROP_DRAG_THRESHOLD: f64 = 3.0;

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
                    // Click on a connected INPUT socket = disconnect-
                    // by-drag: pop the existing noodle off and start
                    // a re-attach drag from the noodle's SOURCE
                    // socket. Releasing on empty canvas leaves the
                    // noodle removed; releasing on a compatible socket
                    // re-routes it. Matches the canonical NodeDesigner
                    // "grab the input end of the wire" interaction.
                    if socket.side == SocketSide::Input {
                        let connected = self
                            .model
                            .lock()
                            .unwrap()
                            .noodles()
                            .iter()
                            .find(|n| n.to_node == node_id && n.to_socket == socket.name)
                            .cloned();
                        if let Some(noodle) = connected {
                            // Find the source socket layout so the
                            // drag starts at the actual output dot.
                            let from_socket_layout = layouts
                                .iter()
                                .find(|l| l.node_id == noodle.from_node)
                                .and_then(|l| {
                                    l.sockets().find(|s| {
                                        s.side == SocketSide::Output && s.name == noodle.from_socket
                                    })
                                })
                                .cloned();
                            self.model.lock().unwrap().remove_noodle(
                                noodle.from_node,
                                &noodle.from_socket,
                                noodle.to_node,
                                &noodle.to_socket,
                            );
                            if let Some(src) = from_socket_layout {
                                self.interaction = CanvasState::DrawingConnection {
                                    from_node: noodle.from_node,
                                    from_socket: noodle.from_socket.clone(),
                                    from_canvas: src.center,
                                    cursor_canvas: canvas_pos,
                                    from_socket_type: src.socket_type,
                                    from_side: SocketSide::Output,
                                };
                                agg_gui::animation::request_draw();
                                return EventResult::Consumed;
                            }
                        }
                    }
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
                        // Resolve the numeric editor contract. Slider rows
                        // keep the immediate-scrub NodeDesigner behaviour;
                        // every other numeric row (explicit NumberDrag or
                        // the implicit default) follows the DragValue
                        // contract — threshold, step snap, click-to-edit.
                        let attrs = prop.editor_kind.as_ref().and_then(|k| k.number_attrs());
                        let is_slider = matches!(prop.editor_kind, Some(EditorKind::Slider(_)));
                        // Step snapping is a NumberDrag-only behaviour. Slider
                        // rows keep their historical *continuous* scrub even
                        // when a step attr is present, so gate the step on
                        // `!is_slider` — otherwise `scrub_value` would quantise
                        // a slider drag and contradict the `DraggingProperty`
                        // doc ("Slider rows ... no step snapping").
                        let step = if is_slider {
                            None
                        } else {
                            attrs.and_then(|a| a.step.or(if a.integer { Some(1.0) } else { None }))
                        };
                        let decimals = attrs
                            .map(|a| {
                                if a.integer {
                                    0
                                } else {
                                    a.max_decimal_places.map(|n| n as usize).unwrap_or(2)
                                }
                            })
                            .unwrap_or(2);
                        // Fall back to the NumberAttrs range when the model's
                        // PropertyView didn't carry an explicit min/max. These
                        // two sources can disagree — a host could set a
                        // PropertyView min above the NumberAttrs max — yielding
                        // an inverted `min > max` pair. `scrub_value` resolves
                        // that deliberately (pins to max, no panic); see its
                        // docs. The debug_assert here surfaces the
                        // contradiction at its source during development.
                        let min = prop.min.or_else(|| attrs.and_then(|a| a.min));
                        let max = prop.max.or_else(|| attrs.and_then(|a| a.max));
                        debug_assert!(
                            match (min, max) {
                                (Some(mn), Some(mx)) => mn <= mx,
                                _ => true,
                            },
                            "numeric row {:?} has inverted bounds: min {:?} > max {:?}",
                            prop.name,
                            min,
                            max
                        );
                        self.interaction = CanvasState::DraggingProperty {
                            node_id,
                            prop_name: prop.name.clone(),
                            start_value: start,
                            start_local_x: pos.x,
                            min,
                            max,
                            step,
                            decimals,
                            // Slider scrubs from the first pixel; NumberDrag
                            // waits for the 3px threshold so a plain click
                            // can open the inline editor instead.
                            dragging: is_slider,
                            click_to_edit: !is_slider,
                            // Canvas-space editor pill rect, so a click-to-edit
                            // release drops the inline editor exactly over it.
                            pill_rect: [
                                prop.top_left[0],
                                prop.top_left[1],
                                prop.size[0],
                                prop.size[1],
                            ],
                        };
                        return EventResult::Consumed;
                    }
                    if let PropertyValue::Bool(b) = prop.current {
                        self.model.lock().unwrap().set_property(
                            node_id,
                            &prop.name,
                            PropertyValue::Bool(!b),
                        );
                        // Toggle flip changes both the editor's pill
                        // paint AND the visible-row set (other rows
                        // gate on this property's value via the host
                        // visibility hook). Force a redraw so the
                        // change actually shows up on screen.
                        agg_gui::animation::request_draw();
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
                    // Text row opens a single-line text editor as a
                    // floating overlay — mirrors the colour-picker route.
                    if let PropertyValue::Text(current) = &prop.current {
                        self.selected.clear();
                        self.selected.insert(node_id);
                        self.notify_primary_selection(Some(node_id));
                        let pill_rect = [
                            prop.top_left[0],
                            prop.top_left[1],
                            prop.size[0],
                            prop.size[1],
                        ];
                        self.open_text_editor(
                            node_id,
                            prop.name.clone(),
                            current.clone(),
                            pill_rect,
                        );
                        return EventResult::Consumed;
                    }
                }
                if let Some(node_id) = self.hit_node(&layouts, canvas_pos) {
                    // (Chevron click is consumed by `ChevronWidget` —
                    // a real child of the node header — before it ever
                    // reaches this canvas-space dispatcher.  The
                    // resulting "collapse this node" signal is drained
                    // in `NodeEditor::layout`.)
                    if hit_title_bar(&layouts, node_id, canvas_pos) {
                        let now = web_time::Instant::now();
                        let is_double = self
                            .last_click
                            .as_ref()
                            .map(|(prev_pos, prev_time)| {
                                let dt = now.duration_since(*prev_time).as_millis();
                                let dx = (pos.x - prev_pos.x).abs();
                                let dy = (pos.y - prev_pos.y).abs();
                                dt <= DBL_CLICK_MS && dx < 6.0 && dy < 6.0
                            })
                            .unwrap_or(false);
                        if is_double {
                            self.last_click = None;
                            // Give the host first crack at the
                            // double-click: it may navigate into a
                            // subgraph / drill into a component. Only
                            // when it declines (returns false) do we
                            // apply the built-in collapse toggle.
                            let handled = self.model.lock().unwrap().on_node_activated(node_id);
                            if !handled {
                                self.toggle_collapsed(node_id);
                            }
                            return EventResult::Consumed;
                        }
                        self.last_click = Some((pos, now));
                    }
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
                // Right-click on a node selects it and offers a
                // node-context menu (Delete + Add Node submenu); on
                // empty canvas, the plain Add-Node menu opens.
                if let Some(node_id) = self.hit_node(&layouts, canvas_pos) {
                    if !modifiers.shift {
                        self.selected.clear();
                    }
                    self.selected.insert(node_id);
                    self.notify_primary_selection(Some(node_id));
                    self.rebuild_popup_for_node_context();
                } else {
                    self.rebuild_popup_for_empty_canvas();
                }
                self.popup.open_at(pos);
                // Opening the popup must invalidate or the menu will
                // not paint until the next unrelated event triggers a
                // redraw.
                agg_gui::animation::request_draw();
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
                // Hosts that map an outside-the-editor pointer into
                // canvas space (a library panel dragging onto the
                // canvas) need the live pan, not just the zoom.
                self.model
                    .lock()
                    .unwrap()
                    .on_canvas_pan_changed(self.canvas_offset);
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
                step,
                dragging,
                ..
            } => {
                let dx = pos.x - *start_local_x;
                // NumberDrag rows don't scrub until the pointer passes the
                // click/drag threshold — that reserved slack is what lets a
                // plain click fall through to the inline editor on release.
                if !*dragging {
                    if dx.abs() < PROP_DRAG_THRESHOLD {
                        // Still within the click zone — consume (we own the
                        // press) but leave the value untouched.
                        EventResult::Consumed
                    } else {
                        *dragging = true;
                        let value = scrub_value(*start_value, dx, *step, *min, *max);
                        let id = *node_id;
                        let name = prop_name.clone();
                        self.model.lock().unwrap().set_property(
                            id,
                            &name,
                            PropertyValue::Number(value),
                        );
                        EventResult::Consumed
                    }
                } else {
                    let value = scrub_value(*start_value, dx, *step, *min, *max);
                    let id = *node_id;
                    let name = prop_name.clone();
                    self.model.lock().unwrap().set_property(
                        id,
                        &name,
                        PropertyValue::Number(value),
                    );
                    EventResult::Consumed
                }
            }
            CanvasState::Idle => EventResult::Ignored,
        };
        if result.is_consumed() {
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
                                _ => {
                                    self.backbuffer.invalidate();
                                    agg_gui::animation::request_draw();
                                    return EventResult::Consumed;
                                }
                            };
                        let _ = self
                            .model
                            .lock()
                            .unwrap()
                            .try_add_noodle(out_node, &out_sock, in_node, &in_sock);
                    }
                }
                // Whether the drop landed on a socket or empty
                // canvas, the dangling bezier we were drawing during
                // the drag has to disappear. Invalidate the cached
                // backbuffer + request a redraw so the canvas
                // repaints without the in-flight line.
                self.backbuffer.invalidate();
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            (_, CanvasState::DraggingNode { .. }) => {
                // Drag ended — clear any snap guides the drag handler
                // wrote during the move, then force a repaint.
                //
                // The canvas retains its painted pixels in a GL FBO
                // backbuffer that's only re-rasterised when the
                // fingerprint changes (model state, selection, etc.).
                // Clearing the guide list doesn't touch the
                // fingerprint, so without an explicit invalidate the
                // next paint blits the cached image — including the
                // stale guides drawn during the drag.  Invalidate +
                // request_draw together ensure the next frame
                // re-rasters with an empty guide list and the host
                // event loop wakes up to paint it.
                agg_gui::snap::clear_guides();
                self.backbuffer.invalidate();
                agg_gui::animation::request_draw();
                EventResult::Consumed
            }
            (_, CanvasState::PanningCanvas { .. }) => EventResult::Consumed,
            (
                _,
                CanvasState::DraggingProperty {
                    node_id,
                    prop_name,
                    start_value,
                    min,
                    max,
                    step,
                    decimals,
                    dragging,
                    click_to_edit,
                    pill_rect,
                    ..
                },
            ) => {
                // A NumberDrag press released before the drag threshold is
                // a plain click → open the inline keyboard editor. Slider
                // rows (and any drag that scrubbed) fall through — the
                // value already committed live during the move.
                if click_to_edit && !dragging {
                    self.open_number_editor(
                        node_id,
                        prop_name,
                        start_value,
                        min,
                        max,
                        step,
                        decimals,
                        pill_rect,
                    );
                }
                EventResult::Consumed
            }
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
        {
            // A cursor-anchored zoom moves the pan too, so both hooks
            // fire — under one lock, so a host that recomputes from the
            // pair never sees a half-updated view.
            let mut model = self.model.lock().unwrap();
            model.on_canvas_pan_changed(self.canvas_offset);
            model.on_canvas_zoom_changed(new_scale);
        }
        agg_gui::animation::request_draw();
        EventResult::Consumed
    }

    pub(super) fn on_key_down(&mut self, key: &Key, _mods: Modifiers) -> EventResult {
        match key {
            Key::Char(' ') => {
                self.space_held = true;
                EventResult::Consumed
            }
            Key::Delete | Key::Backspace => {
                // Backspace is the canonical "delete selection" key on
                // macOS; Delete on Windows / Linux. Accepting both keeps
                // the muscle memory consistent across platforms.
                if self.selected.is_empty() {
                    return EventResult::Ignored;
                }
                let to_remove: Vec<NodeId> = self.selected.drain().collect();
                {
                    let mut model = self.model.lock().unwrap();
                    for id in to_remove {
                        model.remove_node(id);
                    }
                }
                // Removing a node invalidates the cached child widget
                // tree and the GL backbuffer — neither will update
                // without an explicit request.
                self.backbuffer.invalidate();
                agg_gui::animation::request_draw();
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

// The floating overlay editors (`open_color_picker`, `open_text_editor`,
// `open_number_editor`) plus the shared numeric helpers live in
// `overlay_editors.rs` to keep this file under the 800-line guardrail.

/// True when `canvas_pos` lands inside the title-bar strip of the given
/// node's layout. The title bar occupies the top [`TITLE_HEIGHT`] of
/// the node body in canvas-space (Y-up: `top_left.y` is the top edge).
fn hit_title_bar(layouts: &[NodeLayoutInfo], node_id: NodeId, canvas_pos: [f64; 2]) -> bool {
    let Some(l) = layouts.iter().find(|l| l.node_id == node_id) else {
        return false;
    };
    let x0 = l.top_left[0];
    let x1 = x0 + l.size[0];
    let y_top = l.top_left[1];
    let y_bot = y_top - TITLE_HEIGHT;
    canvas_pos[0] >= x0 && canvas_pos[0] <= x1 && canvas_pos[1] >= y_bot && canvas_pos[1] <= y_top
}

impl NodeEditor {
    /// Toggle the per-node collapse flag and invalidate the retained
    /// canvas backbuffer so the change is visible next frame.
    pub(super) fn toggle_collapsed(&mut self, id: NodeId) {
        if !self.collapsed_nodes.insert(id) {
            self.collapsed_nodes.remove(&id);
        }
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
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
    let moving_rect = Rect::new(raw_top_left[0], raw_top_left[1] - size[1], size[0], size[1]);
    let targets: Vec<(SnapId, Rect)> = layouts
        .iter()
        .filter(|l| l.node_id != moving_id)
        .map(|l| {
            (
                SnapId(l.node_id.0),
                Rect::new(
                    l.top_left[0],
                    l.top_left[1] - l.size[1],
                    l.size[0],
                    l.size[1],
                ),
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
