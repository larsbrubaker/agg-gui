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
        let thumb_color = if let Some(c) = self.style.thumb {
            c
        } else if self.dragging || self.focused {
            v.accent_pressed
        } else if self.hovered {
            v.accent_hovered
        } else {
            v.accent
        };
        match self.handle_shape {
            HandleShape::Circle => {
                let r = self.thumb_radius();
                let ring = self.thumb_ring_width();
                if self.style.thumb_hollow {
                    // Outline only: a ring whose outer edge is the radius,
                    // centre left unpainted so the track shows through.
                    ctx.set_stroke_color(thumb_color);
                    ctx.set_line_width(ring);
                    ctx.begin_path();
                    ctx.circle(cx, cy, r - ring * 0.5);
                    ctx.stroke();
                } else {
                    ctx.set_fill_color(thumb_color);
                    ctx.begin_path();
                    ctx.circle(cx, cy, r);
                    ctx.fill();

                    ctx.set_fill_color(self.style.thumb_center.unwrap_or(v.widget_bg));
                    ctx.begin_path();
                    ctx.circle(cx, cy, r - ring);
                    ctx.fill();
                }
            }
            HandleShape::Rect { aspect_ratio } => {
                // Long axis follows the slider orientation.
                let r = self.thumb_radius();
                let (hw, hh) = if self.is_vertical() {
                    (r * 0.9, r * aspect_ratio)
                } else {
                    (r * aspect_ratio, r * 0.9)
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
        let thumb_r = self.thumb_radius();
        let track_h = self.track_height();
        let track_w = (track_right - thumb_r).max(0.0);
        let tx = self.thumb_pos();
        let radius = self.track_radius();
        let rail_y = self.track_start(cy);

        // Rail background.
        ctx.set_fill_color(self.style.track.unwrap_or(v.track_bg));
        ctx.begin_path();
        ctx.rounded_rect(thumb_r, rail_y, track_w, track_h, radius);
        ctx.fill();

        // Trailing fill up to the thumb.
        if self.trailing_fill && tx > thumb_r {
            ctx.set_fill_color(self.style.fill.unwrap_or(v.accent));
            ctx.begin_path();
            ctx.rounded_rect(thumb_r, rail_y, tx - thumb_r, track_h, radius);
            ctx.fill();
        }

        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.circle(tx, cy, thumb_r + 3.0);
            ctx.stroke();
        }

        self.paint_thumb(ctx, tx, cy);
        self.paint_value_label(ctx, cy);
    }

    pub(super) fn paint_vertical(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let thumb_r = self.thumb_radius();
        let track_h = self.track_height();
        let cx = thumb_r; // rail column near the left edge
        let (p0, p1) = self.position_range(); // (bottom, top) in pixels
        let ty = self.thumb_pos();
        let radius = self.track_radius();
        let rail_x = self.track_start(cx);

        // Rail background (full height between the shrunk ends).
        let top = p1.min(p0);
        let rail_h = (p0 - p1).abs();
        ctx.set_fill_color(self.style.track.unwrap_or(v.track_bg));
        ctx.begin_path();
        ctx.rounded_rect(rail_x, top, track_h, rail_h, radius);
        ctx.fill();

        // Trailing fill from the bottom up to the thumb.
        if self.trailing_fill && ty < p0 {
            ctx.set_fill_color(self.style.fill.unwrap_or(v.accent));
            ctx.begin_path();
            ctx.rounded_rect(rail_x, ty, track_h, p0 - ty, radius);
            ctx.fill();
        }

        if self.focused {
            ctx.set_stroke_color(v.accent_focus);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            ctx.circle(cx, ty, thumb_r + 3.0);
            ctx.stroke();
        }

        self.paint_thumb(ctx, cx, ty);
        // Value label centered vertically on the whole widget.
        self.paint_value_label(ctx, self.bounds.height * 0.5);
    }
}
