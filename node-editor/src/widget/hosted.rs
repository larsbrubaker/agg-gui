//! Hosted cards — node bodies built by the host as real agg-gui widgets.
//!
//! By default the editor draws its own simplified cards ([`super::nodes`]).
//! A host that wants each card to carry its *own* widgets (MatterCAD's
//! NodeDesigner hosts the same property-editor rows as its properties
//! panel inside every card) installs a [`NodeBodyFactory`] with
//! [`NodeEditor::with_body_factory`]. For every node the factory answers
//! with a [`HostedNodeBody`], the editor then:
//!
//! - keeps the noodles, sockets, gestures, selection, menus and view;
//! - draws the card chrome (body, title bar, selection ring, badge) and
//!   the sockets itself ([`super::hosted_card`]);
//! - puts the body widget inside the card, in a canvas layer whose
//!   `child_transform` is the pan/zoom, so the body paints, hit-tests,
//!   takes focus and opens popups at the right place at any zoom;
//! - caches the body per [`NodeId`] and rebuilds it only when the model's
//!   [`NodeGraphModel::body_epoch`] changes (never on a paint
//!   fingerprint), so focus and an edit in progress survive;
//! - sizes the card to [`NodeGraphModel::node_width`] (the right edge can
//!   be dragged: [`NodeGraphModel::set_node_width`]) and to the body's
//!   measured height, reported through [`NodeGraphModel::on_node_measured`]
//!   so the host can keep the top edge fixed while the card grows down.
//!
//! Nodes the factory declines keep the simplified card, so a host can mix
//! both. Without a factory nothing here runs and the editor behaves as it
//! always has. Layout of the cards is in this file, the card and layer
//! widgets in `hosted_card.rs`, the resize/raise gestures in
//! `hosted_events.rs`.

use std::collections::HashMap;

use agg_gui::{Rect, Size, Widget};

use crate::draw::CanvasPalette;
use crate::draw::{
    NodeLayoutInfo, NodeRow, SocketLayout, SocketSide, NODE_BOTTOM_PAD, NODE_WIDTH, ROW_HEIGHT,
    TITLE_HEIGHT,
};
use crate::model::{NodeId, NodeView};

use super::hosted_card::{CardChrome, CardSocket, HostedCard, HostedLayer, LAYER_TYPE_NAME};
use super::NodeEditor;

/// Where a socket sits on a hosted card: the edge it is drawn on and its
/// distance in canvas units from the **top of the body** (the title bar
/// is added by the editor).
pub type SocketAnchor = (SocketSide, f64);

/// Resolves a socket's anchor from the laid-out body widget. The widget
/// is the one the factory returned, already laid out at the card width,
/// so a resolver can read its children's bounds (or downcast it through
/// `Widget::as_any`) to put a socket beside the row it belongs to.
pub type SocketAnchorFn = Box<dyn Fn(&dyn Widget, &str) -> Option<SocketAnchor>>;

/// A card body supplied by the host: the widget, and optionally where its
/// sockets go. Sockets without an anchor stack under the title bar,
/// outputs first, one [`ROW_HEIGHT`] apart.
pub struct HostedNodeBody {
    pub widget: Box<dyn Widget>,
    pub socket_anchor: Option<SocketAnchorFn>,
}

impl HostedNodeBody {
    /// A body whose sockets use the default stacking.
    pub fn new(widget: Box<dyn Widget>) -> Self {
        Self {
            widget,
            socket_anchor: None,
        }
    }

    /// Place sockets with `anchor` (see [`SocketAnchorFn`]).
    pub fn with_socket_anchor<F>(mut self, anchor: F) -> Self
    where
        F: Fn(&dyn Widget, &str) -> Option<SocketAnchor> + 'static,
    {
        self.socket_anchor = Some(Box::new(anchor));
        self
    }
}

/// Builds hosted card bodies. Called with no model lock held, so a factory
/// may lock the model (or anything else) itself. Returning `None` keeps
/// the editor's simplified card for that node.
pub trait NodeBodyFactory {
    fn build_body(&mut self, node: &NodeView) -> Option<HostedNodeBody>;
}

impl<F> NodeBodyFactory for F
where
    F: FnMut(&NodeView) -> Option<HostedNodeBody>,
{
    fn build_body(&mut self, node: &NodeView) -> Option<HostedNodeBody> {
        self(node)
    }
}

