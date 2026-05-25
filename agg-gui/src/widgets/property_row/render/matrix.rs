//! Matrix editor renderer — button-style pill showing the matrix's
//! current state ("Identity" / "Matrix" / …).

use crate::{DrawCtx, Rect};

use super::super::value::RowValue;
use super::{editor_pill_rect, paint_pill_bg};

pub(crate) fn paint_editor(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    scale: f64,
) {
    let pill = editor_pill_rect(editor_area, scale);
    paint_pill_bg(ctx, pill, scale);

    let text = match value {
        RowValue::Display(s) => s,
        RowValue::Text(s) => s,
        _ => "Matrix",
    };
    let visuals = ctx.visuals().clone();
    ctx.set_fill_color(visuals.text_color);
    ctx.set_font_size(11.0 * scale);
    let est_w = (text.len() as f64) * 6.5 * scale;
    let text_x = (pill.x + (pill.width - est_w) * 0.5).max(pill.x + 4.0 * scale);
    let text_y = pill.y + pill.height * 0.5 - 4.0 * scale;
    ctx.fill_text(text, text_x, text_y);
}

/// Hit-rect for the matrix pill, for hosts wiring click → open
/// transform popup.
pub fn hit_rect(editor_area: Rect, scale: f64) -> Rect {
    editor_pill_rect(editor_area, scale)
}
