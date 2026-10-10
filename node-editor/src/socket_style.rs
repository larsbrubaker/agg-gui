//! Socket and noodle presentation: socket shapes (circle, bar, diamond,
//! diamond with a dot, multi-input pill), the hover / drop-target ring,
//! and the two noodle looks ([`NoodleStyle::Simple`], the editor's
//! original thin curve, and [`NoodleStyle::NodeDesigner`]).
//!
//! The geometry follows MatterCAD's `NoodleRenderer` / `NodeSocketLayout`
//! (themselves NodeDesigner's `widgets/socket-shapes.js` and
//! `render-noodle.js`). Every helper draws in whatever space the caller's
//! `DrawCtx` is in — canvas units for the editor's noodles, card-local
//! units for a hosted card's sockets — with Y up.
//!
//! The model chooses what each socket and noodle looks like through the
//! defaulted hooks on [`crate::NodeGraphModel`] (`socket_shape`,
//! `socket_multi_input`, `noodle_dashed`, `noodle_color`,
//! `socket_hover_text`); the editor chooses the overall look with
//! `NodeEditor::with_noodle_style`. Callers: `widget/paint.rs` (noodles),
//! `widget/node_parts.rs`, `widget/hosted_card.rs` and `draw_immediate.rs`
//! (sockets), `widget/presentation.rs` (the hover ring and its label).

use agg_gui::{Color, DrawCtx, LineCap};

use crate::draw::SOCKET_RADIUS;

/// How a socket is drawn. MatterCAD: a bar for a single value (number,
/// boolean, string), a diamond for a field output, a diamond with a centre
/// dot for a field input (it takes a field or a single value), a circle
/// for everything else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SocketShape {
    /// The default round socket.
    #[default]
    Circle,
    /// A vertical bar with square ends.
    Bar,
    /// A diamond (a field output).
    Diamond,
    /// A diamond with a dark centre dot (a field input).
    DiamondDot,
}

/// The overall look of noodles and sockets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NoodleStyle {
    /// The editor's original look: a 2-unit curve in the socket colour,
    /// sockets outlined in the palette's node border. The default.
    #[default]
    Simple,
    /// NodeDesigner's (and MatterCAD's): a 3-unit curve in its colour over
    /// a 7-unit dark edge with a 5-unit dot at its middle; sockets 6 units
    /// in radius with a 1-unit dark outline round every shape.
    NodeDesigner,
}

/// NodeDesigner's socket radius (MatterCAD `NodeSocketLayout.SocketRadius`).
pub const NODE_DESIGNER_SOCKET_RADIUS: f64 = 6.0;
/// Half the width of a single-value bar socket.
pub const BAR_HALF_WIDTH: f64 = 3.0;
/// Half the height of a single-value bar socket.
pub const BAR_HALF_HEIGHT: f64 = 7.0;
/// The field diamond's half size: a little over the circle's radius, so
/// the diamond's area reads as a circle's.
pub const DIAMOND_HALF_SIZE: f64 = NODE_DESIGNER_SOCKET_RADIUS + 1.0;
/// The dark dot in a field input's diamond.
pub const FIELD_DOT_RADIUS: f64 = 2.0;
/// Distance between neighbouring noodles' landing points on a multi-input.
pub const MULTI_INPUT_SPACING: f64 = 10.0;
/// The dark outline round every NodeDesigner socket.
pub const SOCKET_OUTLINE_WIDTH: f64 = 1.0;
/// Width of the hover / drop-target ring.
pub const RING_WIDTH: f64 = 2.0;
/// NodeDesigner noodle: the dark edge's width.
pub const NOODLE_EDGE_WIDTH: f64 = 7.0;
/// NodeDesigner noodle: the coloured core's width.
pub const NOODLE_CORE_WIDTH: f64 = 3.0;
/// NodeDesigner noodle: the radius of the dot at its middle.
pub const NOODLE_DOT_RADIUS: f64 = 5.0;
/// Dash and gap length of a dashed noodle (NodeDesigner's `setLineDash([8, 8])`).
pub const NOODLE_DASH: f64 = 8.0;
/// Width of a [`NoodleStyle::Simple`] noodle.
pub const SIMPLE_NOODLE_WIDTH: f64 = 2.0;