/// What one socket of a hosted card looks like, read from the model.
struct SocketLook {
    color: agg_gui::Color,
    shape: crate::socket_style::SocketShape,
    landed: usize,
}

/// Canvas-space geometry of one hosted card, captured by `layout()` and
/// read by `snapshot_layouts()` for hit-testing, noodles and snapping.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CardGeom {
    pub width: f64,
    pub height: f64,
    /// `(socket name, edge, distance from the card's top)`.
    pub sockets: Vec<(String, SocketSide, f64)>,
}

/// A right-edge resize in progress.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResizeDrag {
    pub node: NodeId,
    pub start_width: f64,
    pub start_canvas_x: f64,
}

/// Everything the hosted-card mode keeps on the editor.
pub(crate) struct HostedState {
    pub factory: Option<Box<dyn NodeBodyFactory>>,
    /// Body epoch the cached bodies were built at.
    pub epoch: Option<u64>,
    pub cards: HashMap<NodeId, CardGeom>,
    /// Hosted cards bottom → top: the last-clicked card is painted last
    /// (model order when `raise_on_click` is off).
    pub order: Vec<NodeId>,
    /// A press on a card raises it. Off keeps the cards in model order,
    /// as MatterCAD's NodeDesigner does.
    pub raise_on_click: bool,
    pub resize: Option<ResizeDrag>,
    /// Card heights last reported through `on_node_measured`.
    pub measured: HashMap<NodeId, f64>,
    /// Double-click / chevron collapse of simplified cards.
    pub collapse_enabled: bool,
    /// Snap guides while dragging a node.
    pub snap_guides: bool,
    /// `animation::invalidation_epoch` seen by the last layout, so a
    /// hosted widget that asked for a redraw re-rasters the canvas.
    pub seen_invalidation: u64,
}

impl Default for HostedState {
    fn default() -> Self {
        Self {
            factory: None,
            epoch: None,
            cards: HashMap::new(),
            order: Vec::new(),
            raise_on_click: true,
            resize: None,
            measured: HashMap::new(),
            collapse_enabled: true,
            snap_guides: true,
            seen_invalidation: 0,
        }
    }
}

/// Smallest width a right-edge drag can give a card.
pub const MIN_HOSTED_CARD_WIDTH: f64 = 80.0;

