//! `NodeEditor` — the agg-gui widget that drives a [`NodeGraphModel`].
//!
//! Pan / zoom state lives directly on the widget (`canvas_offset`,
//! `canvas_scale`). All hit-testing converts mouse positions from
//! widget-local coords (Y-up, origin at bottom-left of the widget) to
//! canvas-space using the inverse of the same transform applied during
//! paint.
//!
//! Interaction is a small state machine — see `CanvasState`. Drawing is
//! delegated to [`crate::draw`]. Event handlers (mouse / wheel / key)
//! live in the [`events`] submodule so this file stays under the
//! 800-line guardrail.

mod events;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use agg_gui::{
    DrawCtx, Event, EventResult, HAnchor, MenuEntry, MenuItem, MenuResponse, PopupMenu, Rect, Size,
    VAnchor, Widget, WidgetBase,
};

use crate::draw::{
    draw_bezier_connection, draw_canvas_grid, draw_node, layout_node_with_connections,
    CanvasPalette, NodeLayoutInfo, PropLayout, SocketLayout, SocketSide,
};
use crate::model::{NodeGraphModel, NodeId, SocketTypeId};

const ZOOM_MIN: f64 = 0.15;
const ZOOM_MAX: f64 = 3.0;
const ZOOM_STEP: f64 = 1.1;

/// Shared handle to a host's `NodeGraphModel` implementation. The
/// widget locks for the duration of one event or one paint, then
/// drops — keeping locks short.
///
/// Intentionally **not** `+ Send`: agg-gui's event loop is single-
/// threaded so the trait object never crosses thread boundaries via
/// the widget itself.  Hosts that *do* run a background graph
/// evaluator (e.g. AtomArtist) typically wrap their `Send` graph data
/// in a separate `Arc<Mutex<...>>` *inside* their model; the model
/// trait object only flows through the UI thread.
pub type SharedModel = Arc<Mutex<dyn NodeGraphModel>>;

/// Interaction state machine. Only one drag at a time.
#[derive(Clone, Debug)]
enum CanvasState {
    Idle,
    PanningCanvas {
        start_offset: [f64; 2],
        start_local: agg_gui::Point,
    },
    DraggingNode {
        ids: Vec<NodeId>,
        /// Per-node start position, captured at mousedown.
        start_positions: Vec<[f64; 2]>,
        start_canvas: [f64; 2],
    },
    DrawingConnection {
        from_node: NodeId,
        from_socket: String,
        from_canvas: [f64; 2],
        cursor_canvas: [f64; 2],
        from_socket_type: SocketTypeId,
        from_side: SocketSide,
    },
    /// Click-and-horizontal-drag editing of a numeric property.
    DraggingProperty {
        node_id: NodeId,
        prop_name: String,
        start_value: f64,
        start_local_x: f64,
        min: Option<f64>,
        max: Option<f64>,
    },
}

/// The reusable node-editor widget.
pub struct NodeEditor {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // unused — kept for the Widget trait
    base: WidgetBase,
    model: SharedModel,
    canvas_offset: [f64; 2],
    canvas_scale: f64,
    selected: HashSet<NodeId>,
    palette: CanvasPalette,
    interaction: CanvasState,
    /// Spacebar pan modifier — when held, mouse-left drag pans the canvas
    /// instead of selecting / dragging nodes.
    space_held: bool,
    /// Stable widget id for `find_widget_by_id` lookups.
    id: &'static str,
    /// Right-click add-node popup menu, plus the canvas-space position
    /// where the user clicked (used as the new node's position).
    popup: PopupMenu,
    popup_canvas_pos: [f64; 2],
}

impl NodeEditor {
    /// Construct a new editor over `model`. The default widget id is
    /// `"node-editor"` — change it with [`Self::with_id`] when hosting
    /// multiple editors in one tree.
    pub fn new(model: SharedModel) -> Self {
        let popup_items = build_add_node_popup_items(&model);
        Self {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
            base: WidgetBase::new()
                .with_h_anchor(HAnchor::STRETCH)
                .with_v_anchor(VAnchor::STRETCH),
            model,
            canvas_offset: [0.0, 0.0],
            canvas_scale: 1.0,
            selected: HashSet::new(),
            palette: CanvasPalette::dark(),
            interaction: CanvasState::Idle,
            space_held: false,
            id: "node-editor",
            popup: PopupMenu::new(popup_items),
            popup_canvas_pos: [0.0, 0.0],
        }
    }

