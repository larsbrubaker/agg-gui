//! Composed `Widget` tree for the node-editor canvas.
//!
//! Every visible piece of a node is now a real `Widget` with a proper
//! child-parent relationship:
//!
//! ```text
//! NodeWidget                       — the node body + chrome
//! ├── NodeHeaderWidget             — title bar (drawn first)
//! └── NodeRowWidget* (one per row)
//!     ├── SocketDotWidget?         — the connector dot (left or right)
//!     ├── RowLabelWidget           — the row's text label
//!     └── ValueEditorWidget?       — inline number / color / bool editor
//! ```
//!
//! Coordinates follow agg-gui's convention: parent-local, Y-up, origin
//! at the parent's **bottom-left** corner.  `NodeWidget`'s own bounds
//! live in canvas-space — `NodeEditor` already has the pan/zoom transform
//! applied to its `DrawCtx` when it calls `paint_subtree` on the node
//! widgets, so canvas-space happens to be the right space for the
//! `NodeWidget` bounds.
//!
//! The widgets are paint-side only: they consume an immutable
//! `NodeLayoutInfo` produced by `crate::draw` plus the live `CanvasPalette`
//! and `NodeGraphModel`.  Hit-testing for selection, drag, and connection
//! drawing continues to flow through `NodeLayoutInfo` on `NodeEditor`
//! itself; the per-widget bounds give the inspector a real tree to walk
//! without forcing a second event-routing rewrite.

use agg_gui::{
    DrawCtx, Event, EventResult, HAnchor, Insets, Rect, Size, VAnchor, Widget, WidgetBase,
};

use crate::draw::{
    NodeLayoutInfo, NodeRow, SocketLayout, SocketSide, NODE_RADIUS, ROW_HEIGHT, SOCKET_RADIUS,
    TITLE_HEIGHT,
};

pub(super) const ROW_PADDING_X: f64 = 6.0;
pub(super) const LABEL_FONT_SIZE: f64 = 11.0;
const TITLE_FONT_SIZE: f64 = 13.0;

pub use super::node_paint_context::NodePaintContext;
pub use super::value_editor_widget::ValueEditorWidget;

// ---------------------------------------------------------------------------
// NodeWidget — the top-level node container
// ---------------------------------------------------------------------------

/// A full node — chrome (body, header, border) plus a row child for
/// every output, input, and unbound property.
pub struct NodeWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    node_id: crate::model::NodeId,
    display_name: String,
    category: String,
    selected: bool,
    ctx: NodePaintContext,
}

impl NodeWidget {
    /// Construct a fresh widget tree mirroring `layout`, with no canvas
    /// pan/zoom applied — bounds land at canvas-space positions
    /// directly.  Convenience for callers that don't have a live
    /// canvas transform (tests, default render at scale=1).
    pub fn from_layout(layout: &NodeLayoutInfo, selected: bool, ctx: NodePaintContext) -> Self {
        Self::from_layout_transformed(layout, selected, ctx, 1.0, [0.0, 0.0])
    }

