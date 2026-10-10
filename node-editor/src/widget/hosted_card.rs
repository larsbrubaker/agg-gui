//! The widgets of hosted-card mode (see [`super::hosted`]).
//!
//! ```text
//! NodeEditor
//! ├── NodeWidget*            — simplified cards (nodes the factory declined)
//! └── HostedLayer            — editor-sized, child_transform = pan/zoom
//!     └── HostedCard*        — canvas-space bounds, chrome + sockets
//!         └── host body      — the factory's widget, under the title bar
//! ```
//!
//! The layer is the last child of the editor so hosted cards paint over
//! the simplified ones, and the cards inside it are kept in the editor's
//! z-order (last-clicked on top). Each card paints its own sockets, so a
//! card on top hides the sockets of a card beneath it. Neither the layer
//! nor the card consumes events: anything the body ignores bubbles to the
//! editor, which owns selection, node drags, noodles and menus. A press the
//! body does take still selects and raises its card, through the editor's
//! `Widget::preview_event` (`hosted_events.rs`).

use agg_gui::widgets::window::{
    paint_chrome_body, paint_chrome_border, paint_chrome_shadow, paint_chrome_title_bar,
    ChromeStyle,
};
use agg_gui::{Color, DrawCtx, Event, EventResult, Point, Rect, Size, TransAffine, Widget};

use crate::draw::{SocketSide, NODE_BOTTOM_PAD, NODE_RADIUS, ROW_HEIGHT, TITLE_HEIGHT};
use crate::model::{BadgeSeverity, NodeId, NodeView};

use super::hosted::{card_height, CardGeom, HostedNodeBody, SocketAnchorFn};

/// `type_name` of the layer — how the editor finds it among its children.
pub(crate) const LAYER_TYPE_NAME: &str = "NodeEditorHostedLayer";
const TITLE_FONT_SIZE: f64 = 13.0;
/// Width (canvas units) of the band along a card's right edge that
/// resizes the card.
pub(crate) const RESIZE_GRIP: f64 = 6.0;

/// The canvas layer: editor-sized, applying the pan/zoom to every card.
pub(crate) struct HostedLayer {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    offset: [f64; 2],
    scale: f64,
}

impl HostedLayer {
    pub fn new() -> Self {
        Self {
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: Vec::new(),
            offset: [0.0, 0.0],
            scale: 1.0,
        }
    }

    /// Match the editor's size and its current pan/zoom.
    pub fn set_view(&mut self, size: Size, offset: [f64; 2], scale: f64) {
        self.bounds = Rect::new(0.0, 0.0, size.width, size.height);
        self.offset = offset;
        self.scale = scale;
    }

    pub fn clear(&mut self) {
        self.children.clear();
    }

    /// Drop the cards of nodes that are gone.
    pub fn retain_nodes(&mut self, nodes: &[NodeView]) {
        self.children.retain(|c| {
            card_of(c.as_ref()).is_some_and(|k| nodes.iter().any(|n| n.id == k.node_id))
        });
    }

    pub fn push_card(&mut self, card: HostedCard) {
        self.children.push(Box::new(card));
    }

    pub fn card(&self, id: NodeId) -> Option<&HostedCard> {
        self.children
            .iter()
            .filter_map(|c| card_of(c.as_ref()))
            .find(|k| k.node_id == id)
    }

    pub fn card_mut(&mut self, id: NodeId) -> Option<&mut HostedCard> {
        self.children
            .iter_mut()
            .filter_map(|c| c.as_any_mut()?.downcast_mut::<HostedCard>())
            .find(|k| k.node_id == id)
    }

    /// Order the cards bottom → top as `order` lists them.
    pub fn sort_by_order(&mut self, order: &[NodeId]) {
        self.children.sort_by_key(|c| {
            card_of(c.as_ref())
                .and_then(|k| order.iter().position(|n| *n == k.node_id))
                .unwrap_or(usize::MAX)
        });
    }
}

fn card_of(w: &dyn Widget) -> Option<&HostedCard> {
    w.as_any()?.downcast_ref::<HostedCard>()
}

