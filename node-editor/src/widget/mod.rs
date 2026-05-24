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
//! live in the [`events`] submodule, while paint-cache fingerprinting
//! and child rebuild logic live in [`paint_cache`], so this file stays
//! under the 800-line guardrail.

mod events;
mod host_hooks;
mod node_paint_context;
pub mod nodes;
mod popup;
mod snap_guides;
mod value_editor_widget;

use popup::{build_add_node_popup_items, translate_event_into};

use snap_guides::paint_snap_guides_canvas;
#[cfg(test)]
mod nodes_tests;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use agg_gui::widget::{paint_subtree, BackbufferKind, BackbufferSpec, BackbufferState};
use agg_gui::{
    DrawCtx, Event, EventResult, HAnchor, Insets, MenuResponse, PopupMenu, Rect, Size, VAnchor,
    Widget, WidgetBase,
};

use crate::draw::{
    draw_bezier_connection, draw_canvas_grid, layout_node_with_state, CanvasPalette,
    NodeLayoutInfo, PropLayout, SocketLayout, SocketSide,
};
use crate::model::{NodeGraphModel, NodeId, NoodleView, SocketTypeId};
use crate::widget::nodes::{NodePaintContext, NodeWidget};

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

/// Resolve a noodle's `(from, to)` endpoint sockets against the
/// per-node layouts that paint just produced.
///
/// Looks up each endpoint side-restricted: `from` is the source-side
/// socket of an output, `to` is the target-side socket of an input.
/// This matters when a node carries both an input and an output that
/// share a name — e.g. AtomArtist's unified `Output` node, whose
/// adopted input slot and mirror output socket both take the source
/// socket's name. Without the side filter, the name lookup hits the
/// output-first row order and the noodle's `to` endpoint snaps to the
/// wrong side of the node.
pub(crate) fn resolve_noodle_endpoints<'a>(
    layouts: &'a [NodeLayoutInfo],
    noodle: &NoodleView,
) -> Option<(&'a SocketLayout, &'a SocketLayout)> {
    let from = layouts
        .iter()
        .find(|l| l.node_id == noodle.from_node)
        .and_then(|l| {
            l.sockets()
                .find(|s| s.side == SocketSide::Output && s.name == noodle.from_socket)
        })?;
    let to = layouts
        .iter()
        .find(|l| l.node_id == noodle.to_node)
        .and_then(|l| {
            l.sockets()
                .find(|s| s.side == SocketSide::Input && s.name == noodle.to_socket)
        })?;
    Some((from, to))
}