impl NodeEditor {
    /// Install a [`NodeBodyFactory`] — turns on hosted cards (see the
    /// module docs). Without one the editor draws its simplified cards.
    pub fn with_body_factory<F: NodeBodyFactory + 'static>(mut self, factory: F) -> Self {
        self.hosted.factory = Some(Box::new(factory));
        self
    }

    /// Turn the collapse toggle (title double-click, header chevron) of
    /// simplified cards on or off. On by default.
    pub fn with_collapse_enabled(mut self, enabled: bool) -> Self {
        self.hosted.collapse_enabled = enabled;
        self
    }

    /// Turn the snap guides of a node drag on or off. On by default (and
    /// still subject to agg-gui's global `snap::is_enabled`).
    pub fn with_snap_guides(mut self, enabled: bool) -> Self {
        self.hosted.snap_guides = enabled;
        self
    }

    /// Whether a press on a hosted card raises it to the top of the paint
    /// order. On by default. Off keeps the cards in the order the model
    /// lists its nodes (MatterCAD's NodeDesigner keeps its node windows in
    /// the order they were added): a press still selects the card, and
    /// where two cards overlap the one later in model order is on top and
    /// takes the press.
    pub fn with_raise_on_click(mut self, raise: bool) -> Self {
        self.hosted.raise_on_click = raise;
        self
    }

    /// The hosted cards' node ids, bottom → top.
    pub fn hosted_card_order(&self) -> &[NodeId] {
        &self.hosted.order
    }

    /// Bring a hosted card to the top of the paint order. Does nothing when
    /// [`Self::with_raise_on_click`] is off: the cards stay in model order.
    pub fn raise_card(&mut self, id: NodeId) {
        if !self.hosted.raise_on_click {
            return;
        }
        if self.hosted.order.last() == Some(&id) || !self.hosted.order.contains(&id) {
            return;
        }
        self.hosted.order.retain(|n| *n != id);
        self.hosted.order.push(id);
        self.backbuffer.invalidate();
        agg_gui::animation::request_draw();
    }

    /// Remove the hosted layer from the end of `children` (it is put back
    /// by `rebuild_children`, so the cached bodies survive a rebuild).
    pub(super) fn take_hosted_layer(&mut self) -> Option<Box<dyn Widget>> {
        if self.children.last().map(|c| c.type_name()) == Some(LAYER_TYPE_NAME) {
            self.children.pop()
        } else {
            None
        }
    }

    /// Build, cache and lay out the hosted card bodies. Runs at the start
    /// of `layout()`, before the snapshot that reads [`CardGeom`].
    pub(super) fn layout_hosted(&mut self, available: Size) {
        if self.hosted.factory.is_none() {
            return;
        }
        let model = self.model.lock().unwrap();
        let nodes = model.nodes();
        let epoch = model.body_epoch();
        let ext_sel = model.primary_selection();
        let palette = CanvasPalette::from_visuals(&agg_gui::current_visuals());
        let noodles = model.noodles();
        let looks: Vec<(f64, agg_gui::Color, Vec<SocketLook>)> = nodes
            .iter()
            .map(|n| {
                let width = model.node_width(n.id).unwrap_or(NODE_WIDTH);
                let title = model.category_color(&n.category, palette.node_title_fallback);
                // Same order as `HostedCard::layout_card`'s sockets.
                let sockets = n
                    .outputs
                    .iter()
                    .map(|s| (s, SocketSide::Output))
                    .chain(n.inputs.iter().map(|s| (s, SocketSide::Input)))
                    .map(|(s, side)| SocketLook {
                        color: model.socket_color(s.socket_type),
                        shape: model.socket_shape(n.id, side, &s.name, s.socket_type),
                        landed: match side {
                            SocketSide::Input => {
                                super::presentation::landed_on(&*model, &noodles, n.id, &s.name)
                            }
                            SocketSide::Output => 0,
                        },
                    })
                    .collect();
                (width, title, sockets)
            })
            .collect();
        drop(model);

        if self.children.last().map(|c| c.type_name()) != Some(LAYER_TYPE_NAME) {
            self.children.push(Box::new(HostedLayer::new()));
        }
        let rebuild_all = self.hosted.epoch != Some(epoch);
        self.hosted.epoch = Some(epoch);
        let mut factory = self.hosted.factory.take();
        let layer = layer_in(&mut self.children).expect("hosted layer was just ensured");
        layer.set_view(available, self.canvas_offset, self.canvas_scale);
        if rebuild_all {
            layer.clear();
        }
        layer.retain_nodes(&nodes);
        let mut built = Vec::new();
        for n in &nodes {
            if layer.card(n.id).is_none() {
                if let Some(body) = factory.as_mut().and_then(|f| f.build_body(n)) {
                    built.push(n.id);
                    layer.push_card(HostedCard::new(n.id, body));
                }
            }
        }
        self.hosted.factory = factory;

        let mut geoms = HashMap::new();
        let layer = layer_in(&mut self.children).expect("hosted layer exists");
        for (n, (width, title_color, colors)) in nodes.iter().zip(looks) {
            let Some(card) = layer.card_mut(n.id) else {
                continue;
            };
            let geom = card.layout_card(n, width.max(MIN_HOSTED_CARD_WIDTH));
            let selected = self.selected.contains(&n.id) || ext_sel == Some(n.id);
            let sockets = geom
                .sockets
                .iter()
                .zip(colors)
                .map(|((_, side, y), look)| CardSocket {
                    side: *side,
                    from_top: *y,
                    color: look.color,
                    shape: look.shape,
                    stretch: crate::socket_style::multi_input_stretch(look.landed),
                })
                .collect();
            card.chrome = CardChrome {
                title: n.display_name.clone(),
                title_color,
                body: palette.node_body,
                border: palette.node_border,
                label: palette.label_text,
                selected,
                badge: n.badge().map(|(s, _)| (s, palette.badge_color(s))),
                sockets,
                style: self.presentation.noodle_style,
            };
            card.set_bounds(Rect::new(
                n.position[0],
                n.position[1] - geom.height,
                geom.width,
                geom.height,
            ));
            geoms.insert(n.id, geom);
        }

        // Z-order: keep the existing order, append new cards on top. Without
        // raise-on-click the order is the model's, rebuilt every layout so
        // it follows the model if it reorders its nodes.
        if !self.hosted.raise_on_click {
            self.hosted.order.clear();
        }
        self.hosted.order.retain(|id| geoms.contains_key(id));
        for n in &nodes {
            if geoms.contains_key(&n.id) && !self.hosted.order.contains(&n.id) {
                self.hosted.order.push(n.id);
            }
        }
        let order = self.hosted.order.clone();
        let layer = layer_in(&mut self.children).expect("hosted layer exists");
        layer.sort_by_order(&order);

        let geometry_changed = geoms != self.hosted.cards || !built.is_empty() || rebuild_all;
        self.hosted.cards = geoms;
        self.report_measured_heights();
        let inval = agg_gui::animation::invalidation_epoch();
        if geometry_changed || inval != self.hosted.seen_invalidation {
            self.backbuffer.invalidate();
        }
        self.hosted.seen_invalidation = inval;
    }

    /// Tell the host every card height that changed since it last heard.
    fn report_measured_heights(&mut self) {
        let mut changed: Vec<(NodeId, f64)> = self
            .hosted
            .cards
            .iter()
            .filter(|(id, g)| self.hosted.measured.get(id) != Some(&g.height))
            .map(|(id, g)| (*id, g.height))
            .collect();
        self.hosted
            .measured
            .retain(|id, _| self.hosted.cards.contains_key(id));
        if changed.is_empty() {
            return;
        }
        changed.sort_by_key(|(id, _)| id.0);
        let mut model = self.model.lock().unwrap();
        for (id, h) in changed {
            self.hosted.measured.insert(id, h);
            model.on_node_measured(id, h);
        }
    }

    /// Replace the simplified layout of every hosted node with its card's
    /// geometry, and order the layouts as they are painted: simplified
    /// cards first (selected on top, as before), then the hosted cards
    /// bottom → top — so hit-tests walking the list backwards find the
    /// card the user sees.
    pub(super) fn order_layouts(
        &self,
        nodes: &[NodeView],
        layouts: &mut [NodeLayoutInfo],
        ext_sel: Option<NodeId>,
    ) {
        for (l, n) in layouts.iter_mut().zip(nodes) {
            if let Some(g) = self.hosted.cards.get(&n.id) {
                *l = hosted_layout(n, g);
            }
        }
        let rank = |id: NodeId| self.hosted.order.iter().position(|n| *n == id);
        layouts.sort_by_key(|l| match rank(l.node_id) {
            Some(r) => (1u8, 0u8, r as u64),
            None => {
                let local = self.selected.contains(&l.node_id) as u8;
                let external = (ext_sel == Some(l.node_id)) as u8;
                (0u8, local | external, l.node_id.0)
            }
        });
    }
}

