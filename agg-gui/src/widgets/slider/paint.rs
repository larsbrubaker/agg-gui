//! Painting for [`super::Slider`].
//!
//! Split out of `slider/mod.rs` to keep both files well under the project's
//! 800-line limit. These are inherent methods on `Slider`; the module core
//! (mapping, events, layout) lives in `mod.rs` and calls into here from its
//! `Widget::paint` implementation. Marked `pub(super)` so the parent module can
//! invoke them.

use super::*;

impl Slider {
    /// Paint the draggable handle (circle or rect) centered at `(cx, cy)`.
    pub(super) fn paint_thumb(&self, ctx: &mut dyn DrawCtx, cx: f64, cy: f64) {
        let v = ctx.visuals();
        let thumb_color = if self.dragging || self.focused {
            v.accent_pressed
        } else if self.hovered {
            v.accent_hovered
        } else {
            v.accent
        };
        match self.handle_shape {
            HandleShape::Circle => {
                ctx.set_fill_color(thumb_color);
                ctx.begin_path();
                ctx.circle(cx, cy, THUMB_R);
                ctx.fill();

                ctx.set_fill_color(v.widget_bg);
                ctx.begin_path();
                ctx.circle(cx, cy, THUMB_R - 2.5);
                ctx.fill();
            }
            HandleShape::Rect { aspect_ratio } => {
                // Long axis follows the slider orientation.
                let (hw, hh) = if self.is_vertical() {
                    (THUMB_R * 0.9, THUMB_R * aspect_ratio)
                } else {
                    (THUMB_R * aspect_ratio, THUMB_R * 0.9)
                };
                ctx.set_fill_color(thumb_color);
                ctx.begin_path();
                ctx.rounded_rect(cx - hw, cy - hh, hw * 2.0, hh * 2.0, 2.0);
                ctx.fill();
            }
        }
    }

    /// Draw the value label in the reserved right-hand strip, vertically
    /// centered on `cy`. Shared by both orientations.
    pub(super) fn paint_value_label(&mut self, ctx: &mut dyn DrawCtx, cy: f64) {
        if !self.props.show_value {
            return;
        }
        let text_color = ctx.visuals().text_color;
        self.value_label.set_color(text_color);
        let lb = self.value_label.bounds();
        let strip_left = self.track_right() + VALUE_GAP;
        let ly = cy - lb.height * 0.5;
        self.value_label
            .set_bounds(Rect::new(strip_left, ly, lb.width, lb.height));
        ctx.save();
        ctx.translate(strip_left, ly);
        paint_subtree(&mut self.value_label, ctx);
        ctx.restore();
    }

    pub(super) fn paint_horizontal(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let cy = self.bounds.height * 0.5;
        let track_right = self.track_right();
        let track_w = (track_right - THUMB_R).max(0.0);
        let tx = self.thumb_pos();

        // Rail background.
        ctx.set_fill_color(v.track_bg);
        ctx.begin_path();
        ctx.rounded_rect(THUMB_R, cy - TRACK_H * 0.5, track_w, TRACK_H, TRACK_H * 0.5);
        ctx.fill();

        // Trailing fill up to the thumb.
        if self.trailing_fill && tx > THUMB_R {
            ctx.set_fill_color(v.accent);
            ctx.begin_path();
            ctx.rounded_rect(THUMB_R, cy - TRACK_H * 0.5, tx - THUMB_R, TRACK_H, TRACK_H * 0.5);
            ctx.fill();
        }

        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.circle(tx, cy, THUMB_R + 3.0);
            ctx.stroke();
        }

        self.paint_thumb(ctx, tx, cy);
        self.paint_value_label(ctx, cy);
    }

    pub(super) fn paint_vertical(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let cx = THUMB_R; // rail column near the left edge
        let (p0, p1) = self.position_range(); // (bottom, top) in pixels
        let ty = self.thumb_pos();

        // Rail background (full height between the shrunk ends).
        let top = p1.min(p0);
        let rail_h = (p0 - p1).abs();
        ctx.set_fill_color(v.track_bg);
        ctx.begin_path();
        ctx.rounded_rect(cx - TRACK_H * 0.5, top, TRACK_H, rail_h, TRACK_H * 0.5);
        ctx.fill();

        // Trailing fill from the bottom up to the thumb.
        if self.trailing_fill && ty < p0 {
            ctx.set_fill_color(v.accent);
            ctx.begin_path();
            ctx.rounded_rect(cx - TRACK_H * 0.5, ty, TRACK_H, p0 - ty, TRACK_H * 0.5);
            ctx.fill();
        }

        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.circle(cx, ty, THUMB_R + 3.0);
            ctx.stroke();
        }

        self.paint_thumb(ctx, cx, ty);
        // Value label centered vertically on the whole widget.
        self.paint_value_label(ctx, self.bounds.height * 0.5);
    }
}
