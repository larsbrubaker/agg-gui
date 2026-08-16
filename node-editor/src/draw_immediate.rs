//! Immediate-mode node painting — the optional path for hosts that draw
//! nodes themselves instead of mounting the editor's retained widget
//! tree.
//!
//! **Nothing inside this crate calls it.** [`crate::NodeEditor`] builds
//! real child widgets (`crate::widget::nodes`) and paints those; this
//! module exists so a host holding a [`NodeLayoutInfo`] can render a
//! node onto its own canvas with the same look. Split out of
//! [`crate::draw`] both to keep that file under the 800-line cap and to
//! make the "layout here, live paint over there" boundary obvious.
//!
//! Anyone adding node chrome has to add it to the widget tree to make it
//! visible in the editor; primitives shared by both paths belong in a
//! module both can call, the way [`crate::draw_error`] is.
//!
//! Coordinates are canvas-space and **Y-up** — see [`crate::draw`].

use agg_gui::{Color, DrawCtx};

use crate::draw::{
    CanvasPalette, NodeLayoutInfo, NodeRow, PropLayout, SocketLayout, LABEL_OFFSET_Y, NODE_RADIUS,
    ROW_PADDING_X, SOCKET_RADIUS, TITLE_HEIGHT,
};
use crate::model::{NodeGraphModel, PropertyValue};

/// Render one node into the canvas (caller has already applied pan/zoom).
///
/// `model` is consulted for socket + category colours so the host's
/// palette decisions flow through.
///
/// # This is not the path [`crate::NodeEditor`] paints through
///
/// The widget builds a **retained** tree of `NodeWidget` /
/// `NodeHeaderWidget` / `NodeRowWidget` children (see
/// [`crate::widget::nodes`]) and paints those; nothing inside this crate
/// calls `draw_node`. It stays public as the immediate-mode option for
/// hosts that draw nodes themselves from a [`NodeLayoutInfo`].
///
/// Consequence for anyone adding node chrome: it has to land in the
/// widget tree to be visible in the editor, and the shared primitives
/// belong in a module both paths can call (as
/// [`crate::draw_error`] does).
pub fn draw_node<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    layout: &NodeLayoutInfo,
    selected: bool,
    palette: &CanvasPalette,
    model: &M,
) {
    draw_node_chrome(ctx, layout, selected, palette, model);
    for row in &layout.rows {
        draw_row(ctx, layout, row, palette, model);
    }
    // Last, so the badge sits on top of the title bar's label.
    crate::draw_error::draw_node_error(ctx, layout, palette);
}

fn draw_node_chrome<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    layout: &NodeLayoutInfo,
    selected: bool,
    palette: &CanvasPalette,
    model: &M,
) {
    let x = layout.top_left[0];
    let y_top = layout.top_left[1];
    let w = layout.size[0];
    let h = layout.size[1];
    let y_bot = y_top - h;
    let title_color = model.category_color(&layout.category, palette.node_title_fallback);

    ctx.set_fill_color(if selected {
        palette.node_body_selected
    } else {
        palette.node_body
    });
    ctx.begin_path();
    ctx.rounded_rect(x, y_bot, w, h, NODE_RADIUS);
    ctx.fill();

    ctx.set_fill_color(title_color);
    ctx.begin_path();
    ctx.rounded_rect(x, y_top - TITLE_HEIGHT, w, TITLE_HEIGHT, NODE_RADIUS);
    ctx.fill();
    ctx.set_fill_color(if selected {
        palette.node_body_selected
    } else {
        palette.node_body
    });
    ctx.begin_path();
    ctx.rect(x, y_top - TITLE_HEIGHT, w, NODE_RADIUS);
    ctx.fill();
    ctx.set_fill_color(title_color);
    ctx.begin_path();
    ctx.rect(
        x,
        y_top - TITLE_HEIGHT + NODE_RADIUS,
        w,
        TITLE_HEIGHT - NODE_RADIUS,
    );
    ctx.fill();

    ctx.set_stroke_color(palette.node_border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(x, y_bot, w, h, NODE_RADIUS);
    ctx.stroke();

    ctx.set_fill_color(palette.label_text);
    ctx.set_font_size(13.0);
    let title_y = y_top - TITLE_HEIGHT * 0.5 - 4.0;
    ctx.fill_text(&layout.display_name, x + 10.0, title_y);
}

fn draw_row<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    layout: &NodeLayoutInfo,
    row: &NodeRow,
    palette: &CanvasPalette,
    model: &M,
) {
    let x = layout.top_left[0];
    let w = layout.size[0];
    match row {
        NodeRow::Output(socket) => {
            draw_socket(ctx, socket, palette, model);
            // Right-align label so it hugs the dot.
            let label_y = socket.center[1] - 4.0;
            ctx.set_fill_color(palette.label_text);
            ctx.set_font_size(11.0);
            let est_width = (socket.display_label.len() as f64) * 6.5;
            ctx.fill_text(
                &socket.display_label,
                x + w - est_width - SOCKET_RADIUS * 2.0 - ROW_PADDING_X,
                label_y,
            );
        }
        NodeRow::Input { socket, editor } => {
            draw_socket(ctx, socket, palette, model);
            let label_y = socket.center[1] - 4.0;
            ctx.set_fill_color(palette.label_text);
            ctx.set_font_size(11.0);
            ctx.fill_text(
                &socket.display_label,
                x + SOCKET_RADIUS * 2.0 + ROW_PADDING_X,
                label_y,
            );
            if let Some(ed) = editor {
                draw_value_editor(ctx, ed, palette);
            }
        }
        NodeRow::Property(prop) => {
            draw_value_editor(ctx, prop, palette);
            // Name on the left of the editor's row.
            ctx.set_fill_color(palette.label_text);
            ctx.set_font_size(11.0);
            ctx.fill_text(
                &prop.name,
                prop.top_left[0] + ROW_PADDING_X,
                prop.top_left[1] - LABEL_OFFSET_Y,
            );
        }
    }
}

