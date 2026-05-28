//! Toggle editor renderer — fixed-width switch pill anchored at the
//! right edge of the editor area.
//!
//! The pill is a rounded track (~36×16 px at scale=1) with a circular
//! thumb. Pill stays a fixed visual width so wider editor areas
//! don't stretch the toggle — instead, extra space sits to the left
//! of the pill.

use crate::{Color, DrawCtx, Rect};

use super::super::value::RowValue;

/// Track width at scale=1 — kept fixed so the toggle stays a "normal
/// sized" control instead of spanning the full editor area.
const TRACK_W: f64 = 36.0;
/// Track height at scale=1.
const TRACK_H: f64 = 16.0;

pub(crate) fn paint_editor(ctx: &mut dyn DrawCtx, editor_area: Rect, value: RowValue, scale: f64) {
    let track_w = TRACK_W * scale;
    let track_h = TRACK_H * scale;
    // Anchor pill at the right edge of the editor area.
    let track_x = editor_area.x + editor_area.width - track_w - 8.0 * scale;
    let track_y = editor_area.y + (editor_area.height - track_h) * 0.5;

    let b = matches!(value, RowValue::Bool(true));
    let visuals = ctx.visuals().clone();
    let track_color = if b {
        visuals.accent
    } else {
        let dim = visuals.text_dim;
        Color::rgba(dim.r, dim.g, dim.b, 0.35)
    };
    ctx.set_fill_color(track_color);
    ctx.begin_path();
    ctx.rounded_rect(track_x, track_y, track_w, track_h, track_h * 0.5);
    ctx.fill();

    let thumb_d = (track_h - 4.0 * scale).max(6.0 * scale);
    let thumb_y = track_y + (track_h - thumb_d) * 0.5;
    let thumb_x = if b {
        track_x + track_w - thumb_d - 2.0 * scale
    } else {
        track_x + 2.0 * scale
    };
    ctx.set_fill_color(Color::rgba(0.98, 0.98, 0.98, 1.0));
    ctx.begin_path();
    ctx.rounded_rect(thumb_x, thumb_y, thumb_d, thumb_d, thumb_d * 0.5);
    ctx.fill();
}

/// Hit-rect for the toggle pill — hosts use this to map a click on
/// the row into a toggle flip. Mirrors the geometry in
/// [`paint_editor`].
#[allow(dead_code)]
pub fn hit_rect(editor_area: Rect, scale: f64) -> Rect {
    let track_w = TRACK_W * scale;
    let track_h = TRACK_H * scale;
    let track_x = editor_area.x + editor_area.width - track_w - 8.0 * scale;
    let track_y = editor_area.y + (editor_area.height - track_h) * 0.5;
    Rect::new(track_x, track_y, track_w, track_h)
}
