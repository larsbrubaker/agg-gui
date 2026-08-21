//! `Spinner` — a small circular *indeterminate* progress indicator, the
//! macOS `ProgressView()` / `NSProgressIndicator(style: .spinning)` look.
//!
//! Twelve short radial spokes around a circle, the "head" spoke at full
//! ink and the ones behind it fading out; the head advances one spoke
//! at a time so the ring appears to rotate. Unlike [`ProgressBar`]
//! (which reports a known fraction) the spinner carries no value — show
//! it while something of unknown length is in flight (loading a score,
//! waiting on a device) and hide it (e.g. via `Conditional`) when done.
//!
//! Animation follows the `ProgressBar` pulse pattern: each `paint()`
//! reads the wall clock for the current step and re-arms a ~frame-rate
//! wake through [`request_draw_after`](crate::animation::request_draw_after),
//! so the host loop idles the moment the spinner is culled or hidden —
//! an invisible subtree is never painted and therefore never re-arms.
//!
//! [`ProgressBar`]: super::progress_bar::ProgressBar

use std::time::Duration;

use agg_rust::math_stroke::LineCap;
use web_time::Instant;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, Insets, VAnchor, WidgetBase};
use crate::widget::Widget;

/// Number of spokes around the ring.
const SPOKES: usize = 12;
/// Time per step of the head spoke — one revolution in 12 × 80 ms ≈ 1 s,
/// close to the AppKit cadence.
const STEP_MS: u64 = 80;
/// Lowest spoke alpha (the tail), as a fraction of the head's.
const TAIL_ALPHA: f32 = 0.12;

/// Diameter presets, mirroring SwiftUI `.controlSize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum SpinnerSize {
    /// 16 px — `.controlSize(.small)`, fits on a text line.
    Small,
    /// 32 px — `.controlSize(.regular)`.
    #[default]
    Regular,
}

impl SpinnerSize {
    /// Outer diameter of the ring in logical pixels.
    pub fn diameter(self) -> f64 {
        match self {
            SpinnerSize::Small => 16.0,
            SpinnerSize::Regular => 32.0,
        }
    }
}

/// An indeterminate circular activity indicator. See the module docs.
pub struct Spinner {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
    base: WidgetBase,
    size: SpinnerSize,
    /// Explicit spoke colour; `None` = theme `text_dim`.
    color: Option<Color>,
    /// Time origin for the step phase.
    anim_start: Instant,
}