    /// Construct a fresh widget tree with bounds baked in
    /// **screen-space**.  `scale` and `canvas_offset` flatten the
    /// canvas pan/zoom into every dimension (node bounds, row bounds,
    /// socket radii, font sizes) so the framework's per-child translate
    /// — which adds bounds additively in screen-space without
    /// respecting a parent scale — lands at the right pixels.  This is
    /// also what lets `collect_inspector_nodes` report on-screen rects
    /// for the F12-style hover overlay.
    pub fn from_layout_transformed(
        layout: &NodeLayoutInfo,
        selected: bool,
        mut ctx: NodePaintContext,
        scale: f64,
        canvas_offset: [f64; 2],
    ) -> Self {
        ctx.scale = scale;
        let canvas_w = layout.size[0];
        let canvas_h = layout.size[1];
        let screen_w = canvas_w * scale;
        let screen_h = canvas_h * scale;
        // Y-up: layout.top_left[1] is the canvas-space TOP of the node;
        // widget bounds use the bottom-left corner.  Convert to screen
        // by multiplying canvas position by scale then adding the
        // canvas pan offset.
        let screen_bottom_x = layout.top_left[0] * scale + canvas_offset[0];
        let screen_bottom_y = (layout.top_left[1] - canvas_h) * scale + canvas_offset[1];
        let bounds = Rect::new(screen_bottom_x, screen_bottom_y, screen_w, screen_h);

        let mut children: Vec<Box<dyn Widget>> = Vec::with_capacity(layout.rows.len() + 1);
        children.push(Box::new(NodeHeaderWidget::new(
            screen_w,
            screen_h,
            layout.display_name.clone(),
            layout.category.clone(),
            ctx.clone(),
        )));

        for (row_index, row) in layout.rows.iter().enumerate() {
            children.push(Box::new(NodeRowWidget::from_row(
                row,
                row_index,
                screen_w,
                screen_h,
                ctx.clone(),
            )));
        }

        Self {
            bounds,
            base: WidgetBase::new()
                .with_h_anchor(HAnchor::FIT)
                .with_v_anchor(VAnchor::FIT),
            children,
            node_id: layout.node_id,
            display_name: layout.display_name.clone(),
            category: layout.category.clone(),
            selected,
            ctx,
        }
    }

    pub fn node_id(&self) -> crate::model::NodeId {
        self.node_id
    }
}

impl Widget for NodeWidget {
    fn type_name(&self) -> &'static str {
        "NodeWidget"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn margin(&self) -> Insets {
        self.base.margin
    }
    // The canvas pans / zooms in fractional units; force-snapping to
    // device pixels at every node would visibly jitter during pan.
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("node_id", format!("{}", self.node_id.0)),
            ("display_name", self.display_name.clone()),
            ("category", self.category.clone()),
            ("selected", format!("{}", self.selected)),
        ]
    }

    fn layout(&mut self, available: Size) -> Size {
        // Bounds are owned by the parent (the canvas) — return what we
        // already carry so we keep the node-space size.
        let _ = available;
        Size::new(self.bounds.width, self.bounds.height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let body_color = if self.selected {
            self.ctx.palette.node_body_selected
        } else {
            self.ctx.palette.node_body
        };
        let r = NODE_RADIUS * self.ctx.scale;
        // Body fill.
        ctx.set_fill_color(body_color);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();
        // Border.
        ctx.set_stroke_color(self.ctx.palette.node_border);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.stroke();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        // Event routing is still owned by `NodeEditor` (canvas-space
        // hit testing).  This widget exists for composition + paint.
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// NodeHeaderWidget — the coloured title bar
// ---------------------------------------------------------------------------

pub struct NodeHeaderWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    title: String,
    category: String,
    ctx: NodePaintContext,
}

impl NodeHeaderWidget {
    fn new(node_w: f64, node_h: f64, title: String, category: String, ctx: NodePaintContext) -> Self {
        // `node_w` and `node_h` are already in screen-space (the
        // caller pre-scaled them); the header's logical height
        // `TITLE_HEIGHT` needs the same treatment.
        let title_h = TITLE_HEIGHT * ctx.scale;
        let bounds = Rect::new(0.0, node_h - title_h, node_w, title_h);
        Self {
            bounds,
            base: WidgetBase::new(),
            children: Vec::new(),
            title,
            category,
            ctx,
        }
    }
}

impl Widget for NodeHeaderWidget {
    fn type_name(&self) -> &'static str {
        "NodeHeaderWidget"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let s = self.ctx.scale;
        let r = NODE_RADIUS * s;
        let title_color =
            (self.ctx.title_colors)(&self.category, self.ctx.palette.node_title_fallback);
        // Rounded top corners by painting a rounded rect then masking
        // the bottom strip with a rectangle.  Visually identical to
        // `draw_node_chrome`'s previous logic.
        ctx.set_fill_color(title_color);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, r);
        ctx.fill();
        ctx.set_fill_color(title_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, r);
        ctx.fill();

        ctx.set_fill_color(self.ctx.palette.label_text);
        ctx.set_font_size(TITLE_FONT_SIZE * s);
        // Text baseline ~4px above the header's bottom, matching the
        // previous procedural layout (scaled).
        ctx.fill_text(&self.title, 10.0 * s, h * 0.5 - 4.0 * s);
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// NodeRowWidget — a single row inside a node, with its own sub-widget tree
// ---------------------------------------------------------------------------

pub struct NodeRowWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    row_name: String,
    row_kind: RowKind,
}

