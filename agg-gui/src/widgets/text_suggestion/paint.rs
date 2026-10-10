//! Drawing of the caret suggestion list (agg-sharp
//! `TextSuggestionPopup.OnDraw`): a rounded panel, the highlighted row
//! tinted, each label on the left with its detail right aligned and dimmer,
//! and the highlighted row's description in a footer line. Called from the
//! owning `TextField`'s `paint_global_overlay`, in the field's local space;
//! the state it draws is `super::State`.

use std::sync::Arc;

use super::{State, TextSuggestionController, PAD, ROW_HEIGHT};
use crate::draw_ctx::DrawCtx;

/// C# `ThemeConfig.MenuRowRadius` (4 × DeviceScale).
const RADIUS: f64 = 4.0;

impl TextSuggestionController {
    /// Paints the open list at its placed rect (field-local).
    pub(crate) fn paint(&self, ctx: &mut dyn DrawCtx) {
        let st = self.state.borrow();
        if !st.is_open {
            return;
        }
        paint_state(&st, ctx);
    }
}

fn paint_state(st: &State, ctx: &mut dyn DrawCtx) {
    let Some((font, size)) = st.font.clone() else {
        return;
    };
    let v = ctx.visuals();
    let b = st.rect;
    ctx.set_fill_color(st.background.unwrap_or(v.bg_color));
    ctx.begin_path();
    ctx.rounded_rect(b.x, b.y, b.width, b.height, RADIUS);
    ctx.fill();
    ctx.set_stroke_color(v.text_color.with_alpha(60.0 / 255.0));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(b.x, b.y, b.width, b.height, RADIUS);
    ctx.stroke();

    ctx.set_font(Arc::clone(&font));
    ctx.set_font_size(size);
    let dim = v.text_color.with_alpha(150.0 / 255.0);
    let first = st.first_visible_row;
    for i in first..first + st.visible_row_count() {
        let Some(row) = st.row_bounds_local(i) else {
            continue;
        };
        if i == st.highlight {
            ctx.set_fill_color(
                st.highlight_color
                    .unwrap_or(v.accent.with_alpha(128.0 / 255.0)),
            );
            ctx.begin_path();
            ctx.rounded_rect(row.x, row.y, row.width, row.height, RADIUS);
            ctx.fill();
        }
        let s = &st.suggestions.suggestions[i];
        let baseline = row.y + (row.height - size) / 2.0;
        ctx.set_fill_color(v.text_color);
        ctx.fill_text(&s.label, row.x + PAD, baseline);
        if let Some(detail) = s.detail.as_deref().filter(|d| !d.is_empty()) {
            let detail_left = row.right() - PAD - st.measure(detail);
            // A detail that would run into the label is left out rather than
            // drawn over it.
            if detail_left > row.x + PAD + st.measure(&s.label) + PAD {
                ctx.set_fill_color(dim);
                ctx.fill_text(detail, detail_left, baseline);
            }
        }
    }

    if let Some(description) = st.highlighted_description() {
        let footer_top = b.y + PAD + ROW_HEIGHT;
        ctx.set_stroke_color(v.text_color.with_alpha(40.0 / 255.0));
        ctx.begin_path();
        ctx.move_to(b.x + PAD, footer_top);
        ctx.line_to(b.right() - PAD, footer_top);
        ctx.stroke();
        let baseline = b.y + PAD + (ROW_HEIGHT - size) / 2.0;
        ctx.set_fill_color(dim);
        ctx.fill_text(description, b.x + 2.0 * PAD, baseline);
    }
}