impl Spinner {
    /// A regular-size spinner.
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            base: WidgetBase::new(),
            size: SpinnerSize::Regular,
            color: None,
            anim_start: Instant::now(),
        }
    }

    /// Shorthand for `Spinner::new().with_size(SpinnerSize::Small)`.
    pub fn small() -> Self {
        Self::new().with_size(SpinnerSize::Small)
    }

    pub fn with_size(mut self, size: SpinnerSize) -> Self {
        self.size = size;
        self
    }

    /// Override the spoke colour (default: theme `text_dim`).
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
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

    pub fn size(&self) -> SpinnerSize {
        self.size
    }

    /// Index of the head spoke for an elapsed time.
    fn step_for(elapsed: Duration) -> usize {
        ((elapsed.as_millis() / STEP_MS as u128) % SPOKES as u128) as usize
    }

    /// Alpha of spoke `i` when the head is at `head`: 1.0 at the head,
    /// falling linearly to [`TAIL_ALPHA`] for the spoke just ahead of it.
    fn spoke_alpha(i: usize, head: usize) -> f32 {
        let behind = (head + SPOKES - i) % SPOKES;
        let t = behind as f32 / (SPOKES - 1) as f32;
        1.0 - t * (1.0 - TAIL_ALPHA)
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Spinner {
    fn type_name(&self) -> &'static str {
        "Spinner"
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

    fn measure_min_height(&self, _available_w: f64) -> f64 {
        self.size.diameter()
    }

    fn layout(&mut self, _available: Size) -> Size {
        let d = self.size.diameter();
        let size = Size::new(d, d);
        self.bounds = Rect::new(self.bounds.x, self.bounds.y, d, d);
        size
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let d = self.size.diameter();
        // Centre in whatever bounds the parent gave us (a stretch anchor
        // may hand us more room than the ring needs).
        let cx = self.bounds.width * 0.5;
        let cy = self.bounds.height * 0.5;
        let outer_r = d * 0.5;
        let inner_r = d * 0.27;
        let line_w = (d * 0.085).max(1.0);
        let ink = self.color.unwrap_or_else(|| ctx.visuals().text_dim);
        let head = Self::step_for(self.anim_start.elapsed());

        ctx.set_line_width(line_w);
        ctx.set_line_cap(LineCap::Round);
        for i in 0..SPOKES {
            // Spoke 0 at 12 o'clock, advancing clockwise (Y-up space).
            let a =
                std::f64::consts::FRAC_PI_2 - (i as f64) * std::f64::consts::TAU / SPOKES as f64;
            let (dx, dy) = (a.cos(), a.sin());
            ctx.set_stroke_color(ink.with_alpha(ink.a * Self::spoke_alpha(i, head)));
            ctx.begin_path();
            ctx.move_to(cx + dx * inner_r, cy + dy * inner_r);
            ctx.line_to(
                cx + dx * (outer_r - line_w * 0.5),
                cy + dy * (outer_r - line_w * 0.5),
            );
            ctx.stroke();
        }

        // Re-arm for the next step. Only runs while actually painted, so a
        // hidden spinner does not keep the loop awake.
        crate::animation::request_draw_after_tagged(Duration::from_millis(STEP_MS), "spinner.step");
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("size", format!("{:?}", self.size))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::paint_recorder::PaintRecorder;

    #[test]
    fn sizes_match_the_control_size_presets() {
        let mut s = Spinner::small();
        assert_eq!(s.layout(Size::new(200.0, 200.0)), Size::new(16.0, 16.0));
        assert_eq!(s.measure_min_height(200.0), 16.0);
        let mut r = Spinner::new();
        assert_eq!(r.layout(Size::new(200.0, 200.0)), Size::new(32.0, 32.0));
        assert_eq!(r.size(), SpinnerSize::Regular);
    }

    #[test]
    fn head_advances_one_spoke_per_step_and_wraps() {
        assert_eq!(Spinner::step_for(Duration::from_millis(0)), 0);
        assert_eq!(Spinner::step_for(Duration::from_millis(STEP_MS)), 1);
        assert_eq!(
            Spinner::step_for(Duration::from_millis(STEP_MS * SPOKES as u64)),
            0,
            "a full revolution wraps to the first spoke"
        );
        assert_eq!(
            Spinner::step_for(Duration::from_millis(STEP_MS * (SPOKES as u64 + 3))),
            3
        );
    }

    #[test]
    fn spoke_alpha_is_full_at_the_head_and_fades_behind_it() {
        let head = 4;
        assert_eq!(Spinner::spoke_alpha(head, head), 1.0);
        let one_behind = Spinner::spoke_alpha(3, head);
        let two_behind = Spinner::spoke_alpha(2, head);
        assert!(one_behind < 1.0 && two_behind < one_behind);
        // The spoke just ahead of the head is the tail (oldest).
        let tail = Spinner::spoke_alpha(5, head);
        assert!((tail - TAIL_ALPHA).abs() < 1e-6);
    }

    #[test]
    fn paint_strokes_every_spoke_and_keeps_the_loop_awake() {
        let mut s = Spinner::new();
        s.layout(Size::new(100.0, 100.0));
        s.set_bounds(Rect::new(0.0, 0.0, 32.0, 32.0));
        crate::animation::clear_draw_request();
        let mut rec = PaintRecorder::new();
        s.paint(&mut rec);
        assert_eq!(rec.strokes.len(), SPOKES);
        assert!(rec.fills.is_empty(), "spokes only — no filled shapes");
        // Exactly one spoke is drawn at the peak alpha (the head); the
        // others fade behind it.
        let peak = rec
            .strokes
            .iter()
            .map(|op| op.color.a)
            .fold(0.0_f32, f32::max);
        let at_peak = rec
            .strokes
            .iter()
            .filter(|op| (op.color.a - peak).abs() < 1e-6)
            .count();
        assert_eq!(at_peak, 1);
        assert!(
            crate::animation::peek_next_draw_deadline().is_some(),
            "paint re-arms a scheduled draw for the next step"
        );
        crate::animation::clear_draw_request();
    }

    #[test]
    fn explicit_color_overrides_the_theme_ink() {
        let mut s = Spinner::small().with_color(Color::rgb(1.0, 0.0, 0.0));
        s.layout(Size::new(50.0, 50.0));
        s.set_bounds(Rect::new(0.0, 0.0, 16.0, 16.0));
        let mut rec = PaintRecorder::new();
        s.paint(&mut rec);
        assert!(rec
            .strokes
            .iter()
            .all(|op| op.color.r == 1.0 && op.color.g == 0.0 && op.color.b == 0.0));
        crate::animation::clear_draw_request();
    }
}