/// The reusable node-editor widget.
pub struct NodeEditor {
    bounds: Rect,
    /// One `NodeWidget` per model node, rebuilt by `layout()` when the
    /// model snapshot's fingerprint changes.  Bounds live in
    /// **canvas-space**; `paint()` applies the pan/zoom transform and
    /// leaves it active for the framework's child-paint pass, popping
    /// it in `finish_paint()`.  Exposing children this way is what lets
    /// the inspector see every node.
    children: Vec<Box<dyn Widget>>,
    base: WidgetBase,
    model: SharedModel,
    canvas_offset: [f64; 2],
    canvas_scale: f64,
    selected: HashSet<NodeId>,
    /// Nodes the user has collapsed via the title-bar chevron or
    /// double-click. Collapsed nodes paint as title-bar-only and their
    /// sockets all anchor at the title-bar side-center so existing
    /// noodles still resolve to an endpoint.
    collapsed_nodes: HashSet<NodeId>,
    /// "Chevron clicked" channel shared with every node header. The
    /// header's [`agg_gui::widgets::ChevronWidget`] child writes the
    /// node's id here when the user clicks it; `layout()` drains the
    /// cell and toggles `collapsed_nodes`. Lets the chevron live as a
    /// real composed child widget instead of a manual hit-rect.
    pending_collapse: Rc<Cell<Option<NodeId>>>,
    /// Most recent left-click in widget-local coords + time, used to
    /// detect double-clicks on a node's title bar (toggle collapse).
    last_click: Option<(agg_gui::Point, web_time::Instant)>,
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
    /// Retained GL FBO state — `paint_subtree_gl_backbuffer` keys its
    /// texture cache off this struct's `id()` and skips re-rasterising
    /// while `dirty` is false.
    backbuffer: BackbufferState,
    /// Fingerprint of the data that drives the retained paint.  Updated
    /// by `layout()`; a change invalidates `backbuffer` and rebuilds
    /// `children`.
    last_paint_fingerprint: Option<u64>,
    /// Optional floating editor (today: the ColorWheelPicker dialog
    /// spawned when a row with `EditorHint::Color` is clicked).  Painted
    /// after the canvas in `finish_paint` so it sits above nodes and
    /// edges, and consumes events first in `on_event`.  Outside the
    /// children Vec because (a) the children Vec is rebuilt on every
    /// fingerprint change and (b) Window-style overlays already manage
    /// their own bounds and don't want pan/zoom baked in.
    ///
    /// Stays `None` when an [`Self::overlay_sink`] is installed —
    /// callers that supply a sink want the dialog hoisted to a
    /// screen-level host (so it can be dragged outside the editor
    /// pane), so the local fallback path is bypassed entirely.
    pub(crate) overlay: Option<Box<dyn Widget>>,
    /// Set to `true` by overlay callbacks (Select / Cancel / window
    /// close) to ask the editor to drop the overlay on the next event
    /// or layout pass.  Cleared when the overlay is taken down.
    pub(crate) overlay_close_flag: Option<Rc<Cell<bool>>>,
    /// Optional host-supplied sink for floating dialogs. When set,
    /// [`Self::open_color_picker`] hands the constructed
    /// `(dialog_widget, close_flag)` pair to this callback instead
    /// of storing the dialog in [`Self::overlay`].
    ///
    /// Use case: AtomArtist's app shell wants the color-picker dialog
    /// to live at the top of the widget tree (alongside the debug
    /// windows) so the user can drag it anywhere on screen — not just
    /// within the node-editor pane. Other hosts that don't supply a
    /// sink fall back to the in-editor overlay (the legacy default).
    pub(crate) overlay_sink: Option<Box<dyn FnMut(Box<dyn Widget>, Rc<Cell<bool>>)>>,
    /// Optional host hook fired when one or more files are dropped onto
    /// the canvas. Receives the dropped paths and the canvas-space
    /// position of the cursor at drop time — typically used to import
    /// an asset and spawn a node at that location.
    ///
    /// AtomArtist's app shell installs this to turn `.stl`/`.obj`/`.3mf`
    /// drops into `MeshNode`s. Hosts that don't care about file drops
    /// leave the field `None` and the event is simply ignored.
    pub(crate) file_drop_handler: Option<Box<dyn FnMut(&[std::path::PathBuf], [f64; 2])>>,
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
            collapsed_nodes: HashSet::new(),
            pending_collapse: Rc::new(Cell::new(None)),
            last_click: None,
            palette: CanvasPalette::dark(),
            interaction: CanvasState::Idle,
            space_held: false,
            id: "node-editor",
            popup: PopupMenu::new(popup_items),
            popup_canvas_pos: [0.0, 0.0],
            backbuffer: BackbufferState::new(),
            last_paint_fingerprint: None,
            overlay: None,
            overlay_close_flag: None,
            overlay_sink: None,
            file_drop_handler: None,
        }
    }

    /// Take the overlay down (if any) and clear its close-flag.  Called
    /// when a close was requested or when external code wants to force
    /// the floating editor closed.
    pub(crate) fn close_overlay(&mut self) {
        self.overlay = None;
        self.overlay_close_flag = None;
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// Check `overlay_close_flag` and tear the overlay down if it
    /// fired.  Returns `true` when an overlay was actually closed
    /// (callers can use this to claim a redraw).
    pub(crate) fn drain_overlay_close(&mut self) -> bool {
        let fired = self
            .overlay_close_flag
            .as_ref()
            .map(|f| f.replace(false))
            .unwrap_or(false);
        if fired {
            self.close_overlay();
        }
        fired
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
        let noodles = model.noodles();
        let ext_sel = model.primary_selection();
        drop(model);
        // Index of connected input sockets keyed by `(node_id, socket_name)`.
        let connected: std::collections::HashSet<(NodeId, String)> = noodles
            .iter()
            .map(|e| (e.to_node, e.to_socket.clone()))
            .collect();
        let mut layouts: Vec<NodeLayoutInfo> = nodes
            .iter()
            .map(|n| {
                let collapsed = self.collapsed_nodes.contains(&n.id);
                layout_node_with_state(
                    n,
                    |sock| connected.contains(&(n.id, sock.to_string())),
                    collapsed,
                )
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

    /// Hash of every input that affects how the children's paint looks
    /// across one frame.  Mismatch between the previous fingerprint and
    /// the new one drives both the children rebuild and the GL FBO
    /// invalidation — paint outputs change ⇒ the cached texture must
    /// regenerate.
    ///
    /// Pan/zoom IS part of the fingerprint: layout bakes them into the
    /// child widgets' screen-space bounds (so the inspector tree picks
    /// them up correctly via `collect_inspector_nodes`), which means a
    /// pan/zoom change demands a children rebuild.
    fn compute_fingerprint(&self, layouts: &[NodeLayoutInfo], ext_sel: Option<NodeId>) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        layouts.len().hash(&mut h);
        for l in layouts {
            l.node_id.0.hash(&mut h);
            l.top_left[0].to_bits().hash(&mut h);
            l.top_left[1].to_bits().hash(&mut h);
            l.size[0].to_bits().hash(&mut h);
            l.size[1].to_bits().hash(&mut h);
            l.display_name.hash(&mut h);
            l.category.hash(&mut h);
            l.rows.len().hash(&mut h);
            let sel = self.selected.contains(&l.node_id) || ext_sel == Some(l.node_id);
            sel.hash(&mut h);
            l.collapsed.hash(&mut h);
        }
        self.canvas_offset[0].to_bits().hash(&mut h);
        self.canvas_offset[1].to_bits().hash(&mut h);
        self.canvas_scale.to_bits().hash(&mut h);
        h.finish()
    }

    /// Tear down `self.children` and build a fresh `Vec<NodeWidget>`
    /// from `layouts`.  Bounds are in **screen-space** — the canvas
    /// pan/zoom is baked into each NodeWidget's position and size so
    /// the framework's per-child translate (which adds bounds in
    /// screen-space, not in pre-transform space) lands at the right
    /// pixels AND `collect_inspector_nodes` sees the on-screen rect.
    fn rebuild_children(&mut self, layouts: &[NodeLayoutInfo], ext_sel: Option<NodeId>) {
        let visuals = agg_gui::current_visuals();
        let palette = CanvasPalette::from_visuals(&visuals);
        let model = self.model.lock().unwrap();
        let node_ctx = NodePaintContext::from_model(palette, &*model);
        drop(model);

        let scale = self.canvas_scale;
        let offset = self.canvas_offset;
        let mut new_children: Vec<Box<dyn Widget>> = Vec::with_capacity(layouts.len());
        for l in layouts {
            let selected = self.selected.contains(&l.node_id) || ext_sel == Some(l.node_id);
            let nw = NodeWidget::from_layout_transformed(
                l,
                selected,
                node_ctx.clone(),
                scale,
                offset,
                Rc::clone(&self.pending_collapse),
            );
            new_children.push(Box::new(nw));
        }
        self.children = new_children;
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

        // Drain the chevron-click channel — header chevrons (real
        // child widgets) write a NodeId here when consumed; the editor
        // applies the toggle before laying out so the collapsed flag
        // is visible to this frame's `snapshot_layouts`.
        if let Some(id) = self.pending_collapse.take() {
            self.toggle_collapsed(id);
        }

        // Snapshot once for both the fingerprint AND the (possible)
        // children rebuild — avoids hitting the model twice.
        let layouts = self.snapshot_layouts();
        let ext_sel = self.model.lock().unwrap().primary_selection();
        let sig = self.compute_fingerprint(&layouts, ext_sel);

        if self.last_paint_fingerprint != Some(sig) {
            self.rebuild_children(&layouts, ext_sel);
            self.last_paint_fingerprint = Some(sig);
            self.backbuffer.invalidate();
        }

        // Floating overlay (e.g. ColorWheelPicker dialog).  Drain its
        // close flag first so a Select / Cancel that fired during the
        // last event pass takes the overlay down before we re-lay it.
        self.drain_overlay_close();
        if let Some(overlay) = self.overlay.as_mut() {
            let desired = overlay.layout(Size::new(available.width, available.height));
            let current = overlay.bounds();
            // If the overlay has its own bounds (e.g. a `Window` with
            // a saved position), respect them — only fall back to a
            // centred top-of-canvas placement when bounds are empty.
            if current.width <= 0.0 || current.height <= 0.0 {
                let w = desired.width.max(1.0);
                let h = desired.height.max(1.0);
                let x = ((available.width - w) * 0.5).max(0.0);
                let y = ((available.height - h) - 20.0).max(0.0);
                overlay.set_bounds(Rect::new(x, y, w, h));
            }
        }

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

        // Outer save: pinned by `finish_paint`.  Without it, nodes drawn
        // at canvas-y > self.bounds.height bleed into the sibling pane
        // above when a splitter shrinks the canvas.
        ctx.save();
        ctx.clip_rect(0.0, 0.0, w, h);

        ctx.set_fill_color(self.palette.canvas_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Grid + edges live in canvas-space.  Push the canvas
        // transform (`screen = canvas * scale + offset`) on `ctx` for
        // those paints, but pop it BEFORE returning so the framework's
        // child paint pass sees the editor's normal local space —
        // NodeWidget bounds are already in screen-space (pre-baked by
        // `layout`) so they don't want this transform composed on
        // top.  Order matters: scale first, then translate.
        ctx.save();
        ctx.scale(self.canvas_scale, self.canvas_scale);
        ctx.translate(self.canvas_offset[0], self.canvas_offset[1]);

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

        // Edges (under nodes).  Re-snapshot here rather than caching the
        // `layouts` from `layout()` so paint doesn't carry a hidden
        // dependency on layout-time data — the backbuffer caches the
        // result anyway, so paint runs once per real change.
        let layouts = self.snapshot_layouts();
        let model = self.model.lock().unwrap();
        let noodles = model.noodles();
        for noodle in &noodles {
            if let Some((f, t)) = resolve_noodle_endpoints(&layouts, noodle) {
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
        drop(model);

        // Snap-guide overlay — only paints while a node drag is in
        // progress.  Coordinates come from the thread-local snap
        // registry in canvas-space, matching the current ctx
        // transform, so guides land exactly on the moving node's
        // would-be edges and reference rects.
        if matches!(self.interaction, CanvasState::DraggingNode { .. }) {
            paint_snap_guides_canvas(ctx);
        }

        // Pop the canvas-space transform so the framework recurses into
        // child NodeWidgets in widget-local space — their bounds are
        // already in screen-space (pre-baked by layout()).
        ctx.restore();
    }

    fn finish_paint(&mut self, ctx: &mut dyn DrawCtx) {
        // Popup paints in widget-local space, on top of nodes & edges
        // but inside the canvas clip.
        if self.popup.is_open() {
            if let Some(font) = agg_gui::font_settings::current_system_font() {
                let viewport = Size::new(self.bounds.width, self.bounds.height);
                self.popup.paint(ctx, font, 13.0, viewport);
            }
        }

        // Floating overlay (Window-wrapped color picker, etc.).  Painted
        // last so it sits above nodes, edges, and the popup menu.
        if let Some(overlay) = self.overlay.as_mut() {
            let b = overlay.bounds();
            if b.width > 0.0 && b.height > 0.0 {
                ctx.save();
                ctx.translate(b.x, b.y);
                paint_subtree(overlay.as_mut(), ctx);
                ctx.restore();
            }
        }

        // Pop the outer clip save.
        ctx.restore();
    }

    fn backbuffer_spec(&mut self) -> BackbufferSpec {
        BackbufferSpec {
            kind: BackbufferKind::GlFbo,
            cached: true,
            alpha: 1.0,
            outsets: Insets::ZERO,
            rounded_clip: None,
        }
    }

    fn backbuffer_state_mut(&mut self) -> Option<&mut BackbufferState> {
        Some(&mut self.backbuffer)
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
        // Overlay (color-picker dialog) consumes events first while it's
        // up — it draws on top and needs to capture clicks before they
        // reach the canvas underneath.  After dispatching, drain the
        // close flag so a Select / Cancel / Window-close click takes
        // the overlay down on this same event.
        if let Some(overlay) = self.overlay.as_mut() {
            let b = overlay.bounds();
            let translated = translate_event_into(event, b.x, b.y);
            let result = overlay.on_event(&translated);
            // The picker's child Buttons may have set the close flag —
            // drain it whether or not THIS event was consumed.  We also
            // want to claim the redraw the close needs.
            let closed = self.drain_overlay_close();
            if result == EventResult::Consumed || closed {
                agg_gui::animation::request_draw();
                return EventResult::Consumed;
            }
        }
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
            Event::FileDropped { pos, paths } => {
                // Translate the drop position from widget-local to
                // canvas-space so the host can place a node at the
                // user's intended spot regardless of pan/zoom.
                let canvas_pos = self.local_to_canvas(*pos);
                if let Some(handler) = self.file_drop_handler.as_mut() {
                    handler(paths, canvas_pos);
                    agg_gui::animation::request_draw();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }
}
