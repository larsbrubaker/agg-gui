//! Slider editor renderer — slider track + thumb + right-aligned
//! value text inside an editor pill.
//!
//! Both [`paint_editor`] (for `EditorKind::Slider`) and
//! [`paint_editor_drag`] (for `EditorKind::NumberDrag`) live here.
//! Both paint the same pill + numeric text; only Slider draws the
//! track-fill underlay showing the current value position between
//! `min` and `max`.

use crate::{DrawCtx, Rect};

use super::super::editor::NumberAttrs;
use super::super::value::RowValue;
use super::{editor_pill_rect, format_number, paint_pill_bg};

pub(crate) fn paint_editor(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    attrs: &NumberAttrs,
    scale: f64,
) {
    let pill = editor_pill_rect(editor_area, scale);
    paint_pill_bg(ctx, pill, scale);

    if let RowValue::Number(n) = value {
        // Fill bar — fraction of the track from `min` to current.
        if let (Some(mn), Some(mx)) = (attrs.min, attrs.max) {
            if mx > mn {
                let t = ((n - mn) / (mx - mn)).clamp(0.0, 1.0);
                let visuals = ctx.visuals().clone();
                let mut fill = visuals.accent;
                fill.a = 0.35;
                let fill_w = pill.width * t;
                if fill_w > 0.0 {
                    ctx.set_fill_color(fill);
                    ctx.begin_path();
                    ctx.rounded_rect(pill.x, pill.y, fill_w, pill.height, 3.0 * scale);
                    ctx.fill();
                }
            }
        }

        paint_centred_value(ctx, pill, n, attrs, scale);
    }
}

/// Drag-value variant — pill + left/right drag arrows + centred numeric
/// text, no track fill. Visually mirrors the standalone
/// [`crate::widgets::DragValue`] widget so a NumberDrag property row and
/// a DragValue read as the same control: the drag arrows are the
/// affordance for horizontal scrubbing, and a plain click enters inline
/// keyboard editing (the interaction contract lives host-side, e.g. the
/// node-editor's canvas dispatcher).
pub(crate) fn paint_editor_drag(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    attrs: &NumberAttrs,
    scale: f64,
) {
    let pill = editor_pill_rect(editor_area, scale);
    paint_pill_bg(ctx, pill, scale);
    paint_drag_arrows(ctx, pill, scale);

    if let RowValue::Number(n) = value {
        paint_centred_value(ctx, pill, n, attrs, scale);
    }
}

/// Paint the left ("◀") / right ("▶") drag-affordance triangles inside a
/// pill. Proportions mirror `DragValue`'s arrow triangles (scaled) so the
/// row control reads identically to the standalone widget.
fn paint_drag_arrows(ctx: &mut dyn DrawCtx, pill: Rect, scale: f64) {
    let visuals = ctx.visuals().clone();
    let mut arrow = visuals.accent;
    arrow.a = 0.45;
    ctx.set_fill_color(arrow);

    let mid = pill.y + pill.height * 0.5;
    let margin = 6.0 * scale;
    let tri_w = 5.0 * scale;
    let tri_half = 3.5 * scale;

    // Left-pointing triangle at the pill's left edge.
    let lx = pill.x + margin;
    ctx.begin_path();
    ctx.move_to(lx, mid);
    ctx.line_to(lx + tri_w, mid - tri_half);
    ctx.line_to(lx + tri_w, mid + tri_half);
    ctx.close_path();
    ctx.fill();

    // Right-pointing triangle at the pill's right edge.
    let rx = pill.x + pill.width - margin;
    ctx.begin_path();
    ctx.move_to(rx, mid);
    ctx.line_to(rx - tri_w, mid - tri_half);
    ctx.line_to(rx - tri_w, mid + tri_half);
    ctx.close_path();
    ctx.fill();
}

/// Shared value-text paint used by both slider and drag variants —
/// horizontally centred in `pill`, vertically centred using the
/// label-baseline convention.
fn paint_centred_value(ctx: &mut dyn DrawCtx, pill: Rect, n: f64, attrs: &NumberAttrs, scale: f64) {
    let s = format_number(n, Some(attrs));
    let visuals = ctx.visuals().clone();
    ctx.set_fill_color(visuals.text_color);
    ctx.set_font_size(11.0 * scale);
    let est_w = (s.len() as f64) * 6.5 * scale;
    let text_x = (pill.x + (pill.width - est_w) * 0.5).max(pill.x + 4.0 * scale);
    let text_y = pill.y + pill.height * 0.5 - 4.0 * scale;
    ctx.fill_text(&s, text_x, text_y);
}
