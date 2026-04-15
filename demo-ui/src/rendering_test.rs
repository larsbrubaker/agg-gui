//! Rendering test tab — a full-canvas visual correctness test.
//!
//! Shows color accuracy, alpha compositing, stroke quality, shape primitives,
//! and text rasterization so that backend rendering can be compared side-by-side.
//! This tab is selected via the "Rendering test" app tab in the top bar.

use std::sync::Arc;

use agg_gui::{
    Color, DrawCtx, Event, EventResult,
    Font, Rect, Size, Widget,
};

// ── RenderingTestView ─────────────────────────────────────────────────────────

/// Full-canvas rendering correctness test.  Draws directly with `DrawCtx`
/// primitives, partitioned into labeled sections.
pub struct RenderingTestView {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    font:     Arc<Font>,
}

impl RenderingTestView {
    pub fn new(font: Arc<Font>) -> Self {
        Self { bounds: Rect::default(), children: Vec::new(), font }
    }
}

// ── Section helpers ────────────────────────────────────────────────────────────

/// Draw a small section heading at `(x, y)` (Y-up baseline).
fn draw_heading(ctx: &mut dyn DrawCtx, font: &Arc<Font>, x: f64, y: f64, text: &str) {
    ctx.set_font(Arc::clone(font));
    ctx.set_font_size(11.0);
    ctx.set_fill_color(ctx.visuals().text_dim);
    ctx.fill_text(text, x, y);
}

impl Widget for RenderingTestView {
    fn type_name(&self) -> &'static str { "RenderingTestView" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v  = ctx.visuals();
        let w  = self.bounds.width;
        let h  = self.bounds.height;