impl Widget for HostedLayer {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        LAYER_TYPE_NAME
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    /// `screen = canvas * scale + offset`, the transform the editor paints
    /// its grid and noodles with.
    fn child_transform(&self) -> Option<TransAffine> {
        let mut m = TransAffine::new_scaling_uniform(self.scale);
        m.translate(self.offset[0], self.offset[1]);
        Some(m)
    }
    /// The editor lays the cards out; keep the size it gave.
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    /// Only a point over a card belongs to the layer, so empty canvas
    /// reaches the editor directly.
    fn hit_test(&self, local_pos: Point) -> bool {
        let (mut x, mut y) = (local_pos.x, local_pos.y);
        if let Some(t) = self.child_transform() {
            t.inverse_transform(&mut x, &mut y);
        }
        self.children.iter().any(|c| {
            let b = c.bounds();
            x >= b.x && x <= b.x + b.width && y >= b.y && y <= b.y + b.height
        })
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// One socket as the card paints it.
#[derive(Clone, Debug)]
pub(crate) struct CardSocket {
    pub side: SocketSide,
    /// Distance from the card's top edge.
    pub from_top: f64,
    pub color: Color,
    pub shape: crate::socket_style::SocketShape,
    /// [`crate::socket_style::multi_input_stretch`] of a multi-input.
    pub stretch: f64,
}

/// What the card's chrome shows, refreshed by every editor layout.
/// A hosted card's drop shadow, in canvas units: how far it blurs out, its
/// offset (Y up) and its colour (`None`: the theme's window shadow).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardShadow {
    pub blur: f64,
    pub offset: [f64; 2],
    pub color: Option<Color>,
}

#[derive(Clone, Debug)]
pub(crate) struct CardChrome {
    pub title: String,
    pub title_color: Color,
    pub body: Color,
    pub border: Color,
    pub label: Color,
    pub selected: bool,
    pub badge: Option<(BadgeSeverity, Color)>,
    pub sockets: Vec<CardSocket>,
    pub style: crate::socket_style::NoodleStyle,
    /// `false` while the editor's sockets are off: none are drawn, but
    /// they keep their hit area.
    pub draw_sockets: bool,
    /// The editor's socket hit box, half extents in card units (`None`:
    /// the round [`SOCKET_HIT_RADIUS`]).
    pub socket_hit: Option<[f64; 2]>,
    /// The host's card shadow (`None`: agg-gui's window shadow).
    pub shadow: Option<CardShadow>,
}

/// A hosted card: chrome and sockets around the host's body widget.
pub struct HostedCard {
    pub(crate) node_id: NodeId,
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    socket_anchor: Option<SocketAnchorFn>,
    pub(crate) chrome: CardChrome,
}

impl HostedCard {
    pub(crate) fn new(node_id: NodeId, body: HostedNodeBody) -> Self {
        Self {
            node_id,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            children: vec![body.widget],
            socket_anchor: body.socket_anchor,
            chrome: CardChrome {
                title: String::new(),
                title_color: Color::rgba(0.3, 0.3, 0.3, 1.0),
                body: Color::rgba(0.2, 0.2, 0.2, 1.0),
                border: Color::rgba(0.1, 0.1, 0.1, 1.0),
                label: Color::rgba(1.0, 1.0, 1.0, 1.0),
                selected: false,
                badge: None,
                sockets: Vec::new(),
                style: Default::default(),
                draw_sockets: true,
                socket_hit: None,
                shadow: None,
            },
        }
    }

    /// The node this card shows.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Lay the body out at `width`, measure it, and place the sockets.
    pub(crate) fn layout_card(&mut self, node: &NodeView, width: f64) -> CardGeom {
        let body = &mut self.children[0];
        let measured = body.measure_min_height(width).max(0.0);
        let got = body.layout(Size::new(width, measured));
        let body_h = measured.max(got.height);
        if body_h > measured {
            body.layout(Size::new(width, body_h));
        }
        body.set_bounds(Rect::new(0.0, NODE_BOTTOM_PAD, width, body_h));

        let mut sockets = Vec::new();
        let named = node
            .outputs
            .iter()
            .map(|s| (s, SocketSide::Output))
            .chain(node.inputs.iter().map(|s| (s, SocketSide::Input)));
        for (i, (s, side)) in named.enumerate() {
            let anchor = self
                .socket_anchor
                .as_ref()
                .and_then(|f| f(self.children[0].as_ref(), &s.name));
            let (edge, from_body_top) = anchor.unwrap_or((side, ROW_HEIGHT * (i as f64 + 0.5)));
            sockets.push((s.name.clone(), edge, TITLE_HEIGHT + from_body_top));
        }
        CardGeom {
            width,
            height: card_height(body_h),
            sockets,
        }
    }