/// The hit-test / noodle layout of a hosted card: its rect and its
/// sockets, no rows of its own (the body widget owns those).
fn hosted_layout(n: &NodeView, g: &CardGeom) -> NodeLayoutInfo {
    let [x, top] = n.position;
    let mut rows = Vec::with_capacity(g.sockets.len());
    for (name, edge, from_top) in &g.sockets {
        let center = match edge {
            SocketSide::Input => [x, top - from_top],
            SocketSide::Output => [x + g.width, top - from_top],
        };
        if let Some(s) = n.outputs.iter().find(|s| &s.name == name) {
            rows.push(NodeRow::Output(SocketLayout {
                side: SocketSide::Output,
                name: s.name.clone(),
                display_label: s.label().to_string(),
                socket_type: s.socket_type,
                center,
                shape: Default::default(),
                landed: 0,
            }));
        } else if let Some(s) = n.inputs.iter().find(|s| &s.name == name) {
            rows.push(NodeRow::Input {
                socket: SocketLayout {
                    side: SocketSide::Input,
                    name: s.name.clone(),
                    display_label: s.label().to_string(),
                    socket_type: s.socket_type,
                    center,
                    shape: Default::default(),
                    landed: 0,
                },
                editor: None,
                height: ROW_HEIGHT,
            });
        }
    }
    NodeLayoutInfo {
        node_id: n.id,
        top_left: n.position,
        size: [g.width, g.height],
        rows,
        display_name: n.display_name.clone(),
        category: n.category.clone(),
        collapsed: false,
        error: n.error.clone(),
        warning: n.warning.clone(),
    }
}

/// The hosted layer at the end of the editor's children, if any.
fn layer_in(children: &mut [Box<dyn Widget>]) -> Option<&mut HostedLayer> {
    children
        .last_mut()?
        .as_any_mut()?
        .downcast_mut::<HostedLayer>()
}

/// Height of a card whose body measured `body_h`.
pub(crate) fn card_height(body_h: f64) -> f64 {
    TITLE_HEIGHT + body_h + NODE_BOTTOM_PAD
}