/// NodeDesigner's socket outline and noodle edge colour, `#444`.
pub fn outline_color() -> Color {
    Color::rgba(
        0x44 as f32 / 255.0,
        0x44 as f32 / 255.0,
        0x44 as f32 / 255.0,
        1.0,
    )
}

/// How far, in canvas units, a multi-input's pill reaches past a plain
/// socket's middle above and below with `landed` noodles in it: enough to
/// space them [`MULTI_INPUT_SPACING`] apart. Zero with one or none.
pub fn multi_input_stretch(landed: usize) -> f64 {
    if landed > 1 {
        (landed - 1) as f64 * MULTI_INPUT_SPACING / 2.0
    } else {
        0.0
    }
}

/// Vertical offset (Y up) from a multi-input socket's centre at which the
/// noodle at `index` (in the model's noodle order) lands: the first
/// highest, each next [`MULTI_INPUT_SPACING`] lower, centred on the socket.
pub fn landing_offset(index: usize, landed: usize) -> f64 {
    if landed < 2 {
        0.0
    } else {
        multi_input_stretch(landed) - index as f64 * MULTI_INPUT_SPACING
    }
}

/// Everything one socket needs to be drawn.
#[derive(Clone, Copy, Debug)]
pub struct SocketPaint {
    pub shape: SocketShape,
    pub color: Color,
    /// [`multi_input_stretch`] of a multi-input; zero otherwise.
    pub stretch: f64,
    pub style: NoodleStyle,
    /// Outline colour for [`NoodleStyle::Simple`] (the palette's node border).
    pub border: Color,
}

impl SocketPaint {
    fn radius(&self) -> f64 {
        match self.style {
            NoodleStyle::Simple => SOCKET_RADIUS,
            NoodleStyle::NodeDesigner => NODE_DESIGNER_SOCKET_RADIUS,
        }
    }

    fn outline(&self) -> Color {
        match self.style {
            NoodleStyle::Simple => self.border,
            NoodleStyle::NodeDesigner => outline_color(),
        }
    }
}

/// Draw one socket centred on `center`.
pub fn draw_socket(ctx: &mut dyn DrawCtx, center: [f64; 2], p: &SocketPaint) {
    let [cx, cy] = center;
    let r = p.radius();
    // The original look, unchanged: a filled circle stroked in the border.
    if p.style == NoodleStyle::Simple && p.shape == SocketShape::Circle && p.stretch <= 0.0 {
        ctx.set_fill_color(p.color);
        ctx.begin_path();
        ctx.circle(cx, cy, r);
        ctx.fill();
        ctx.set_stroke_color(p.border);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.circle(cx, cy, r);
        ctx.stroke();
        return;
    }
    // MatterCAD's way: the outline is the shape grown by its width, filled
    // first, and the coloured shape over it.
    let o = SOCKET_OUTLINE_WIDTH;
    let outline = p.outline();
    match p.shape {
        SocketShape::Diamond | SocketShape::DiamondDot => {
            // A constant-width outline sits sqrt 2 further out at a diamond's points.
            fill_diamond(ctx, center, DIAMOND_HALF_SIZE + o * 2f64.sqrt(), outline);
            fill_diamond(ctx, center, DIAMOND_HALF_SIZE, p.color);
            if p.shape == SocketShape::DiamondDot {
                ctx.set_fill_color(outline);
                ctx.begin_path();
                ctx.circle(cx, cy, FIELD_DOT_RADIUS);
                ctx.fill();
            }
        }
        SocketShape::Bar => {
            fill_rect(
                ctx,
                center,
                BAR_HALF_WIDTH + o,
                BAR_HALF_HEIGHT + o,
                outline,
            );
            fill_rect(ctx, center, BAR_HALF_WIDTH, BAR_HALF_HEIGHT, p.color);
        }
        SocketShape::Circle if p.stretch > 0.0 => {
            fill_pill(ctx, center, r + o, p.stretch, outline);
            fill_pill(ctx, center, r, p.stretch, p.color);
        }
        SocketShape::Circle => {
            ctx.set_fill_color(outline);
            ctx.begin_path();
            ctx.circle(cx, cy, r + o);
            ctx.fill();
            ctx.set_fill_color(p.color);
            ctx.begin_path();
            ctx.circle(cx, cy, r);
            ctx.fill();
        }
    }
}

