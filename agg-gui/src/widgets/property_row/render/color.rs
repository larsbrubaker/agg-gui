//! Color editor renderer — color swatch with checker underlay for
//! transparency.

use crate::{Color, DrawCtx, Rect};

use super::super::value::RowValue;
use super::editor_pill_rect;

pub(crate) fn paint_editor(ctx: &mut dyn DrawCtx, editor_area: Rect, value: RowValue, scale: f64) {
    let swatch = editor_pill_rect(editor_area, scale);

    // Checker underlay reads through any transparent swatch.
    paint_checker(ctx, swatch, scale);

    if let RowValue::Color(c) = value {
        ctx.set_fill_color(Color::rgba(c[0], c[1], c[2], c[3]));
        ctx.begin_path();
        ctx.rounded_rect(swatch.x, swatch.y, swatch.width, swatch.height, 3.0 * scale);
        ctx.fill();
    }

    let visuals = ctx.visuals().clone();
    ctx.set_stroke_color(visuals.window_stroke);
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(swatch.x, swatch.y, swatch.width, swatch.height, 3.0 * scale);
    ctx.stroke();
}

fn paint_checker(ctx: &mut dyn DrawCtx, rect: Rect, scale: f64) {
    let cell = 4.0 * scale;
    let light = Color::rgba(0.92, 0.92, 0.93, 1.0);
    let dark = Color::rgba(0.78, 0.78, 0.80, 1.0);

    ctx.set_fill_color(light);
    ctx.begin_path();
    ctx.rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0 * scale);
    ctx.fill();

    ctx.set_fill_color(dark);
    let mut y = rect.y;
    let mut row = 0usize;
    while y < rect.y + rect.height {
        let mut x = rect.x + ((row % 2) as f64) * cell;
        while x < rect.x + rect.width {
            let w = cell.min(rect.x + rect.width - x);
            let h = cell.min(rect.y + rect.height - y);
            if w > 0.0 && h > 0.0 {
                ctx.begin_path();
                ctx.rounded_rect(x, y, w, h, 0.0);
                ctx.fill();
            }
            x += cell * 2.0;
        }
        y += cell;
        row += 1;
    }
}
