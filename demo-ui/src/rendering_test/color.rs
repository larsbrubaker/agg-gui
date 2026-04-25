//! Egui-style color/gamma rendering smoke tests for the Rendering Test view.
//!
//! This complements the agg-gui-specific pixel and blending diagnostics with a
//! compact subset of egui's `ColorTest`: opaque gamma gradients, alpha blending
//! on contrasting backgrounds, and an additive-style blue-over-red ramp.

use std::sync::Arc;

use agg_gui::{Color, DrawCtx, Event, EventResult, Font, Rect, Size, Widget};

const GRAD_W: usize = 256;
const GRAD_H: f64 = 18.0;
const ROW_H: f64 = 28.0;
const LABEL_X: f64 = 276.0;

pub(super) struct ColorTest {
    pub(super) bounds: Rect,
    pub(super) children: Vec<Box<dyn Widget>>,
    pub(super) font: Arc<Font>,
}

impl Widget for ColorTest {
    fn type_name(&self) -> &'static str {
        "ColorTest"
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
        let w = available.width.min(560.0);
        let h = ROW_H * 7.0 + 8.0;
        self.bounds = Rect::new(0.0, 0.0, w, h);
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let mut y = self.bounds.height - GRAD_H - 4.0;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(11.0);

        paint_solid_row(
            ctx,
            y,
            Color::rgb(1.0, 165.0 / 255.0, 0.0),
            "orange rgb(255, 165, 0)",
        );
        y -= ROW_H;
        paint_gradient_row(
            ctx,
            y,
            Color::white(),
            Color::rgb(1.0, 0.0, 0.0),
            Color::rgb(0.0, 1.0, 0.0),
            "gamma interpolation: red -> green",
        );
        y -= ROW_H;
        paint_gradient_row(
            ctx,
            y,
            Color::black(),
            Color::black(),
            Color::white(),
            "gamma interpolation: black -> white",
        );
        y -= ROW_H;
        paint_alpha_row(
            ctx,
            y,
            Color::white(),
            Color::rgba(0.0, 0.75, 0.0, 0.0),
            Color::rgba(0.0, 0.75, 0.0, 1.0),
            "alpha blend on white",
        );
        y -= ROW_H;
        paint_alpha_row(
            ctx,
            y,
            Color::black(),
            Color::rgba(1.0, 1.0, 1.0, 0.0),
            Color::rgba(1.0, 1.0, 1.0, 1.0),
            "alpha blend on black",
        );
        y -= ROW_H;
        paint_additive_row(ctx, y, "add blue over red");

        ctx.set_fill_color(v.text_dim);
        ctx.fill_text(
            "All rows should change smoothly without banding or dark seams.",
            0.0,
            8.0,
        );
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

fn paint_solid_row(ctx: &mut dyn DrawCtx, y: f64, color: Color, label: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.rect(0.0, y, GRAD_W as f64, GRAD_H);
    ctx.fill();
    paint_label(ctx, y, label);
}

fn paint_gradient_row(
    ctx: &mut dyn DrawCtx,
    y: f64,
    bg: Color,
    left: Color,
    right: Color,
    label: &str,
) {
    paint_bg(ctx, y, bg);
    for x in 0..GRAD_W {
        let t = x as f32 / (GRAD_W - 1) as f32;
        ctx.set_fill_color(gamma_lerp(left, right, t));
        ctx.begin_path();
        ctx.rect(x as f64, y, 1.0, GRAD_H);
        ctx.fill();
    }
    paint_label(ctx, y, label);
}

fn paint_alpha_row(
    ctx: &mut dyn DrawCtx,
    y: f64,
    bg: Color,
    left: Color,
    right: Color,
    label: &str,
) {
    paint_bg(ctx, y, bg);
    for x in 0..GRAD_W {
        let t = x as f32 / (GRAD_W - 1) as f32;
        ctx.set_fill_color(gamma_lerp(left, right, t));
        ctx.begin_path();
        ctx.rect(x as f64, y, 1.0, GRAD_H);
        ctx.fill();
    }
    paint_label(ctx, y, label);
}

fn paint_additive_row(ctx: &mut dyn DrawCtx, y: f64, label: &str) {
    paint_bg(ctx, y, Color::rgb(1.0, 0.0, 0.0));
    for x in 0..GRAD_W {
        let t = x as f32 / (GRAD_W - 1) as f32;
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, t));
        ctx.begin_path();
        ctx.rect(x as f64, y, 1.0, GRAD_H);
        ctx.fill();
    }
    paint_label(ctx, y, label);
}

fn paint_bg(ctx: &mut dyn DrawCtx, y: f64, bg: Color) {
    ctx.set_fill_color(bg);
    ctx.begin_path();
    ctx.rect(0.0, y, GRAD_W as f64, GRAD_H);
    ctx.fill();
}

fn paint_label(ctx: &mut dyn DrawCtx, y: f64, label: &str) {
    ctx.set_fill_color(ctx.visuals().text_color);
    ctx.fill_text(label, LABEL_X, y + 4.0);
}

fn gamma_lerp(left: Color, right: Color, t: f32) -> Color {
    Color::rgba(
        left.r + (right.r - left.r) * t,
        left.g + (right.g - left.g) * t,
        left.b + (right.b - left.b) * t,
        left.a + (right.a - left.a) * t,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_lerp_preserves_endpoints() {
        let left = Color::rgba(0.1, 0.2, 0.3, 0.4);
        let right = Color::rgba(0.9, 0.8, 0.7, 0.6);

        assert_eq!(gamma_lerp(left, right, 0.0), left);
        assert_eq!(gamma_lerp(left, right, 1.0), right);
    }

    #[test]
    fn gamma_lerp_interpolates_alpha() {
        let c = gamma_lerp(Color::transparent(), Color::white(), 0.25);
        assert_eq!(c, Color::rgba(0.25, 0.25, 0.25, 0.25));
    }
}