/// The 2-unit ring round a socket — the hovered one, or the one a drop
/// would connect to — just outside its outline, following its shape.
pub fn draw_socket_ring(ctx: &mut dyn DrawCtx, center: [f64; 2], p: &SocketPaint, ring: Color) {
    let [cx, cy] = center;
    let gap = SOCKET_OUTLINE_WIDTH + RING_WIDTH / 2.0;
    ctx.set_stroke_color(ring);
    ctx.set_line_width(RING_WIDTH);
    ctx.begin_path();
    match p.shape {
        SocketShape::Diamond | SocketShape::DiamondDot => {
            diamond_path(ctx, center, DIAMOND_HALF_SIZE + gap * 2f64.sqrt())
        }
        SocketShape::Bar => {
            let (hw, hh) = (BAR_HALF_WIDTH + gap, BAR_HALF_HEIGHT + gap);
            ctx.rect(cx - hw, cy - hh, 2.0 * hw, 2.0 * hh);
        }
        SocketShape::Circle if p.stretch > 0.0 => {
            pill_path(ctx, center, p.radius() + gap, p.stretch)
        }
        SocketShape::Circle => ctx.circle(cx, cy, p.radius() + gap),
    }
    ctx.stroke();
}

/// Draw a noodle from an output at `from` to an input at `to`: a cubic
/// whose control points lie a quarter of the ends' distance out
/// horizontally (rightward from the output, leftward into the input).
/// A `dashed` noodle (MatterCAD: one into a field input) is 8 on, 8 off,
/// edge and colour alike, with butt caps so the gaps stay open.
pub fn draw_noodle(
    ctx: &mut dyn DrawCtx,
    from: [f64; 2],
    to: [f64; 2],
    color: Color,
    dashed: bool,
    style: NoodleStyle,
) {
    if dashed {
        // Butt caps, as MatterCAD / NodeDesigner draw it: a round cap adds
        // half the 7-unit edge to each end of every dash and nearly closes
        // the 8-unit gaps.
        ctx.set_line_cap(LineCap::Butt);
        ctx.set_line_dash(&[NOODLE_DASH, NOODLE_DASH], 0.0);
    }
    match style {
        NoodleStyle::Simple => {
            crate::draw::draw_bezier_connection(ctx, from, to, color, SIMPLE_NOODLE_WIDTH)
        }
        NoodleStyle::NodeDesigner => {
            crate::draw::draw_bezier_connection(ctx, from, to, outline_color(), NOODLE_EDGE_WIDTH);
            crate::draw::draw_bezier_connection(ctx, from, to, color, NOODLE_CORE_WIDTH);
        }
    }
    if dashed {
        ctx.set_line_dash(&[], 0.0);
        // Back to agg-gui's default cap.
        ctx.set_line_cap(LineCap::Round);
    }
    if style == NoodleStyle::NodeDesigner {
        // The curve is symmetric, so its middle is the ends' midpoint.
        ctx.set_fill_color(color);
        ctx.begin_path();
        ctx.circle(
            (from[0] + to[0]) / 2.0,
            (from[1] + to[1]) / 2.0,
            NOODLE_DOT_RADIUS,
        );
        ctx.fill();
    }
}

fn diamond_path(ctx: &mut dyn DrawCtx, [cx, cy]: [f64; 2], half: f64) {
    ctx.move_to(cx, cy + half);
    ctx.line_to(cx + half, cy);
    ctx.line_to(cx, cy - half);
    ctx.line_to(cx - half, cy);
    ctx.close_path();
}

fn fill_diamond(ctx: &mut dyn DrawCtx, center: [f64; 2], half: f64, color: Color) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    diamond_path(ctx, center, half);
    ctx.fill();
}

fn fill_rect(ctx: &mut dyn DrawCtx, [cx, cy]: [f64; 2], hw: f64, hh: f64, color: Color) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.rect(cx - hw, cy - hh, 2.0 * hw, 2.0 * hh);
    ctx.fill();
}

/// An upright pill: `radius` either side, reaching `stretch` further up and down.
fn pill_path(ctx: &mut dyn DrawCtx, [cx, cy]: [f64; 2], radius: f64, stretch: f64) {
    ctx.rounded_rect(
        cx - radius,
        cy - radius - stretch,
        2.0 * radius,
        2.0 * (radius + stretch),
        radius,
    );
}

fn fill_pill(ctx: &mut dyn DrawCtx, center: [f64; 2], radius: f64, stretch: f64, color: Color) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    pill_path(ctx, center, radius, stretch);
    ctx.fill();
}
