//! `ProgressBar` — a read-only horizontal progress indicator.
//!
//! Supports an optional loading animation (`animate`) that pulses the fill
//! brightness and sweeps a small spinner arc at the fill edge, mirroring
//! egui's `ProgressBar::animate`. The animation only runs while `value < 1.0`
//! and the bar is actually painted (i.e. visible), re-arming a ~60 fps wake via
//! [`request_draw_after`](crate::animation::request_draw_after) each frame so
//! the loop idles the moment the bar is culled or finishes.

use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

const BAR_H: f64 = 18.0;
const WIDGET_H: f64 = 24.0;

/// Linear interpolation between `a` and `b` by `t` (unclamped).
#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Inspector-visible properties of a [`ProgressBar`].
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[derive(Clone, Debug)]
pub struct ProgressBarProps {
    /// Progress in `[0.0, 1.0]`.
    pub value: f64,
    pub show_text: bool,
    pub font_size: f64,
    pub fill_color: Option<Color>,
    /// When `true`, play the loading animation while `value < 1.0`.
    pub animate: bool,
}

impl Default for ProgressBarProps {
    fn default() -> Self {
        Self {
            value: 0.0,
            show_text: true,
            font_size: 11.0,
            fill_color: None,
            animate: false,
        }
    }
}

/// A horizontal progress bar. `value` is in `[0.0, 1.0]`.
pub struct ProgressBar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    pub props: ProgressBarProps,
    font: Arc<Font>,
    /// When set, the animation runs while the bar is hovered (matches the egui
    /// gallery, which passes `response.hovered()` to `.animate`).
    animate_on_hover: bool,
    /// Live hover state, tracked so `animate_on_hover` can start/stop the loop.
    hovered: bool,
    /// Time origin for the pulse/spinner phase.
    anim_start: Instant,
}

impl ProgressBar {
    pub fn new(value: f64, font: Arc<Font>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            props: ProgressBarProps {
                value: value.clamp(0.0, 1.0),
                ..ProgressBarProps::default()
            },
            font,
            animate_on_hover: false,
            hovered: false,
            anim_start: Instant::now(),
        }
    }

    pub fn with_show_text(mut self, show: bool) -> Self {
        self.props.show_text = show;
        self
    }
    pub fn with_fill_color(mut self, color: Color) -> Self {
        self.props.fill_color = Some(color);
        self
    }

    /// Enable the loading animation. While set (and `value < 1.0`), the fill
    /// brightness pulses and a small spinner arc sweeps at the fill edge.
    /// Mirrors egui's `ProgressBar::animate`.
    pub fn with_animate(mut self, animate: bool) -> Self {
        self.props.animate = animate;
        self
    }
    /// Runtime setter for the animation flag (e.g. driven by app state).
    pub fn set_animate(&mut self, animate: bool) {
        self.props.animate = animate;
    }
    /// Animate only while the bar is hovered — the egui Widget Gallery
    /// behavior ("The progress bar can be animated!").
    pub fn with_animate_on_hover(mut self, on: bool) -> Self {
        self.animate_on_hover = on;
        self
    }

    /// Whether the animation should currently play: enabled (explicitly or via
    /// hover) and not yet complete.
    #[inline]
    fn animating(&self) -> bool {
        self.props.value < 1.0 && (self.props.animate || (self.animate_on_hover && self.hovered))
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }

    pub fn set_value(&mut self, v: f64) {
        self.props.value = v.clamp(0.0, 1.0);
    }

    pub fn value(&self) -> f64 {
        self.props.value
    }
}

impl Widget for ProgressBar {
    fn type_name(&self) -> &'static str {
        "ProgressBar"
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

    #[cfg(feature = "reflect")]
    fn as_reflect(&self) -> Option<&dyn bevy_reflect::Reflect> {
        Some(&self.props)
    }
    #[cfg(feature = "reflect")]
    fn as_reflect_mut(&mut self) -> Option<&mut dyn bevy_reflect::Reflect> {
        Some(&mut self.props)
    }