#[derive(Clone, Debug)]
enum RowKind {
    Output,
    Input { has_editor: bool },
    Property,
}

impl NodeRowWidget {
    fn from_row(
        row: &NodeRow,
        row_index: usize,
        node_w: f64,
        node_h: f64,
        ctx: NodePaintContext,
    ) -> Self {
        // `node_w`, `node_h` are already in screen-space (the caller
        // pre-scaled them); the row's logical metrics need the same
        // treatment so a scaled node's interior is visually consistent.
        let s = ctx.scale;
        let title_h = TITLE_HEIGHT * s;
        let row_h = ROW_HEIGHT * s;
        // Row at `row_index` (0 = top, directly under the title) sits at
        // y ∈ [node_h - title_h - (row_index+1)*row_h,
        //      node_h - title_h - row_index *row_h].
        let row_top = node_h - title_h - (row_index as f64) * row_h;
        let row_bot = row_top - row_h;
        let bounds = Rect::new(0.0, row_bot, node_w, row_h);

        let (row_name, row_kind, children) = match row {
            NodeRow::Output(socket) => {
                let mut children: Vec<Box<dyn Widget>> = Vec::new();
                children.push(Box::new(SocketDotWidget::new(
                    socket.clone(),
                    SocketSide::Output,
                    node_w,
                    row_h,
                    ctx.clone(),
                )));
                children.push(Box::new(RowLabelWidget::new_right(
                    socket.display_label.clone(),
                    node_w,
                    row_h,
                    ctx.clone(),
                )));
                (
                    format!("output:{}", socket.name),
                    RowKind::Output,
                    children,
                )
            }
            NodeRow::Input { socket, editor } => {
                let mut children: Vec<Box<dyn Widget>> = Vec::new();
                children.push(Box::new(SocketDotWidget::new(
                    socket.clone(),
                    SocketSide::Input,
                    node_w,
                    row_h,
                    ctx.clone(),
                )));
                children.push(Box::new(RowLabelWidget::new_left(
                    socket.display_label.clone(),
                    node_w,
                    row_h,
                    ctx.clone(),
                )));
                let has_editor = editor.is_some();
                if let Some(ed) = editor {
                    children.push(Box::new(ValueEditorWidget::new(
                        ed.clone(),
                        node_w,
                        row_h,
                        ctx.clone(),
                        /* show_label */ false,
                    )));
                }
                (
                    format!("input:{}", socket.name),
                    RowKind::Input { has_editor },
                    children,
                )
            }
            NodeRow::Property(prop) => {
                let mut children: Vec<Box<dyn Widget>> = Vec::new();
                children.push(Box::new(ValueEditorWidget::new(
                    prop.clone(),
                    node_w,
                    ROW_HEIGHT,
                    ctx.clone(),
                    /* show_label */ true,
                )));
                (
                    format!("prop:{}", prop.name),
                    RowKind::Property,
                    children,
                )
            }
        };

        Self {
            bounds,
            base: WidgetBase::new(),
            children,
            row_name,
            row_kind,
        }
    }
}

impl Widget for NodeRowWidget {
    fn type_name(&self) -> &'static str {
        "NodeRowWidget"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("row", self.row_name.clone()),
            (
                "kind",
                match &self.row_kind {
                    RowKind::Output => "output".into(),
                    RowKind::Input { has_editor } => format!("input(editor={has_editor})"),
                    RowKind::Property => "property".into(),
                },
            ),
        ]
    }
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Row backdrop is invisible — visuals come from children.
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// SocketDotWidget — the coloured circle on the left or right edge
// ---------------------------------------------------------------------------

pub struct SocketDotWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    socket: SocketLayout,
    side: SocketSide,
    ctx: NodePaintContext,
}

