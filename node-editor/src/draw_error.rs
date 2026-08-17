//! Badge chrome for a node whose host reported a failed *or degraded*
//! evaluation — the shared primitives, in whatever local space the
//! caller is drawing in.
//!
//! The shape is identical for both severities; only the colour differs
//! ([`CanvasPalette::badge_color`]). A node carrying both an error and a
//! warning wears the error badge — one badge fits on a title bar, and
//! the louder state is the one the user must act on
//! ([`crate::model::badge_of`]).
//!
//! Two callers, one implementation:
//!
//! - the **live** canvas: [`super::widget::nodes::NodeWidget::paint`]
//!   draws the outline and its `NodeHeaderWidget` child draws the badge,
//!   both in screen-space widget-local coordinates. This is what the
//!   user sees.
//! - the immediate-mode [`crate::draw::draw_node`], in canvas space, for
//!   hosts that render nodes themselves rather than mounting the
//!   editor's retained widget tree.
//!
//! # Why a badge and not a tooltip
//!
//! The canvas has no tooltip path down to nodes, and an error message is
//! usually a sentence — too long for a node title bar. So the canvas
//! answers "*which* node is broken" (an error-coloured outline plus a
//! round `!` badge at the title bar's right end) and leaves "what went
//! wrong" to the host's own surface, which reads
//! [`crate::NodeView::error`] (AtomArtist posts it to its status bar).
//!
//! Coordinates are **Y-up** everywhere, so "the top of the title bar" is
//! the larger Y.

use agg_gui::{Color, DrawCtx};

use crate::draw::{NodeLayoutInfo, NODE_RADIUS, TITLE_HEIGHT};
use crate::palette::CanvasPalette;

/// Radius of the round `!` badge on the title bar, in unscaled units.
pub const ERROR_BADGE_RADIUS: f64 = 7.0;

/// Gap between the badge's center and the node's right edge, unscaled.
/// Big enough to clear the badge itself plus a hair of padding, and the
/// title bar's other occupant (the collapse chevron) sits at the *left*
/// edge, so the two never collide.
pub const ERROR_BADGE_INSET: f64 = 12.0;

/// Width of the error outline, unscaled — heavier than the normal
/// border so a broken node reads at a glance even zoomed out.
pub const ERROR_OUTLINE_WIDTH: f64 = 2.0;

/// Center of the error badge inside a **title bar** of size `w` × `h`
/// whose own origin is its bottom-left corner (the header widget's local
/// space). `scale` is the canvas zoom already baked into `w` / `h`.
pub fn badge_center_in_title_bar(w: f64, h: f64, scale: f64) -> [f64; 2] {
    [w - ERROR_BADGE_INSET * scale, h * 0.5]
}

/// Center of the error badge for a canvas-space node layout — the
/// immediate-mode path's equivalent of
/// [`badge_center_in_title_bar`].
pub fn error_badge_center(layout: &NodeLayoutInfo) -> [f64; 2] {
    [
        layout.top_left[0] + layout.size[0] - ERROR_BADGE_INSET,
        layout.top_left[1] - TITLE_HEIGHT * 0.5,
    ]
}

/// Re-stroke a node body outline in the error colour.
pub fn draw_error_outline(
    ctx: &mut dyn DrawCtx,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
    scale: f64,
    color: Color,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    ctx.set_stroke_color(color);
    ctx.set_line_width(ERROR_OUTLINE_WIDTH * scale);
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, radius);
    ctx.stroke();
}

/// Draw the round `!` badge centred on `center`.
///
/// The exclamation mark is a stem and a dot drawn as paths rather than
/// text, so the badge never depends on a glyph being present in the
/// host's font.
pub fn draw_error_badge(ctx: &mut dyn DrawCtx, center: [f64; 2], scale: f64, color: Color) {
    let [cx, cy] = center;
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.circle(cx, cy, ERROR_BADGE_RADIUS * scale);
    ctx.fill();

    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path();
    ctx.rect(cx - 1.0 * scale, cy - 1.5 * scale, 2.0 * scale, 6.0 * scale);
    ctx.fill();
    ctx.begin_path();
    ctx.circle(cx, cy - 3.5 * scale, 1.2 * scale);
    ctx.fill();
}

/// Immediate-mode convenience: outline + badge for a canvas-space
/// layout. A no-op for a healthy node, so callers can call it
/// unconditionally.
pub fn draw_node_error(ctx: &mut dyn DrawCtx, layout: &NodeLayoutInfo, palette: &CanvasPalette) {
    let Some((severity, _)) = layout.badge() else {
        return;
    };
    let color = palette.badge_color(severity);
    draw_error_outline(
        ctx,
        layout.top_left[0],
        layout.top_left[1] - layout.size[1],
        layout.size[0],
        layout.size[1],
        NODE_RADIUS,
        1.0,
        color,
    );
    draw_error_badge(ctx, error_badge_center(layout), 1.0, color);
}
