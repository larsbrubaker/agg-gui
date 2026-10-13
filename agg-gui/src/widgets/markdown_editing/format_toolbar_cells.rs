//! How the formatting toolbar's button groups are drawn: the group's rounded
//! outline, the dividers between its cells, and each cell's state (the
//! accent tint of a style that covers the selection, the faint shade of one
//! that covers part of it, hover and press), plus the icon colours.  Split out
//! of `format_toolbar_widgets` so that file stays under the line limit.
//!
//! The whole group is painted here, in one device-pixel grid, rather than by
//! each button over its own bounds: every edge (outline, divider, cell) is
//! rounded to a whole device pixel by the same function, so at 1x, 1.5x and
//! 2x the strokes are one solid pixel wide, adjacent cells share their edges
//! with no hairline gap or overlap, and the end cells' tint follows the
//! outline's rounded corners.  Colours come from the theme's [`Visuals`]
//! (text, accent, widget stroke), so a dark theme needs nothing extra.

use std::rc::Rc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;
use crate::theme::Visuals;
use crate::widget::Widget;
use crate::widgets::Button;

use super::format_toolbar::ToolbarShared;
use super::format_toolbar_widgets::{MarkdownFormatButton, GROUP_DESIGN_RADIUS};
use super::inline_style::RichStyleCoverage;

/// Opacity of the accent over a cell whose style covers the whole selection.
const ACTIVE_TINT: f64 = 0.18;
/// ... while the pointer is over it, and while it is held down.
const ACTIVE_HOVER_TINT: f64 = 0.26;
const ACTIVE_PRESS_TINT: f64 = 0.34;
/// Opacity of the text colour over a cell whose style covers part of the
/// selection, a hovered cell and a held one.
const MIXED_SHADE: f64 = 0.10;
const HOVER_SHADE: f64 = 0.08;
const PRESS_SHADE: f64 = 0.15;
/// How far a pressed style's icon moves from the accent toward the text
/// colour, so it reads deeper than the tint behind it (C#
/// `PrimaryAccentColor.Blend(TextColor, .4)`).
const ACTIVE_ICON_BLEND: f64 = 0.4;
/// Opacity of a disabled button's icon.
const DISABLED_ICON_ALPHA: f64 = 0.35;

/// The icon colour: the text colour, a deeper accent while the style is on,
/// faded while the button is disabled.
pub(super) fn icon_color(v: &Visuals, enabled: bool, pressed: bool) -> Color {
    if !enabled {
        return with_alpha(v.text_color, DISABLED_ICON_ALPHA);
    }
    if pressed {
        let (a, t) = (v.accent, v.text_color);
        let mix = |from: f32, to: f32| from + (to - from) * ACTIVE_ICON_BLEND as f32;
        return Color::rgba(mix(a.r, t.r), mix(a.g, t.g), mix(a.b, t.b), 1.0);
    }
    v.text_color
}

fn with_alpha(c: Color, alpha: f64) -> Color {
    Color::rgba(c.r, c.g, c.b, alpha as f32 * c.a)
}

/// The cell fill for a button in its current state, or `None` for none.
fn cell_fill(
    v: &Visuals,
    coverage: RichStyleCoverage,
    enabled: bool,
    hovered: bool,
    held: bool,
) -> Option<Color> {
    if !enabled {
        return None;
    }
    let (hovered, held) = (hovered || held, held);
    match coverage {
        RichStyleCoverage::All => {
            let alpha = if held {
                ACTIVE_PRESS_TINT
            } else if hovered {
                ACTIVE_HOVER_TINT
            } else {
                ACTIVE_TINT
            };
            Some(with_alpha(v.accent, alpha))
        }
        _ => {
            let alpha = if held {
                PRESS_SHADE
            } else if hovered {
                HOVER_SHADE
            } else if coverage == RichStyleCoverage::Mixed {
                MIXED_SHADE
            } else {
                return None;
            };
            Some(with_alpha(v.text_color, alpha))
        }
    }
}

