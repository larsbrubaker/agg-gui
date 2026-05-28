//! Read-only string editor renderer — wrapped paragraph text inside
//! the editor area.
//!
//! When the row's label is empty (MatterCAD's `[DisplayName("")]`
//! combo), callers should also pass an editor_area covering the full
//! row width via [`paint_row`](super::paint_row) with `label=""`.
//! That route ends up here with the full row area to wrap text in.

use crate::{DrawCtx, Rect};

use super::super::value::RowValue;

pub(crate) fn paint_editor(ctx: &mut dyn DrawCtx, editor_area: Rect, value: RowValue, scale: f64) {
    let text = match value {
        RowValue::Text(s) | RowValue::Display(s) => s,
        _ => return,
    };
    let visuals = ctx.visuals().clone();
    ctx.set_fill_color(visuals.text_color);
    ctx.set_font_size(11.0 * scale);

    let glyph_w = 6.5 * scale;
    let line_h = 14.0 * scale;
    let max_chars = ((editor_area.width / glyph_w).floor() as usize).max(1);

    // First baseline sits ~10·scale below the area top so a single
    // line reads centred-ish in a typical row; later lines step down
    // by `line_h`.
    let mut y = editor_area.y + 10.0 * scale;
    let mut current = String::new();
    for word in text.split_whitespace() {
        let extra = if current.is_empty() { 0 } else { 1 };
        if current.chars().count() + extra + word.chars().count() > max_chars {
            ctx.fill_text(&current, editor_area.x, y);
            y += line_h;
            current.clear();
            if y > editor_area.y + editor_area.height {
                return;
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        ctx.fill_text(&current, editor_area.x, y);
    }
}