    fn style(&self, v: &agg_gui::Visuals) -> ChromeStyle {
        let mut style = ChromeStyle::from_visuals(v);
        style.corner_radius = NODE_RADIUS;
        style.title_height = TITLE_HEIGHT;
        style.body_color = self.chrome.body;
        style.border_color = if self.chrome.selected {
            v.accent
        } else {
            self.chrome.border
        };
        style.title_color = self.chrome.title_color;
        style.title_text_color = self.chrome.label;
        if let Some(shadow) = self.chrome.shadow {
            style.shadow_blur = shadow.blur;
            style.shadow_dx = shadow.offset[0];
            // `ChromeStyle`'s offset is Y down.
            style.shadow_dy = -shadow.offset[1];
            if let Some(color) = shadow.color {
                style.shadow_color = color;
            }
        }
        style
    }
}

impl Widget for HostedCard {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "HostedCard"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("node_id", self.node_id.0.to_string()),
            ("title", self.chrome.title.clone()),
            ("selected", self.chrome.selected.to_string()),
        ]
    }
    /// The editor lays the card out (`layout_card`); keep its size.
    fn layout(&mut self, _available: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let (w, h) = (self.bounds.width, self.bounds.height);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let v = ctx.visuals();
        let style = self.style(&v);
        paint_chrome_shadow(ctx, w, h, &style);
        paint_chrome_body(ctx, w, h, &style, false);
        paint_chrome_title_bar(
            ctx,
            0.0,
            h - TITLE_HEIGHT,
            w,
            &style,
            false,
            &self.chrome.title,
            TITLE_FONT_SIZE,
        );
        paint_chrome_border(ctx, w, h, &style);
        if self.chrome.selected {
            ctx.set_stroke_color(v.accent);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.rounded_rect(
                1.0,
                1.0,
                (w - 2.0).max(0.0),
                (h - 2.0).max(0.0),
                NODE_RADIUS,
            );
            ctx.stroke();
        }
        if let Some((_, color)) = self.chrome.badge {
            crate::draw_error::draw_error_outline(
                ctx,
                1.0,
                1.0,
                (w - 2.0).max(0.0),
                (h - 2.0).max(0.0),
                NODE_RADIUS,
                1.0,
                color,
            );
            let [cx, cy] = crate::draw_error::badge_center_in_title_bar(w, TITLE_HEIGHT, 1.0);
            crate::draw_error::draw_error_badge(ctx, [cx, cy + h - TITLE_HEIGHT], 1.0, color);
        }
    }

    /// Sockets go over the body (they straddle the card's edges), so they
    /// paint after the children.
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.chrome.draw_sockets {
            return;
        }
        let (w, h) = (self.bounds.width, self.bounds.height);
        for s in &self.chrome.sockets {
            let cx = match s.side {
                SocketSide::Input => 0.0,
                SocketSide::Output => w,
            };
            let cy = h - s.from_top;
            let paint = crate::socket_style::SocketPaint {
                shape: s.shape,
                color: s.color,
                stretch: s.stretch,
                style: self.chrome.style,
                border: self.chrome.border,
            };
            crate::socket_style::draw_socket(ctx, [cx, cy], &paint);
        }
    }

    /// A socket (half of which lies over the body) and the right-edge
    /// resize band belong to the editor, not to whatever body widget sits
    /// under them: the card claims the point and, ignoring the event, lets
    /// it bubble up to the editor's socket / resize handling.
    fn claims_pointer_exclusively(&self, p: Point) -> bool {
        let (w, h) = (self.bounds.width, self.bounds.height);
        if p.x >= w - RESIZE_GRIP && p.y < h - TITLE_HEIGHT {
            return true;
        }
        self.chrome.sockets.iter().any(|s| {
            let cx = match s.side {
                SocketSide::Input => 0.0,
                SocketSide::Output => w,
            };
            crate::draw::socket_hit([cx, h - s.from_top], [p.x, p.y], self.chrome.socket_hit)
        })
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
