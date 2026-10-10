use std::sync::Arc;

use agg_gui::{Color, DrawCtx, Event, EventResult, Font, Rect, Size, Widget};

// ---------------------------------------------------------------------------
// Blending / feathering test
// ---------------------------------------------------------------------------

/// 512 × 256 canvas split top (black bg) / bottom (white bg).
///
/// Each half:
/// - Left side: Bézier curves of 7 increasing stroke widths with width labels
/// - Right side: opacity-fade text labels at 8 levels, then font-size text samples
pub(super) struct BlendingTest {
    pub(super) bounds: Rect,
    pub(super) children: Vec<Box<dyn Widget>>,
    pub(super) font: Arc<Font>,
}

impl Widget for BlendingTest {
    fn type_name(&self) -> &'static str {
        "BlendingTest"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width.min(512.0);
        let h = 512.0_f64;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let half_h = h * 0.5;

        // Top half: black background, white content.
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(0.0, half_h, w, half_h);
        ctx.fill();
        paint_half(ctx, &self.font, 0.0, half_h, w, half_h, Color::white());

        // Bottom half: white background, black content.
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, half_h);
        ctx.fill();
        paint_half(ctx, &self.font, 0.0, 0.0, w, half_h, Color::black());
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Paint one half of the blending test, matching egui's `paint_fine_lines_and_text`:
/// - Left side:  corner-sweeping CubicBézier arcs (spiral inward) at 7 stroke widths
/// - Right side: three text columns (white / gray / black) at 8 opacity levels,
///   followed by font-size ramp samples
///
/// The arc rect starts at the left half of this half-panel, shrunk 16 px on every side.
/// Each iteration the visual top drops 24 px and the right edge retreats 24 px, producing
/// the characteristic nested-arc spiral seen in egui.  Y-up coordinate system throughout.
fn paint_half(
    ctx: &mut dyn DrawCtx,
    font: &Arc<Font>,
    ox: f64, // origin x (lower-left of this half in widget coords)
    oy: f64, // origin y
    w: f64,
    h: f64,
    color: Color,
) {
    ctx.set_font(Arc::clone(font));

    // ── Right side: three opacity columns + font-size ramp ───────────────
    // Columns: white / gray / black at decreasing opacities (egui has all three).
    let right_x = ox + w * 0.5 + 4.0;
    let col_w = (w * 0.5 - 8.0) / 3.0;
    let row_h = 20.0_f64;
    // Y-up: visually "top" = oy + h; rows step downward.
    let mut text_y = oy + h - row_h * 0.7;

    let opacities: &[f32] = &[1.00, 0.50, 0.25, 0.10, 0.05, 0.02, 0.01, 0.00];
    ctx.set_font_size(11.0);
    for &op in opacities {
        ctx.set_fill_color(Color::white().with_alpha(op));
        ctx.fill_text(&format!("{:.0}% white", 100.0 * op), right_x, text_y);
        ctx.set_fill_color(Color::rgb(0.5, 0.5, 0.5).with_alpha(op));
        ctx.fill_text(&format!("{:.0}% gray", 100.0 * op), right_x + col_w, text_y);
        ctx.set_fill_color(Color::black().with_alpha(op));
        ctx.fill_text(
            &format!("{:.0}% black", 100.0 * op),
            right_x + col_w * 2.0,
            text_y,
        );
        text_y -= row_h;
    }

    // Font-size ramp: drawn in the half's primary color.
    let font_sizes: &[f64] = &[6.0, 7.0, 8.0, 9.0, 10.0, 12.0, 14.0];
    ctx.set_fill_color(color);
    for &sz in font_sizes {
        ctx.set_font_size(sz);
        ctx.fill_text(
            &format!("{sz}px - The quick brown fox jumps over the lazy dog and runs away."),
            right_x,
            text_y,
        );
        text_y -= sz + 1.0;
    }

    // ── Left side: corner-sweeping CubicBézier arcs (egui pattern) ───────
    // Rect is the left half of this half-panel, shrunk 16 px on all sides.
    // In Y-up: visual top = high Y, visual bottom = low Y.
    let rect_left = ox + 16.0;
    let mut rect_right = ox + w * 0.5 - 16.0;
    let mut rect_top = oy + h - 16.0; // Y-up: visual top has large Y
    let rect_bottom = oy + 16.0;

    let widths: &[f64] = &[0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0];
    ctx.set_font_size(10.0);

    for &lw in widths {
        let center_y = (rect_top + rect_bottom) * 0.5;

        // Label at the visual top-left of the current rect.
        ctx.set_fill_color(color);
        ctx.fill_text(&format!("{lw}"), rect_left, rect_top);

        // CubicBézier sweeping from near left_top to right_top, then down to right_bottom.
        // Egui Y-down: left_top+vec2(16,0) → right_top → right_center → right_bottom.
        // Y-up translation: top=high Y, bottom=low Y — the shape is identical.
        ctx.set_stroke_color(color);
        ctx.set_line_width(lw);
        ctx.begin_path();
        ctx.move_to(rect_left + 16.0, rect_top);
        ctx.cubic_to(
            rect_right,
            rect_top, // CP1: right_top
            rect_right,
            center_y, // CP2: right_center
            rect_right,
            rect_bottom, // end:  right_bottom
        );
        ctx.stroke();

        // Shrink rect for next iteration:
        // egui Y-down min.y += 24 → visual top retreats → Y-up: rect_top decreases.
        rect_top -= 24.0;
        rect_right -= 24.0;
    }

    // ── Gradient bar: transparent → opaque ───────────────────────────────
    let left_x = ox + 16.0;
    let grad_y = oy + 10.0;
    ctx.set_fill_color(color);
    ctx.set_font_size(9.0);
    ctx.fill_text("transparent --> opaque", left_x, grad_y + 12.0);

    let grad_w = w * 0.5 - 24.0;
    let steps = 32_usize;
    let step_w = grad_w / steps as f64;
    for i in 0..steps {
        let alpha = i as f32 / steps as f32;
        ctx.set_fill_color(color.with_alpha(alpha));
        ctx.begin_path();
        ctx.rect(left_x + i as f64 * step_w, grad_y - 8.0, step_w, 8.0);
        ctx.fill();
    }
}