    /// Override the widget id. Useful when multiple editors live in
    /// the same tree (e.g. a main editor + a subgraph editor in a
    /// modal panel).
    pub fn with_id(mut self, id: &'static str) -> Self {
        self.id = id;
        self
    }

    /// Override the palette (theme colours). Default is rebuilt every
    /// paint from `ctx.visuals()` so light/dark mode toggles flow
    /// through automatically — call this only if you want a custom
    /// look.
    pub fn set_palette(&mut self, palette: CanvasPalette) {
        self.palette = palette;
    }

    pub fn pan(&self) -> [f64; 2] {
        self.canvas_offset
    }

    pub fn scale(&self) -> f64 {
        self.canvas_scale
    }

    pub fn selected_ids(&self) -> &HashSet<NodeId> {
        &self.selected
    }

    fn local_to_canvas(&self, p: agg_gui::Point) -> [f64; 2] {
        [
            (p.x - self.canvas_offset[0]) / self.canvas_scale,
            (p.y - self.canvas_offset[1]) / self.canvas_scale,
        ]
    }

    /// Compute layouts for every node currently in the model. Layouts
    /// are returned in a deterministic order (selected last so they
    /// paint on top). Bound-input editors are suppressed for sockets
    /// that already have an incoming edge — that's the "data flowing
    /// in" rule from NodeDesigner's row layout.
    fn snapshot_layouts(&self) -> Vec<NodeLayoutInfo> {
        let model = self.model.lock().unwrap();
        let nodes = model.nodes();
        let edges = model.edges();
        let ext_sel = model.primary_selection();
        drop(model);
        // Index of connected input sockets keyed by `(node_id, socket_name)`.
        let connected: std::collections::HashSet<(NodeId, String)> = edges
            .iter()
            .map(|e| (e.to_node, e.to_socket.clone()))
            .collect();
        let mut layouts: Vec<NodeLayoutInfo> = nodes
            .iter()
            .map(|n| {
                layout_node_with_connections(n, |sock| {
                    connected.contains(&(n.id, sock.to_string()))
                })
            })
            .collect();
        layouts.sort_by_key(|l| {
            let local = self.selected.contains(&l.node_id) as u8;
            let external = (ext_sel == Some(l.node_id)) as u8;
            (local | external, l.node_id.0)
        });
        layouts
    }

    fn hit_node(&self, layouts: &[NodeLayoutInfo], canvas_pos: [f64; 2]) -> Option<NodeId> {
        for l in layouts.iter().rev() {
            if l.body_contains(canvas_pos) {
                return Some(l.node_id);
            }
        }
        None
    }

    fn hit_socket(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
    ) -> Option<(NodeId, SocketLayout)> {
        for l in layouts.iter().rev() {
            if let Some(s) = l.socket_at(canvas_pos) {
                return Some((l.node_id, s.clone()));
            }
        }
        None
    }

    fn hit_property(
        &self,
        layouts: &[NodeLayoutInfo],
        canvas_pos: [f64; 2],
    ) -> Option<(NodeId, PropLayout)> {
        for l in layouts.iter().rev() {
            if let Some(p) = l.prop_at(canvas_pos) {
                return Some((l.node_id, p.clone()));
            }
        }
        None
    }

    /// Action callback for the right-click popup — handles
    /// `"add.{type_id}"` entries by routing through the model.
    fn handle_popup_action(&mut self, action: &str) {
        if let Some(type_id) = action.strip_prefix("add.") {
            let pos = self.popup_canvas_pos;
            let mut model = self.model.lock().unwrap();
            let _ = model.add_node(type_id, pos);
        }
    }

