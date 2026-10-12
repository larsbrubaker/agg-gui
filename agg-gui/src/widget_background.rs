//! A widget's rounded background and outline, and a progress bar's fill
//! inside that outline (agg-sharp `GuiWidget.RenderBackground` /
//! `InsetRoundedRect` / `InsetInvertsBounds` in `Gui/GUIWidget.cs`, and the
//! fill half of `Gui/ProgressBar.cs`'s `OnDraw`).
//!
//! Lengths are logical units: the [`DrawCtx`] transform carries the device
//! scale, so C#'s `outlineWidth * DeviceScale` is plain `outline_width` here.
//!
//! The geometry matters for "stadium" shapes (radius = half the height):
//! every inset piece also loses the inset from its corner radius, so the arcs
//! still fit the smaller box, and a piece whose inset would turn the rect
//! inside out is skipped rather than drawn as a thin band (agg's
//! `RoundedRect` puts inverted bounds back in order).

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::geometry::Rect;

/// C#'s `InsetInvertsBounds`: insetting `bounds` by `inset` eats more than
/// half its width or height.
fn inset_inverts_bounds(bounds: Rect, inset: f64) -> bool {
    inset * 2.0 > bounds.width || inset * 2.0 > bounds.height
}

/// C#'s `InsetRoundedRect` added to the current path: `bounds` inset by
/// `inset` on every side, the corner radius reduced by the same amount.
fn inset_rounded_rect(ctx: &mut dyn DrawCtx, bounds: Rect, radius: f64, inset: f64) {
    ctx.rounded_rect(
        bounds.x + inset,
        bounds.y + inset,
        bounds.width - inset * 2.0,
        bounds.height - inset * 2.0,
        (radius - inset).max(0.0),
    );
}

/// C#'s `GuiWidget.RenderBackground`: fills `bounds` with `background`
/// (rounded by `radius`) and, when `outline_color` is visible and
/// `outline_width > 0`, strokes an outline whose outer edge lands on
/// `bounds`, with the background inset to sit entirely inside it. A piece
/// that does not fit (an outline wider than the widget) paints nothing.
pub fn render_background(
    ctx: &mut dyn DrawCtx,
    bounds: Rect,
    background: Color,
    radius: f64,
    outline_width: f64,
    outline_color: Color,
) {
    if outline_color.a > 0.0 && outline_width > 0.0 {
        let stroke = outline_width;
        if background.a > 0.0 && !inset_inverts_bounds(bounds, stroke) {
            // the background sits entirely inside the border, so it insets by
            // the full stroke width
            ctx.set_fill_color(background);
            ctx.begin_path();
            inset_rounded_rect(ctx, bounds, radius, stroke);
            ctx.fill();
        }
        // the border's centerline is half a stroke in from the bounds so the
        // stroke's outer edge lands on them
        if !inset_inverts_bounds(bounds, stroke / 2.0) {
            ctx.set_stroke_color(outline_color);
            ctx.set_line_width(stroke);
            ctx.begin_path();
            inset_rounded_rect(ctx, bounds, radius, stroke / 2.0);
            ctx.stroke();
        }
    } else if background.a > 0.0 {
        ctx.set_fill_color(background);
        ctx.begin_path();
        inset_rounded_rect(ctx, bounds, radius, 0.0);
        ctx.fill();
    }
}

/// The fill of agg-sharp's `ProgressBar.OnDraw`, drawn after
/// [`render_background`] with the same `radius`, `outline_width` and
/// `outline_color`: `ratio` of the room inside the outline, from its left
/// edge, rounded with the radius less the inset (and narrowed further once
/// the fill is thinner than the corner diameter, so the arcs never sweep past
/// the fill's right edge). With no outline (`outline_width <= 0`) and a
/// visible `outline_color`, the legacy sharp one-unit rect border is drawn
/// after the fill, as C# does.
pub fn render_progress_fill(
    ctx: &mut dyn DrawCtx,
    bounds: Rect,
    ratio: f64,
    fill: Color,
    radius: f64,
    outline_width: f64,
    outline_color: Color,
) {
    // the fill goes down after the outline, so it has to stop where the
    // background does - inset by the full stroke width
    let inset = if outline_color.a > 0.0 && outline_width > 0.0 {
        outline_width
    } else {
        0.0
    };
    let left = bounds.x + inset;
    let bottom = bounds.y + inset;
    let top = bounds.y + bounds.height - inset;
    let room = (bounds.width - inset * 2.0).max(0.0);
    let fill_width = room.min(room * ratio).max(0.0);
    if fill_width > 0.0 && top > bottom {
        ctx.set_fill_color(fill);
        ctx.begin_path();
        if radius == 0.0 {
            ctx.rect(left, bottom, fill_width, top - bottom);
        } else {
            // DrawCtx::rounded_rect normalizes the radius to the box, as
            // C#'s explicit `normalize_radius` call does
            ctx.rounded_rect(
                left,
                bottom,
                fill_width,
                top - bottom,
                (radius - inset).max(0.0),
            );
        }
        ctx.fill();
    }
    // the legacy sharp rect border for zero-outline bars (C#'s
    // `graphics2D.Rectangle(LocalBounds, BorderColor)`: a one-unit stroke on
    // a centerline half a device pixel in from the bounds), after the fill so
    // a full bar cannot cover it
    if outline_color.a > 0.0 && outline_width <= 0.0 {
        let half_pixel = 0.5 / crate::device_scale::device_scale();
        ctx.set_stroke_color(outline_color);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.rect(
            bounds.x + half_pixel,
            bounds.y + half_pixel,
            bounds.width - half_pixel * 2.0,
            bounds.height - half_pixel * 2.0,
        );
        ctx.stroke();
    }
}
