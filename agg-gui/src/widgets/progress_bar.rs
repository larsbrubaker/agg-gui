//! `ProgressBar` — a read-only horizontal progress indicator.
//!
//! Supports an optional loading animation (`animate`) that pulses the fill
//! brightness, mirroring egui's `ProgressBar::animate`. There is deliberately
//! **no** spinner arc or dot at the fill head — a circle there reads as an
//! interactive handle, but the bar is read-only. The animation only runs while
//! `value < 1.0` and the bar is actually painted (i.e. visible), re-arming a
//! ~60 fps wake via [`request_draw_after`](crate::animation::request_draw_after)
//! each frame so the loop idles the moment the bar is culled or finishes.

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
    /// brightness pulses gently. Mirrors egui's `ProgressBar::animate`; there is
    /// no spinner arc or handle at the fill head.
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
        // While animating, gently pulse the whole fill's brightness (egui-like,
        // a smooth 0.78..1.0 sine). This is the ONLY animated element: no arc,
        // dot, or moving handle at the head that could read as interactive.
        let base_fill = self.props.fill_color.unwrap_or(v.accent);
        let time = self.anim_start.elapsed().as_secs_f64();
        let fill_color = if animating {
            // sin maps to 0..1 via (sin+1)/2, then into the 0.78..1.0 range.
            let pulse = (time * std::f64::consts::TAU * 0.6).sin() * 0.5 + 0.5;
            let factor = lerp(0.78, 1.0, pulse) as f32;
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

        // Keep the pulse alive: re-arm ~60 fps without invalidating cached
        // widgets. The bar is uncached, so its next paint re-reads the phase
        // and redraws. Gated on `animating` AND actually painting, so the loop
        // idles the instant the bar is culled or reaches 100%.
        if animating {
            crate::animation::request_draw_after_tagged(
                Duration::from_millis(16),
                "progress_bar.pulse",
            );
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
    use crate::draw_ctx::{FillRule, GlPaint, LinearGradientPaint, RadialGradientPaint};
    use crate::text::TextMetrics;
    use agg_rust::comp_op::CompOp;
    use agg_rust::math_stroke::{LineCap, LineJoin};
    use agg_rust::trans_affine::TransAffine;

    const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

    fn test_font() -> Arc<Font> {
        Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
    }

    /// Counts stroked paths and filled rounded rects so a test can assert the
    /// bar draws its track + fill but NO stroked spinner arc (the old handle
    /// that read as interactive). The arc was the widget's only `stroke()`, so
    /// `strokes == 0` pins its removal.
    struct PaintRecorder {
        transform: TransAffine,
        stack: Vec<TransAffine>,
        strokes: usize,
        filled_rounded_rects: usize,
        last_was_rounded_rect: bool,
    }

    impl PaintRecorder {
        fn new() -> Self {
            Self {
                transform: TransAffine::new(),
                stack: Vec::new(),
                strokes: 0,
                filled_rounded_rects: 0,
                last_was_rounded_rect: false,
            }
        }
    }

    impl DrawCtx for PaintRecorder {
        fn set_fill_color(&mut self, _color: Color) {}
        fn set_stroke_color(&mut self, _color: Color) {}
        fn set_fill_linear_gradient(&mut self, _gradient: LinearGradientPaint) {}
        fn set_fill_radial_gradient(&mut self, _gradient: RadialGradientPaint) {}
        fn set_line_width(&mut self, _w: f64) {}
        fn set_line_join(&mut self, _join: LineJoin) {}
        fn set_line_cap(&mut self, _cap: LineCap) {}
        fn set_miter_limit(&mut self, _limit: f64) {}
        fn set_line_dash(&mut self, _dashes: &[f64], _offset: f64) {}
        fn set_blend_mode(&mut self, _mode: CompOp) {}
        fn set_global_alpha(&mut self, _alpha: f64) {}
        fn set_fill_rule(&mut self, _rule: FillRule) {}
        fn set_font(&mut self, _font: Arc<Font>) {}
        fn set_font_size(&mut self, _size: f64) {}
        fn clip_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn reset_clip(&mut self) {}
        fn clear(&mut self, _color: Color) {}
        fn begin_path(&mut self) {
            self.last_was_rounded_rect = false;
        }
        fn move_to(&mut self, _x: f64, _y: f64) {
            self.last_was_rounded_rect = false;
        }
        fn line_to(&mut self, _x: f64, _y: f64) {
            self.last_was_rounded_rect = false;
        }
        fn cubic_to(&mut self, _cx1: f64, _cy1: f64, _cx2: f64, _cy2: f64, _x: f64, _y: f64) {}
        fn quad_to(&mut self, _cx: f64, _cy: f64, _x: f64, _y: f64) {}
        fn arc_to(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64, _ccw: bool) {}
        fn circle(&mut self, _cx: f64, _cy: f64, _r: f64) {
            self.last_was_rounded_rect = false;
        }
        fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {
            self.last_was_rounded_rect = false;
        }
        fn rounded_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64, _r: f64) {
            self.last_was_rounded_rect = true;
        }
        fn close_path(&mut self) {}
        fn fill(&mut self) {
            if self.last_was_rounded_rect {
                self.filled_rounded_rects += 1;
            }
        }
        fn stroke(&mut self) {
            self.strokes += 1;
        }
        fn fill_and_stroke(&mut self) {}
        fn draw_triangles_aa(&mut self, _vertices: &[[f32; 3]], _indices: &[u32], _color: Color) {}
        fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
        fn fill_text_gsv(&mut self, _text: &str, _x: f64, _y: f64, _size: f64) {}
        fn measure_text(&self, _text: &str) -> Option<TextMetrics> {
            Some(TextMetrics {
                width: 30.0,
                ascent: 8.0,
                descent: 2.0,
                line_height: 12.0,
            })
        }
        fn transform(&self) -> TransAffine {
            self.transform
        }
        fn save(&mut self) {
            self.stack.push(self.transform);
        }
        fn restore(&mut self) {
            if let Some(t) = self.stack.pop() {
                self.transform = t;
            }
        }
        fn translate(&mut self, tx: f64, ty: f64) {
            self.transform
                .premultiply(&TransAffine::new_translation(tx, ty));
        }
        fn rotate(&mut self, radians: f64) {
            self.transform
                .premultiply(&TransAffine::new_rotation(radians));
        }
        fn scale(&mut self, sx: f64, sy: f64) {
            self.transform
                .premultiply(&TransAffine::new_scaling(sx, sy));
        }
        fn set_transform(&mut self, m: TransAffine) {
            self.transform = m;
        }
        fn reset_transform(&mut self) {
            self.transform = TransAffine::new();
        }
        fn gl_paint(&mut self, _screen_rect: Rect, _painter: &mut dyn GlPaint) {}
    }

    fn paint_recorded(pb: &mut ProgressBar) -> PaintRecorder {
        pb.set_bounds(Rect::new(0.0, 0.0, 200.0, WIDGET_H));
        let mut ctx = PaintRecorder::new();
        pb.paint(&mut ctx);
        ctx
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

    /// Regression: while animating, the bar must draw NO stroked spinner arc /
    /// handle at the fill head — only the filled track and fill. A circle there
    /// reads as an interactive drag handle, but the bar is read-only.
    #[test]
    fn animating_paint_has_no_spinner_arc() {
        let mut pb = ProgressBar::new(0.5, test_font())
            .with_animate(true)
            .with_show_text(false);
        let rec = paint_recorded(&mut pb);
        assert_eq!(
            rec.strokes, 0,
            "animating bar must not stroke a spinner arc / handle"
        );
        assert_eq!(
            rec.filled_rounded_rects, 2,
            "track + fill are the only filled shapes"
        );
    }

    /// The static (non-animating) bar likewise draws just track + fill and no
    /// stroked element — the pulse is purely a fill-brightness effect.
    #[test]
    fn static_paint_has_no_stroked_element() {
        let mut pb = ProgressBar::new(0.5, test_font()).with_show_text(false);
        let rec = paint_recorded(&mut pb);
        assert_eq!(rec.strokes, 0);
        assert_eq!(rec.filled_rounded_rects, 2);
    }
}