fn draw_socket<M: NodeGraphModel + ?Sized>(
    ctx: &mut dyn DrawCtx,
    socket: &SocketLayout,
    palette: &CanvasPalette,
    model: &M,
) {
    let c = model.socket_color(socket.socket_type);
    ctx.set_fill_color(c);
    ctx.begin_path();
    ctx.circle(socket.center[0], socket.center[1], SOCKET_RADIUS);
    ctx.fill();
    ctx.set_stroke_color(palette.node_border);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.circle(socket.center[0], socket.center[1], SOCKET_RADIUS);
    ctx.stroke();
}

fn draw_value_editor(ctx: &mut dyn DrawCtx, prop: &PropLayout, palette: &CanvasPalette) {
    let body_lum =
        0.299 * palette.node_body.r + 0.587 * palette.node_body.g + 0.114 * palette.node_body.b;
    let pill_bg = if body_lum < 0.5 {
        Color::rgba(0.15, 0.16, 0.20, 0.9)
    } else {
        Color::rgba(0.93, 0.93, 0.94, 0.9)
    };
    let pill_x = prop.top_left[0];
    let pill_y_top = prop.top_left[1];
    let pill_w = prop.size[0];
    let pill_h = prop.size[1] - 2.0;
    let pill_y_bot = pill_y_top - pill_h;

    ctx.set_fill_color(pill_bg);
    ctx.begin_path();
    ctx.rounded_rect(pill_x, pill_y_bot, pill_w, pill_h, 3.0);
    ctx.fill();

    // For Color, paint a swatch occupying the right half of the pill.
    if let PropertyValue::Color(c) = &prop.current {
        let swatch_inset = 3.0;
        ctx.set_fill_color(Color::rgba(c[0], c[1], c[2], c[3]));
        ctx.begin_path();
        ctx.rounded_rect(
            pill_x + swatch_inset,
            pill_y_bot + swatch_inset,
            pill_w - 2.0 * swatch_inset,
            pill_h - 2.0 * swatch_inset,
            2.0,
        );
        ctx.fill();
        return;
    }

    let value_str = format_value(&prop.current);
    ctx.set_fill_color(palette.label_text);
    ctx.set_font_size(11.0);
    let est = (value_str.len() as f64) * 6.0;
    let value_x = pill_x + pill_w - est - 6.0;
    ctx.fill_text(&value_str, value_x, pill_y_top - LABEL_OFFSET_Y);
}

fn format_value(v: &PropertyValue) -> String {
    match v {
        PropertyValue::Number(n) => {
            if (n.fract()).abs() < 1e-6 {
                format!("{}", *n as i64)
            } else {
                format!("{:.3}", n)
            }
        }
        PropertyValue::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        PropertyValue::Color(_) => String::new(),
        PropertyValue::Text(s) => s.clone(),
        PropertyValue::Other { display } => display.clone(),
    }
}
