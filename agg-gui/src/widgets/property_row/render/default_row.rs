//! Fallback editor renderer — value text in a pill. Used by
//! `EditorKind::Display` and any variant we haven't built a
//! dedicated renderer for yet.

use crate::{DrawCtx, Rect};

use super::super::value::RowValue;
use super::{editor_pill_rect, format_number, paint_pill_bg};

pub(crate) fn paint_editor(
    ctx: &mut dyn DrawCtx,
    editor_area: Rect,
    value: RowValue,
    scale: f64,
) {
    let pill = editor_pill_rect(editor_area, scale);
    paint_pill_bg(ctx, pill, scale);

    let value_text = match value {
        RowValue::Number(n) => format_number(n, None),
        RowValue::Bool(b) => b.to_string(),
        RowValue::Color(_) => String::new(),
        RowValue::Text(s) | RowValue::Display(s) => s.to_string(),
    };
    if value_text.is_empty() {
        return;
    }
    let visuals = ctx.visuals().clone();
    ctx.set_fill_color(visuals.text_color);
    ctx.set_font_size(11.0 * scale);
    let est_w = (value_text.len() as f64) * 6.5 * scale;
    let text_x = (pill.x + (pill.width - est_w) * 0.5).max(pill.x + 4.0 * scale);
    let text_y = pill.y + pill.height * 0.5 - 4.0 * scale;
    ctx.fill_text(&value_text, text_x, text_y);
}