    fn begin_drag_node(&mut self, id: NodeId, canvas_start: [f64; 2]) {
        let mut drag_ids: Vec<NodeId> = self.selected.iter().copied().collect();
        if !drag_ids.contains(&id) {
            drag_ids.clear();
            drag_ids.push(id);
            self.selected.clear();
            self.selected.insert(id);
        }
        let model = self.model.lock().unwrap();
        let nodes = model.nodes();
        drop(model);
        let mut start_positions = Vec::with_capacity(drag_ids.len());
        for &nid in &drag_ids {
            let pos = nodes
                .iter()
                .find(|n| n.id == nid)
                .map(|n| n.position)
                .unwrap_or([0.0, 0.0]);
            start_positions.push(pos);
        }
        self.interaction = CanvasState::DraggingNode {
            ids: drag_ids,
            start_positions,
            start_canvas: canvas_start,
        };
    }
}

impl Widget for NodeEditor {
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

    fn type_name(&self) -> &'static str {
        "NodeEditor"
    }

    fn id(&self) -> Option<&str> {
        Some(self.id)
    }

    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        // Refresh palette per frame — theme switches flow through.
        let visuals = ctx.visuals();
        self.palette = CanvasPalette::from_visuals(&visuals);

        if let Some(f) = agg_gui::font_settings::current_system_font() {
            ctx.set_font(f);
        }

        // Outer save: clip + transform restore back to whatever the
        // parent gave us. Without this, nodes drawn at canvas-y >
        // self.bounds.height bleed into the sibling pane above when a
        // splitter shrinks the canvas.
        ctx.save();
        ctx.clip_rect(0.0, 0.0, w, h);

        ctx.set_fill_color(self.palette.canvas_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        ctx.save();
        ctx.translate(self.canvas_offset[0], self.canvas_offset[1]);
        ctx.scale(self.canvas_scale, self.canvas_scale);

        let inv_scale = 1.0 / self.canvas_scale;
        let visible_min = [
            (0.0 - self.canvas_offset[0]) * inv_scale,
            (0.0 - self.canvas_offset[1]) * inv_scale,
        ];
        let visible_max = [
            (w - self.canvas_offset[0]) * inv_scale,
            (h - self.canvas_offset[1]) * inv_scale,
        ];

        draw_canvas_grid(
            ctx,
            (visible_min, visible_max),
            40.0,
            self.palette.canvas_grid,
        );

        let layouts = self.snapshot_layouts();

        // Edges first (under nodes).
        let model = self.model.lock().unwrap();
        let edges = model.edges();
        for edge in &edges {
            let from = layouts
                .iter()
                .find(|l| l.node_id == edge.from_node)
                .and_then(|l| l.sockets().find(|s| s.name == edge.from_socket));
            let to = layouts
                .iter()
                .find(|l| l.node_id == edge.to_node)
                .and_then(|l| l.sockets().find(|s| s.name == edge.to_socket));
            if let (Some(f), Some(t)) = (from, to) {
                let col = model.socket_color(f.socket_type);
                draw_bezier_connection(ctx, f.center, t.center, col, 2.0);
            }
        }

        // Live in-progress connection.
        if let CanvasState::DrawingConnection {
            from_canvas,
            cursor_canvas,
            from_socket_type,
            ..
        } = &self.interaction
        {
            let mut col = model.socket_color(*from_socket_type);
            col.a *= 0.85;
            draw_bezier_connection(ctx, *from_canvas, *cursor_canvas, col, 2.0);
        }

        let ext_sel = model.primary_selection();
        for l in &layouts {
            let selected = self.selected.contains(&l.node_id) || ext_sel == Some(l.node_id);
            draw_node(ctx, l, selected, &self.palette, &*model);
        }
        drop(model);

        ctx.restore(); // pop pan/zoom transform

        // Right-click popup paints last so it sits above nodes & edges.
        if self.popup.is_open() {
            if let Some(font) = agg_gui::font_settings::current_system_font() {
                let viewport = Size::new(self.bounds.width, self.bounds.height);
                self.popup.paint(ctx, font, 13.0, viewport);
            }
        }

        ctx.restore(); // pop clip rect
    }

    fn hit_test(&self, local_pos: agg_gui::Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }

    fn claims_pointer_exclusively(&self, _local_pos: agg_gui::Point) -> bool {
        !matches!(self.interaction, CanvasState::Idle)
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        if self.popup.is_open() {
            let viewport = Size::new(self.bounds.width, self.bounds.height);
            let (result, response) = self.popup.handle_event(event, viewport);
            if let MenuResponse::Action(action) = response {
                self.handle_popup_action(&action);
                self.popup.close();
            } else if let MenuResponse::Closed = response {
                self.popup.close();
            }
            if result == EventResult::Consumed {
                return EventResult::Consumed;
            }
        }
        match event {
            Event::MouseDown {
                pos,
                button,
                modifiers,
            } => self.on_mouse_down(*pos, *button, *modifiers),
            Event::MouseUp {
                pos,
                button,
                modifiers,
            } => self.on_mouse_up(*pos, *button, *modifiers),
            Event::MouseMove { pos } => self.on_mouse_move(*pos),
            Event::MouseWheel {
                pos,
                delta_y,
                modifiers,
                ..
            } => self.on_wheel(*pos, *delta_y, *modifiers),
            Event::KeyDown { key, modifiers } => self.on_key_down(key, *modifiers),
            Event::KeyUp { key, modifiers } => self.on_key_up(key, *modifiers),
            _ => EventResult::Ignored,
        }
    }
}