    fn margin(&self) -> Insets {
        self.base.margin
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn h_anchor(&self) -> HAnchor {
        self.base.h_anchor
    }
    fn v_anchor(&self) -> VAnchor {
        self.base.v_anchor
    }
    fn min_size(&self) -> Size {
        self.base.min_size
    }
    fn max_size(&self) -> Size {
        self.base.max_size
    }

    fn layout(&mut self, available: Size) -> Size {
        Size::new(available.width, WIDGET_H)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let bar_y = (h - BAR_H) * 0.5;
        let r = BAR_H * 0.5;

        // Track
        ctx.set_fill_color(v.track_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, bar_y, w, BAR_H, r);
        ctx.fill();

        let animating = self.animating();

        // Fill — use explicit fill_color if set, otherwise fall back to accent.
        // While animating, pulse the brightness like egui (cos-driven, 0.7..1.0).
        let base_fill = self.props.fill_color.unwrap_or(v.accent);
        let time = self.anim_start.elapsed().as_secs_f64();
        let fill_color = if animating {
            let factor = lerp(0.7, 1.0, time.cos().abs()) as f32;
            Color::rgba(
                base_fill.r * factor,
                base_fill.g * factor,
                base_fill.b * factor,
                base_fill.a,
            )
        } else {
            base_fill
        };
        let fill_w = (w * self.props.value).max(0.0);
        if fill_w >= 1.0 {
            ctx.set_fill_color(fill_color);
            ctx.begin_path();
            ctx.rounded_rect(0.0, bar_y, fill_w, BAR_H, r);
            ctx.fill();
        }

        // Spinner arc that sweeps at the leading edge of the fill, matching
        // egui's animated ProgressBar.
        if animating {
            let center_y = bar_y + BAR_H * 0.5;
            let half_h = BAR_H * 0.5;
            let circle_r = half_h - 2.0;
            let start_angle = time * std::f64::consts::TAU;
            let end_angle = start_angle + 240f64.to_radians() * time.sin();
            let n = 20;
            ctx.set_stroke_color(v.text_color);
            ctx.set_line_width(2.0);
            ctx.begin_path();
            for i in 0..n {
                let angle = lerp(start_angle, end_angle, i as f64 / n as f64);
                let (sin, cos) = angle.sin_cos();
                let px = fill_w - half_h + circle_r * cos;
                let py = center_y + circle_r * sin;
                if i == 0 {
                    ctx.move_to(px, py);
                } else {
                    ctx.line_to(px, py);
                }
            }
            ctx.stroke();
            // Re-arm ~60 fps without invalidating cached widgets: the bar is
            // uncached, so its next paint re-reads the phase and redraws.
            crate::animation::request_draw_after(Duration::from_millis(16));
        }

        // Percentage text centered over bar
        if self.props.show_text {
            let label = format!("{:.0}%", self.props.value * 100.0);
            ctx.set_font(Arc::clone(&self.font));
            ctx.set_font_size(self.props.font_size);
            // Text color: always use theme text contrasted against the bar.
            let mid = w * 0.5;
            let text_color = if fill_w > mid {
                Color::rgba(1.0, 1.0, 1.0, 0.9)
            } else {
                v.text_dim
            };
            ctx.set_fill_color(text_color);
            if let Some(m) = ctx.measure_text(&label) {
                let tx = (w - m.width) * 0.5;
                let ty = bar_y + BAR_H * 0.5 - (m.ascent - m.descent) * 0.5;
                ctx.fill_text(&label, tx, ty);
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        // Track hover so `animate_on_hover` can start/stop the loading loop.
        // Only matters when hover actually gates the animation.
        if let Event::MouseMove { pos } = event {
            if self.animate_on_hover {
                let was = self.hovered;
                self.hovered = self.hit_test(*pos);
                if was != self.hovered {
                    // Hover edge changes the bar's content (animation on/off),
                    // so invalidate; the paint pass re-arms the frame timer.
                    crate::animation::request_draw();
                }
            }
        }
        EventResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

    fn test_font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    #[test]
    fn not_animating_by_default() {
        let pb = ProgressBar::new(0.5, test_font());
        assert!(!pb.animating());
    }

    #[test]
    fn explicit_animate_runs_below_full() {
        let pb = ProgressBar::new(0.5, test_font()).with_animate(true);
        assert!(pb.animating());
    }

    #[test]
    fn animation_stops_when_complete() {
        // egui gates the animation on `progress < 1.0`.
        let pb = ProgressBar::new(1.0, test_font()).with_animate(true);
        assert!(!pb.animating());
    }

    #[test]
    fn hover_mode_only_animates_while_hovered() {
        let mut pb = ProgressBar::new(0.5, test_font()).with_animate_on_hover(true);
        assert!(!pb.animating(), "idle until hovered");
        pb.hovered = true;
        assert!(pb.animating(), "animates while hovered");
        pb.hovered = false;
        assert!(!pb.animating(), "stops when hover leaves");
    }
}