impl SocketDotWidget {
    fn new(
        socket: SocketLayout,
        side: SocketSide,
        node_w: f64,
        row_h: f64,
        ctx: NodePaintContext,
    ) -> Self {
        // `node_w`, `row_h` are already in screen-space; SOCKET_RADIUS
        // needs the same scale.
        let cx = match side {
            SocketSide::Input => 0.0,
            SocketSide::Output => node_w,
        };
        let cy = row_h * 0.5;
        let r = SOCKET_RADIUS * ctx.scale;
        let bounds = Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
        Self {
            bounds,
            base: WidgetBase::new(),
            children: Vec::new(),
            socket,
            side,
            ctx,
        }
    }
}

impl Widget for SocketDotWidget {
    fn type_name(&self) -> &'static str {
        "SocketDotWidget"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("socket", self.socket.name.clone()),
            (
                "side",
                match self.side {
                    SocketSide::Input => "input".into(),
                    SocketSide::Output => "output".into(),
                },
            ),
            ("type", format!("{}", self.socket.socket_type.0)),
        ]
    }
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // The widget is a 2R x 2R square; draw the dot at its centre in
        // local coords.  `bounds.width` is exactly 2*SOCKET_RADIUS so
        // we can recover the radius without referencing the constant.
        let r = self.bounds.width * 0.5;
        let cx = r;
        let cy = self.bounds.height * 0.5;
        let fill = (self.ctx.socket_colors)(self.socket.socket_type);
        ctx.set_fill_color(fill);
        ctx.begin_path();
        ctx.circle(cx, cy, r);
        ctx.fill();
        ctx.set_stroke_color(self.ctx.palette.node_border);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.circle(cx, cy, r);
        ctx.stroke();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ---------------------------------------------------------------------------
// RowLabelWidget — the row's text label
// ---------------------------------------------------------------------------

/// Where the label hugs the row — left edge (input rows) or right edge
/// (output rows).
#[derive(Clone, Copy, Debug)]
enum LabelSide {
    Left,
    Right,
}

pub struct RowLabelWidget {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
    text: String,
    side: LabelSide,
    ctx: NodePaintContext,
}

impl RowLabelWidget {
    fn new_left(text: String, node_w: f64, row_h: f64, ctx: NodePaintContext) -> Self {
        // Reserve from the dot's right edge to the right edge of the
        // row.  Painting reads `text_x` from `side`.  All horizontal
        // metrics scale with the active canvas zoom.
        let s = ctx.scale;
        let left = (SOCKET_RADIUS * 2.0 + ROW_PADDING_X) * s;
        let bounds = Rect::new(left, 0.0, (node_w - left).max(0.0), row_h);
        Self {
            bounds,
            base: WidgetBase::new(),
            children: Vec::new(),
            text,
            side: LabelSide::Left,
            ctx,
        }
    }

    fn new_right(text: String, node_w: f64, row_h: f64, ctx: NodePaintContext) -> Self {
        let s = ctx.scale;
        let right_inset = (SOCKET_RADIUS * 2.0 + ROW_PADDING_X) * s;
        let width = (node_w - right_inset).max(0.0);
        let bounds = Rect::new(0.0, 0.0, width, row_h);
        Self {
            bounds,
            base: WidgetBase::new(),
            children: Vec::new(),
            text,
            side: LabelSide::Right,
            ctx,
        }
    }
}

impl Widget for RowLabelWidget {
    fn type_name(&self) -> &'static str {
        "RowLabelWidget"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn enforce_integer_bounds(&self) -> bool {
        false
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("text", self.text.clone())]
    }
    fn layout(&mut self, _: Size) -> Size {
        Size::new(self.bounds.width, self.bounds.height)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if self.text.is_empty() {
            return;
        }
        let s = self.ctx.scale;
        ctx.set_fill_color(self.ctx.palette.label_text);
        ctx.set_font_size(LABEL_FONT_SIZE * s);
        let baseline_y = self.bounds.height * 0.5 - 4.0 * s;
        let x = match self.side {
            LabelSide::Left => 0.0,
            LabelSide::Right => {
                let est = (self.text.len() as f64) * 6.5 * s;
                (self.bounds.width - est).max(0.0)
            }
        };
        ctx.fill_text(&self.text, x, baseline_y);
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}


