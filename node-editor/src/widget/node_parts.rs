//! The two leaf widgets inside a node row: the socket dot and the row
//! label.
//!
//! Split out of [`super::nodes`] (which sits at the project's 800-line
//! cap) with no behaviour change — `NodeRowWidget` still builds both,
//! and both still read their metrics from the shared
//! [`NodePaintContext`].
//!
//! Coordinates are parent-local and **Y-up**, like every other widget in
//! the tree: a row's origin is its bottom-left corner.

use agg_gui::{DrawCtx, Event, EventResult, Rect, Size, Widget, WidgetBase};

use super::nodes::{LABEL_FONT_SIZE, ROW_PADDING_X};
use crate::draw::{SocketLayout, SocketSide, SOCKET_RADIUS};

pub use super::node_paint_context::NodePaintContext;

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
    pub(super) fn new(
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
    pub(super) fn new_left(text: String, node_w: f64, row_h: f64, ctx: NodePaintContext) -> Self {
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

    pub(super) fn new_right(text: String, node_w: f64, row_h: f64, ctx: NodePaintContext) -> Self {
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