        // Canvas background.
        ctx.set_fill_color(v.bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();

        // Layout constants.
        let pad   = 20.0_f64;
        let col_w = (w - pad * 2.0) / 3.0;

        // ── Column 1 — Color & Alpha ──────────────────────────────────────────
        {
            let col_x = pad;
            let mut y  = h - pad - 14.0; // Y-up: start near top

            draw_heading(ctx, &self.font, col_x, y, "Solid colors");
            y -= 18.0;

            let hues: &[(f32, f32, f32, &str)] = &[
                (0.88, 0.22, 0.18, "Red"),
                (0.96, 0.60, 0.08, "Orange"),
                (0.88, 0.82, 0.12, "Yellow"),
                (0.18, 0.72, 0.32, "Green"),
                (0.10, 0.50, 0.90, "Blue"),
                (0.60, 0.25, 0.88, "Violet"),
            ];

            let swatch_w = (col_w - 4.0) / hues.len() as f64;
            let swatch_h = 22.0_f64;

            for (i, &(r, g, b, _name)) in hues.iter().enumerate() {
                let sx = col_x + i as f64 * swatch_w;
                ctx.set_fill_color(Color::rgb(r, g, b));
                ctx.begin_path();
                ctx.rounded_rect(sx, y - swatch_h, swatch_w - 2.0, swatch_h, 3.0);
                ctx.fill();
            }
            y -= swatch_h + 14.0;

            // Alpha ramp for accent color.
            draw_heading(ctx, &self.font, col_x, y, "Alpha ramp (accent)");
            y -= 18.0;

            let steps = 8_usize;
            let step_w = (col_w - 4.0) / steps as f64;
            for i in 0..steps {
                let alpha = (i + 1) as f32 / steps as f32;
                let a = v.accent;
                ctx.set_fill_color(Color::rgba(a.r, a.g, a.b, alpha));
                ctx.begin_path();
                ctx.rounded_rect(
                    col_x + i as f64 * step_w, y - swatch_h,
                    step_w - 2.0, swatch_h, 3.0,
                );
                ctx.fill();
            }
            y -= swatch_h + 14.0;

            // Grayscale ramp.
            draw_heading(ctx, &self.font, col_x, y, "Grayscale ramp");
            y -= 18.0;

            for i in 0..steps {
                let t = i as f32 / (steps - 1) as f32;
                ctx.set_fill_color(Color::rgb(t, t, t));
                ctx.begin_path();
                ctx.rounded_rect(
                    col_x + i as f64 * step_w, y - swatch_h,
                    step_w - 2.0, swatch_h, 3.0,
                );
                ctx.fill();
            }
            y -= swatch_h + 14.0;

            // Alpha compositing over bg.
            draw_heading(ctx, &self.font, col_x, y, "Compositing (over bg)");
            y -= 18.0;

            let colors: &[(f32, f32, f32)] = &[
                (0.88, 0.22, 0.18),
                (0.18, 0.72, 0.32),
                (0.10, 0.50, 0.90),
            ];
            let block_w = (col_w - 4.0) / colors.len() as f64;
            for (i, &(r, g, b)) in colors.iter().enumerate() {
                let bx = col_x + i as f64 * block_w;
                // Checkerboard-like background: alternating bg.
                ctx.set_fill_color(if i % 2 == 0 { v.widget_bg } else { v.panel_fill });
                ctx.begin_path();
                ctx.rect(bx, y - swatch_h * 1.5, block_w - 2.0, swatch_h * 1.5);
                ctx.fill();
                // Semi-transparent overlay.
                ctx.set_fill_color(Color::rgba(r, g, b, 0.50));
                ctx.begin_path();
                ctx.rect(bx, y - swatch_h * 1.5, block_w - 2.0, swatch_h * 1.5);
                ctx.fill();
            }
        }

        // ── Column 2 — Stroke & Shape tests ──────────────────────────────────
        {
            let col_x = pad + col_w;
            let mut y  = h - pad - 14.0;

            draw_heading(ctx, &self.font, col_x, y, "Stroke widths");
            y -= 18.0;

            let widths = [0.5_f64, 1.0, 1.5, 2.0, 3.0, 5.0];
            let line_h = 14.0_f64;
            for (i, &lw) in widths.iter().enumerate() {
                let ly = y - i as f64 * line_h - line_h * 0.5;
                ctx.set_stroke_color(v.text_color);
                ctx.set_line_width(lw);
                ctx.begin_path();
                ctx.move_to(col_x, ly);
                ctx.line_to(col_x + col_w - 4.0, ly);
                ctx.stroke();

                // Label on the right.
                ctx.set_font(Arc::clone(&self.font));
                ctx.set_font_size(9.5);
                ctx.set_fill_color(v.text_dim);
                ctx.fill_text(&format!("{lw}px"), col_x + col_w - 38.0, ly + 4.0);
            }
            y -= widths.len() as f64 * line_h + 14.0;

            draw_heading(ctx, &self.font, col_x, y, "Shape primitives");
            y -= 18.0;

            let shape_h = 60.0_f64;
            let shape_w = (col_w - 4.0) / 3.0;
            let fill_c  = Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.35);
            let stk_c   = v.accent;

            // Rectangle.
            ctx.set_fill_color(fill_c);
            ctx.begin_path();
            ctx.rect(col_x + 2.0, y - shape_h + 4.0, shape_w - 8.0, shape_h - 8.0);
            ctx.fill();
            ctx.set_stroke_color(stk_c);
            ctx.set_line_width(1.5);
            ctx.begin_path();
            ctx.rect(col_x + 2.0, y - shape_h + 4.0, shape_w - 8.0, shape_h - 8.0);
            ctx.stroke();

            // Rounded rect.
            ctx.set_fill_color(fill_c);
            ctx.begin_path();
            ctx.rounded_rect(col_x + shape_w + 2.0, y - shape_h + 4.0, shape_w - 8.0, shape_h - 8.0, 10.0);
            ctx.fill();
            ctx.set_stroke_color(stk_c);
            ctx.begin_path();
            ctx.rounded_rect(col_x + shape_w + 2.0, y - shape_h + 4.0, shape_w - 8.0, shape_h - 8.0, 10.0);
            ctx.stroke();

            // Circle.
            let cr = (shape_h - 8.0) * 0.5;
            let cx = col_x + shape_w * 2.0 + cr + 2.0;
            let cy = y - cr - 4.0;
            ctx.set_fill_color(fill_c);
            ctx.begin_path();
            ctx.circle(cx, cy, cr);
            ctx.fill();
            ctx.set_stroke_color(stk_c);
            ctx.begin_path();
            ctx.circle(cx, cy, cr);
            ctx.stroke();

            y -= shape_h + 14.0;

            draw_heading(ctx, &self.font, col_x, y, "Bézier curves");
            y -= 18.0;

            let bez_h = 70.0_f64;
            // Draw a cubic bezier.
            ctx.set_stroke_color(Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.80));
            ctx.set_line_width(2.0);
            ctx.begin_path();
            let bx = col_x + 4.0;
            let bw = col_w - 8.0;
            // Cubic: S-curve.
            ctx.move_to(bx, y - bez_h * 0.10);
            ctx.cubic_to(
                bx + bw * 0.25, y - bez_h * 0.90,
                bx + bw * 0.75, y - bez_h * 0.10,
                bx + bw,        y - bez_h * 0.90,
            );
            ctx.stroke();