/// Build the right-click "Add Node" menu — category-grouped submenus
/// containing every type the model exposes.  Action ids are
/// `"add.{type_id}"`.
fn build_add_node_popup_items(model: &SharedModel) -> Vec<MenuEntry> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EdgeResult, EdgeView, NodeTypeView, NodeView, PropertyValue};
    use agg_gui::Point;

    /// Trivial in-memory model for unit tests.
    #[derive(Default)]
    struct Memory {
        nodes: Vec<NodeView>,
        edges: Vec<EdgeView>,
        zoom: f64,
        last_selection: Option<NodeId>,
    }

    impl NodeGraphModel for Memory {
        fn nodes(&self) -> Vec<NodeView> {
            self.nodes.clone()
        }
        fn edges(&self) -> Vec<EdgeView> {
            self.edges.clone()
        }
        fn node_types_by_category(&self) -> Vec<(String, Vec<NodeTypeView>)> {
            vec![]
        }
        fn set_node_position(&mut self, id: NodeId, pos: [f64; 2]) {
            if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
                n.position = pos;
            }
        }
        fn add_node(&mut self, _type_id: &str, _pos: [f64; 2]) -> Option<NodeId> {
            None
        }
        fn remove_node(&mut self, id: NodeId) {
            self.nodes.retain(|n| n.id != id);
        }
        fn try_add_edge(
            &mut self,
            from_node: NodeId,
            from_socket: &str,
            to_node: NodeId,
            to_socket: &str,
        ) -> EdgeResult {
            self.edges.push(EdgeView {
                from_node,
                from_socket: from_socket.into(),
                to_node,
                to_socket: to_socket.into(),
            });
            EdgeResult::Connected
        }
        fn set_property(&mut self, _id: NodeId, _name: &str, _value: PropertyValue) {}
        fn on_canvas_zoom_changed(&mut self, zoom: f64) {
            self.zoom = zoom;
        }
        fn on_primary_selection_changed(&mut self, id: Option<NodeId>) {
            self.last_selection = id;
        }
    }

    fn fixture() -> SharedModel {
        Arc::new(Mutex::new(Memory::default()))
    }

    #[test]
    fn local_to_canvas_round_trip_with_pan_and_zoom() {
        let editor = {
            let mut e = NodeEditor::new(fixture());
            e.canvas_offset = [50.0, 30.0];
            e.canvas_scale = 1.5;
            e
        };
        let lp = Point::new(80.0, 60.0);
        let cp = editor.local_to_canvas(lp);
        assert!((cp[0] - (80.0 - 50.0) / 1.5).abs() < 1e-9);
        assert!((cp[1] - (60.0 - 30.0) / 1.5).abs() < 1e-9);
    }

    #[test]
    fn editor_has_default_id() {
        let e = NodeEditor::new(fixture());
        assert_eq!(e.id(), Some("node-editor"));
    }

    #[test]
    fn with_id_overrides_default() {
        let e = NodeEditor::new(fixture()).with_id("custom-canvas");
        assert_eq!(e.id(), Some("custom-canvas"));
    }
}
