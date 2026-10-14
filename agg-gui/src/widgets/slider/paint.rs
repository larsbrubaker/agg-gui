//! Painting for [`super::Slider`].
//!
//! Split out of `slider/mod.rs` to keep both files well under the project's
//! 800-line limit. These are inherent methods on `Slider`; the module core
//! (mapping, events, layout) lives in `mod.rs` and calls into here from its
//! `Widget::paint` implementation. Marked `pub(super)` so the parent module can
//! invoke them.  A disabled slider's colours come from `state.rs`.  Both
//! orientations draw the rail between [`Slider::rail_ends`], which honours
//! `SliderStyle::full_length_track`.

use super::*;
use crate::color::Color;

impl Slider {
    /// The rail's ends along the slider axis, `(value-0 end, value-1 end)`.
    ///
    /// By default the rail runs between the thumb centre's end positions
    /// (`position_range`).  With [`SliderStyle::full_length_track`] it
    /// reaches out at each end by the inset `position_range` keeps the
    /// thumb in by (the thumb radius horizontally, the handle's extent
    /// vertically), so the thumb sits flush with the rail's ends at the
    /// minimum and maximum, as agg-sharp's `SlideView` draws it.
    pub(super) fn rail_ends(&self) -> (f64, f64) {
        let (p0, p1) = self.position_range();
        if !self.style.full_length_track {
            return (p0, p1);
        }
        let inset = if self.is_vertical() {
            self.handle_extent()
        } else {
            self.thumb_radius()
        };
        let dir = if p1 < p0 { -1.0 } else { 1.0 };
        (p0 - dir * inset, p1 + dir * inset)
    }

    /// The (trailing fill, rail) colours: the style's or the theme's, or
    /// their disabled greys (`state.rs`) while the slider is disabled.
    pub(super) fn paint_colors(&self, v: &crate::theme::Visuals) -> (Color, Color) {
        let track = self.style.track.unwrap_or(v.track_bg);
        if self.enabled() {
            (self.style.fill.unwrap_or(v.accent), track)
        } else {
            Self::disabled_colors(track)
        }
    }

    /// Paint the draggable handle (circle or rect) centered at `(cx, cy)`.
    pub(super) fn paint_thumb(&self, ctx: &mut dyn DrawCtx, cx: f64, cy: f64) {
        let v = ctx.visuals();
        let thumb_color = if !self.enabled() {
            self.paint_colors(&v).0
        } else if let Some(c) = self.style.thumb {
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
        let thumb_r = self.thumb_radius();
        let track_h = self.track_height();
        let (rail_left, rail_right) = self.rail_ends();
        let track_w = (rail_right - rail_left).max(0.0);
        let tx = self.thumb_pos();
        let radius = self.track_radius();
        let rail_y = self.track_start(cy);

        // Rail background.
        ctx.set_fill_color(self.paint_colors(&v).1);
        ctx.begin_path();
        ctx.rounded_rect(rail_left, rail_y, track_w, track_h, radius);
        ctx.fill();

        // Trailing fill from the rail's start up to the thumb.
        if self.trailing_fill && tx > rail_left {
            ctx.set_fill_color(self.paint_colors(&v).0);
            ctx.begin_path();
            ctx.rounded_rect(rail_left, rail_y, tx - rail_left, track_h, radius);
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
        let (p0, p1) = self.rail_ends(); // (value-0 end, value-1 end)
        let ty = self.thumb_pos();
        let radius = self.track_radius();
        let rail_x = self.track_start(cx);

        // Rail background between its ends.
        let top = p1.min(p0);
        let rail_h = (p0 - p1).abs();
        ctx.set_fill_color(self.paint_colors(&v).1);
        ctx.begin_path();
        ctx.rounded_rect(rail_x, top, track_h, rail_h, radius);
        ctx.fill();

        // Trailing fill from the rail's value-0 end up to the thumb.
        if self.trailing_fill && ty < p0 {
            ctx.set_fill_color(self.paint_colors(&v).0);
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