            // Control point dots.
            ctx.set_fill_color(Color::rgba(1.0, 0.5, 0.1, 0.80));
            for &(px, py) in &[
                (bx, y - bez_h * 0.10),
                (bx + bw * 0.25, y - bez_h * 0.90),
                (bx + bw * 0.75, y - bez_h * 0.10),
                (bx + bw, y - bez_h * 0.90),
            ] {
                ctx.begin_path();
                ctx.circle(px, py, 3.5);
                ctx.fill();
            }
            // Control lines.
            ctx.set_stroke_color(Color::rgba(1.0, 0.5, 0.1, 0.35));
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(bx, y - bez_h * 0.10);
            ctx.line_to(bx + bw * 0.25, y - bez_h * 0.90);
            ctx.move_to(bx + bw, y - bez_h * 0.90);
            ctx.line_to(bx + bw * 0.75, y - bez_h * 0.10);
            ctx.stroke();
        }

        // ── Column 3 — Text rendering quality ────────────────────────────────
        {
            let col_x = pad + col_w * 2.0;
            let mut y  = h - pad - 14.0;

            draw_heading(ctx, &self.font, col_x, y, "Text sizes");
            y -= 18.0;

            let sizes: &[(f64, &str)] = &[
                (8.0,  "8px — tiny text"),
                (10.0, "10px — small"),
                (12.0, "12px — body"),
                (14.0, "14px — large body"),
                (18.0, "18px — heading"),
                (24.0, "24px — display"),
            ];

            ctx.set_font(Arc::clone(&self.font));
            for &(sz, text) in sizes {
                ctx.set_font_size(sz);
                ctx.set_fill_color(v.text_color);
                ctx.fill_text(text, col_x, y);
                y -= sz * 1.6;
            }

            y -= 8.0;
            draw_heading(ctx, &self.font, col_x, y, "Sub-pixel samples");
            y -= 18.0;

            // Repeated short words to stress sub-pixel hinting.
            ctx.set_font_size(12.0);
            ctx.set_fill_color(v.text_color);
            let sample = "Hello World  |  Lorem ipsum";
            for i in 0..4 {
                ctx.fill_text(sample, col_x + i as f64 * 0.25, y - i as f64 * 16.0);
            }
            y -= 4.0 * 16.0 + 16.0;

            draw_heading(ctx, &self.font, col_x, y, "Line rendering");
            y -= 18.0;

            // Horizontal, vertical, diagonal lines at 1px.
            ctx.set_stroke_color(v.text_color);
            ctx.set_line_width(1.0);

            let lx = col_x;
            let lw = col_w - 8.0;

            // Horizontal.
            ctx.begin_path();
            ctx.move_to(lx, y - 10.0);
            ctx.line_to(lx + lw, y - 10.0);
            ctx.stroke();

            // Vertical.
            ctx.begin_path();
            ctx.move_to(lx + lw * 0.25, y - 25.0);
            ctx.line_to(lx + lw * 0.25, y);
            ctx.stroke();

            // 45° diagonal.
            ctx.begin_path();
            ctx.move_to(lx + lw * 0.4, y);
            ctx.line_to(lx + lw * 0.4 + 30.0, y - 30.0);
            ctx.stroke();

            // Dashed line (manual dashes).
            ctx.set_stroke_color(Color::rgba(v.accent.r, v.accent.g, v.accent.b, 0.80));
            ctx.set_line_width(1.5);
            let dash_y = y - 40.0;
            let mut dx = lx;
            while dx < lx + lw {
                ctx.begin_path();
                ctx.move_to(dx, dash_y);
                ctx.line_to((dx + 6.0).min(lx + lw), dash_y);
                ctx.stroke();
                dx += 10.0;
            }
        }

        // ── Centered title ────────────────────────────────────────────────────
        {
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(10.5);
            ctx.set_fill_color(v.text_dim);
            let title = "Rendering Test — agg-gui visual correctness";
            if let Some(m) = ctx.measure_text(title) {
                ctx.fill_text(title, (w - m.width) * 0.5, 16.0);
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}