/// Paints a group: outline, cells and dividers, all on the device grid.
pub(super) fn paint_group(
    ctx: &mut dyn DrawCtx,
    bounds: Rect,
    children: &[Box<dyn Widget>],
    shared: &Rc<ToolbarShared>,
) {
    let v = ctx.visuals();
    let scale = crate::ux_scale::effective_scale().max(f64::MIN_POSITIVE);
    // Everything below is in device pixels, divided back to logical units for the context.
    ctx.save();
    ctx.snap_to_pixel();
    let px = |logical: f64| (logical * scale).round();
    let (w, h) = (px(bounds.width), px(bounds.height));
    let line = px(1.0).max(1.0);
    let radius = GROUP_DESIGN_RADIUS * scale;
    ctx.scale(1.0 / scale, 1.0 / scale);

    // The cells first, so the outline and dividers sit on their edges.
    let cells: Vec<(f64, f64, &MarkdownFormatButton)> = children
        .iter()
        .filter_map(|c| {
            let b = c.as_any()?.downcast_ref::<MarkdownFormatButton>()?;
            let r = c.bounds();
            Some((px(r.x), px(r.x + r.width), b))
        })
        .collect();
    let last = cells.len().saturating_sub(1);
    let inner = (radius - line).max(0.0);
    for (i, (x0, x1, button)) in cells.iter().enumerate() {
        let state = shared.model.borrow().buttons[button.kind().index()];
        let (hovered, held) = button
            .children()
            .first()
            .and_then(|c| c.as_any()?.downcast_ref::<Button>())
            .map_or((false, false), |b| (b.is_hovered(), b.is_pressed()));
        let Some(fill) = cell_fill(&v, state.coverage, state.enabled, hovered, held) else {
            continue;
        };
        // The end cells reach the outline's inner edge, and are rounded to follow it.
        let left = if i == 0 { line } else { *x0 };
        let right = if i == last { w - line } else { *x1 };
        let radii = [
            if i == 0 { inner } else { 0.0 },
            if i == last { inner } else { 0.0 },
            if i == last { inner } else { 0.0 },
            if i == 0 { inner } else { 0.0 },
        ];
        ctx.set_fill_color(fill);
        ctx.begin_path();
        cell_path(ctx, left, line, right - left, h - 2.0 * line, radii);
        ctx.fill();
    }

    // Dividers: the gap between two cells, from the outline's inside top to inside bottom.
    ctx.set_fill_color(v.widget_stroke);
    for pair in cells.windows(2) {
        let (left, right) = (pair[0].1, pair[1].0);
        let gap = (right - left).max(line);
        ctx.begin_path();
        ctx.rect(left, line, gap, h - 2.0 * line);
        ctx.fill();
    }

    // The outline: one device-pixel stroke, inset half its width so it fills whole pixels.
    ctx.set_stroke_color(v.widget_stroke);
    ctx.set_line_width(line);
    ctx.begin_path();
    ctx.rounded_rect(
        line * 0.5,
        line * 0.5,
        w - line,
        h - line,
        (radius - line * 0.5).max(0.0),
    );
    ctx.stroke();
    ctx.restore();
}

/// A rectangle with its own radius at each corner, `[bottom-left,
/// bottom-right, top-right, top-left]` (Y up).
fn cell_path(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64, r: [f64; 4]) {
    const KAPPA: f64 = 0.552_284_749_830_793_4;
    let [bl, br, tr, tl] = r;
    ctx.move_to(x + bl, y);
    ctx.line_to(x + w - br, y);
    ctx.cubic_to(
        x + w - br + br * KAPPA,
        y,
        x + w,
        y + br - br * KAPPA,
        x + w,
        y + br,
    );
    ctx.line_to(x + w, y + h - tr);
    ctx.cubic_to(
        x + w,
        y + h - tr + tr * KAPPA,
        x + w - tr + tr * KAPPA,
        y + h,
        x + w - tr,
        y + h,
    );
    ctx.line_to(x + tl, y + h);
    ctx.cubic_to(
        x + tl - tl * KAPPA,
        y + h,
        x,
        y + h - tl + tl * KAPPA,
        x,
        y + h - tl,
    );
    ctx.line_to(x, y + bl);
    ctx.cubic_to(x, y + bl - bl * KAPPA, x + bl - bl * KAPPA, y, x + bl, y);
    ctx.close_path();
}
