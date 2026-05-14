//! `PerformanceView` — Mean CPU usage label + frame-time sparkline.
//!
//! Apps wire a [`SharedFrameHistory`] from their main loop into the
//! widget's constructor, then push each completed frame's wall time via
//! [`FrameHistory::push`].  The widget renders the rolling mean as
//! "Mean CPU usage: X.XX ms / frame" plus a sparkline graph below — same
//! presentation used in the agg-gui demo's Backend panel and the egui
//! reference's `frame_history` widget.
//!
//! This is intentionally a minimal, self-contained widget (one Label
//! child for glyph caching, one direct paint pass for the sparkline) so
//! it drops into any container — a side panel, a popup window, a
//! collapsing header — without extra plumbing.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::Label;

// ── Frame history (rolling sample buffer) ─────────────────────────────────────

/// Rolling buffer of recent frame times in milliseconds.  Apps push from
/// the main loop; widgets read for display.  Sized for ~1 second at
/// 60 fps (matches the egui reference and the prior `demo_ui` copy).
pub struct FrameHistory {
    times: Vec<f32>,
    head: usize,
    len: usize,
}

impl FrameHistory {
    /// Number of samples retained.  Tuned for a 1-second window at the
    /// 60 fps target — short enough to surface a transient hitch on the
    /// graph, long enough that a single slow frame doesn't dominate the
    /// "Mean CPU usage" readout.
    pub const CAP: usize = 60;

    pub fn new() -> Self {
        Self {
            times: vec![0.0; Self::CAP],
            head: 0,
            len: 0,
        }
    }

    /// Append `frame_ms`, dropping the oldest sample once the buffer is full.
    pub fn push(&mut self, frame_ms: f32) {
        self.times[self.head] = frame_ms;
        self.head = (self.head + 1) % Self::CAP;
        if self.len < Self::CAP {
            self.len += 1;
        }
    }

    /// Average of all retained samples (0.0 when empty).
    pub fn mean_ms(&self) -> f32 {
        if self.len == 0 {
            return 0.0;
        }
        self.times[..self.len].iter().sum::<f32>() / self.len as f32
    }

    /// Convenience: 1000 / mean_ms (or 0.0 for an empty / zero buffer).
    pub fn fps(&self) -> f32 {
        let m = self.mean_ms();
        if m < 0.001 {
            0.0
        } else {
            1000.0 / m
        }
    }

    /// Number of valid samples currently held.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterate samples from oldest to newest (sparkline-friendly order).
    pub fn samples(&self) -> impl Iterator<Item = f32> + '_ {
        let cap = Self::CAP;
        (0..self.len).map(move |i| {
            let idx = (self.head + cap - self.len + i) % cap;
            self.times[idx]
        })
    }
}

impl Default for FrameHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared handle to a [`FrameHistory`] — passed to the widget at
/// construction and to the platform shell so it can push samples.
pub type SharedFrameHistory = Rc<RefCell<FrameHistory>>;

/// Convenience: heap-allocate a fresh shared history.  Equivalent to
/// `Rc::new(RefCell::new(FrameHistory::new()))`.
pub fn shared_frame_history() -> SharedFrameHistory {
    Rc::new(RefCell::new(FrameHistory::new()))
}

// ── PerformanceView widget ────────────────────────────────────────────────────

/// "Mean CPU usage" label stacked above a frame-time sparkline.
///
/// Composition (Y-up: top of widget = high local Y):
///
/// ```text
/// ┌────────────────────────────────────────┐ ← top
/// │ Mean CPU usage: 4.12 ms / frame        │   label_height
/// ├────────────────────────────────────────┤
/// │                                        │
/// │  (sparkline of the last N frame times) │   sparkline_height
/// │                                        │
/// └────────────────────────────────────────┘ ← bottom (y = 0)
/// ```
///
/// The horizontal orange line on the sparkline marks the 16.7 ms / 60 fps
/// reference budget — same convention as the egui reference panel.
pub struct PerformanceView {
    bounds: Rect,
    /// Always exactly one child: the `Label` displaying "Mean CPU
    /// usage".  Stored in `children` so the framework's tree walk
    /// recurses into it; `paint()` refreshes the text via
    /// `set_label_text` each frame and the Label's own glyph cache
    /// invalidates only on text changes.
    children: Vec<Box<dyn Widget>>,
    history: SharedFrameHistory,
    sparkline_height: f64,
    label_height: f64,
    padding: f64,
    show_background: bool,
    live_redraw: bool,
}

impl PerformanceView {
    /// Build a new view bound to `history`.  `font` is used for the
    /// "Mean CPU usage" label.
    pub fn new(font: Arc<Font>, history: SharedFrameHistory) -> Self {
        let mut label = Label::new("Mean CPU usage: 0.00 ms / frame", font).with_font_size(11.0);
        // Live counter — value changes every frame, so caching the
        // glyph bitmap to a backbuffer would invalidate every frame
        // anyway.  Direct rasterisation is cheaper here.
        label.buffered = false;
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(label)],
            history,
            sparkline_height: 56.0,
            label_height: 18.0,
            padding: 12.0,
            show_background: false,
            live_redraw: false,
        }
    }

    pub fn with_sparkline_height(mut self, h: f64) -> Self {
        self.sparkline_height = h.max(8.0);
        self
    }

    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p.max(0.0);
        self
    }

    /// Paint a panel-fill background behind the widget.  Off by default
    /// (lets the host pick — the demo's Backend panel already paints
    /// its own background; a popup window paints its own panel fill).
    pub fn with_background(mut self, on: bool) -> Self {
        self.show_background = on;
        self
    }

    /// When `true`, claim a redraw every frame so the rolling mean +
    /// sparkline always show live values.  Off by default — the demo's
    /// Backend panel relies on continuous-mode repaints (or unrelated
    /// dirty events) to refresh the readout, and a default-on flag
    /// would prevent the host from going idle in reactive mode.
    /// Opt in for popup-window hosts that exist specifically to show
    /// live performance numbers (Solitaire's Debug → Performance
    /// Window).
    pub fn with_live_redraw(mut self, on: bool) -> Self {
        self.live_redraw = on;
        self
    }

    fn total_height(&self) -> f64 {
        self.label_height + self.sparkline_height + self.padding * 3.0
    }
}

impl Widget for PerformanceView {
    fn type_name(&self) -> &'static str {
        "PerformanceView"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let w = available.width.max(1.0);
        let h = self.total_height();
        self.bounds = Rect::new(0.0, 0.0, w, h);
        // Label sits at the TOP of the widget (Y-up: high local Y).
        if let Some(child) = self.children.first_mut() {
            let inner_w = (w - self.padding * 2.0).max(1.0);
            let s = child.layout(Size::new(inner_w, self.label_height));
            // Y-up: top of widget = h.  Place label so its top edge sits
            // `padding` below the widget top, then centre vertically
            // within the `label_height` row.
            let row_top = h - self.padding;
            let row_bottom = row_top - self.label_height;
            let label_y = row_bottom + (self.label_height - s.height) * 0.5;
            child.set_bounds(Rect::new(self.padding, label_y, s.width, s.height));
        }
        Size::new(w, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Optional panel-fill background (off by default; hosts that
        // already draw a panel under us — Backend sidebar, Window — set
        // this to false).
        if self.show_background {
            ctx.set_fill_color(v.panel_fill);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, h);
            ctx.fill();
        }

        // Refresh the label text via the trait setters so the framework
        // tree walk paints the (potentially-new) glyph string.
        let mean = self.history.borrow().mean_ms();
        let text = format!("Mean CPU usage: {mean:.2} ms / frame");
        if let Some(child) = self.children.first_mut() {
            child.set_label_text(&text);
            child.set_label_color(v.text_dim);
        }

        // Sparkline area sits below the label row in Y-up:
        //   x: [padding .. w - padding]
        //   y: [padding .. padding + sparkline_height]
        // (label occupies Y above this strip; widget bottom = y=0.)
        let sx = self.padding;
        let sy = self.padding;
        let sw = (w - self.padding * 2.0).max(1.0);
        let sh = self.sparkline_height;
        paint_sparkline(ctx, &self.history, sx, sy, sw, sh);
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn needs_draw(&self) -> bool {
        // Default: defer to the host (matches the existing Backend
        // panel behaviour — readout updates whenever some other
        // widget dirties the frame).  When `live_redraw` is set the
        // widget claims a fresh frame so the rolling mean + sparkline
        // always reflect the latest sample, suitable for a popup
        // window opened specifically to inspect performance.
        self.live_redraw
    }
}

// ── Sparkline painting (free function, shared by hosts that want it) ──────────

/// Paint a frame-time sparkline at `(x, y, w, h)` in the active
/// `DrawCtx`'s coordinate space.  Reads from `history` for samples and
/// draws an orange 16.7 ms (60 fps) reference line.  Exposed in case a
/// caller wants the graph without the surrounding label / padding.
pub fn paint_sparkline(
    ctx: &mut dyn DrawCtx,
    history: &SharedFrameHistory,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) {
    let v = ctx.visuals();
    let hist = history.borrow();

    // Background.
    ctx.set_fill_color(v.track_bg);
    ctx.begin_path();
    ctx.rounded_rect(x, y, w, h, 4.0);
    ctx.fill();

    if hist.len() < 2 {
        return;
    }
    let samples: Vec<f32> = hist.samples().collect();
    // 60 fps reference (16.7 ms) is the floor for the Y axis range so a
    // run of fast frames doesn't auto-zoom and exaggerate noise.
    let max_ms = samples.iter().cloned().fold(0.1_f32, f32::max).max(16.7);

    // Line chart.  Mapping: smaller ms -> higher y (Y-up: top of strip).
    ctx.set_stroke_color(v.accent);
    ctx.set_line_width(1.5);
    ctx.begin_path();
    let n = samples.len();
    for (i, &ms) in samples.iter().enumerate() {
        let px = x + i as f64 / (n - 1) as f64 * w;
        let py = y + (1.0 - ms as f64 / max_ms as f64) * (h - 4.0) + 2.0;
        if i == 0 {
            ctx.move_to(px, py);
        } else {
            ctx.line_to(px, py);
        }
    }
    ctx.stroke();

    // 60 fps reference line.
    let ref_y = y + (1.0 - 16.7 / max_ms as f64) * (h - 4.0) + 2.0;
    if ref_y >= y + 2.0 && ref_y <= y + h - 2.0 {
        ctx.set_stroke_color(Color::rgba(1.0, 0.6, 0.0, 0.7));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, ref_y);
        ctx.line_to(x + w, ref_y);
        ctx.stroke();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_history_mean_with_no_samples_is_zero() {
        let h = FrameHistory::new();
        assert_eq!(h.mean_ms(), 0.0);
        assert_eq!(h.fps(), 0.0);
        assert!(h.is_empty());
    }

    #[test]
    fn frame_history_mean_averages_recent_samples() {
        let mut h = FrameHistory::new();
        h.push(10.0);
        h.push(20.0);
        h.push(30.0);
        assert!((h.mean_ms() - 20.0).abs() < 0.001);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn frame_history_wraps_at_capacity() {
        let mut h = FrameHistory::new();
        // Fill twice past capacity; only the most recent CAP samples
        // should contribute to the mean.
        for i in 0..(FrameHistory::CAP * 2) {
            h.push(i as f32);
        }
        assert_eq!(h.len(), FrameHistory::CAP);
        // The most recent CAP samples are CAP..2*CAP-1; their mean is
        // (CAP + (2*CAP - 1)) / 2.
        let cap = FrameHistory::CAP as f32;
        let expected = (cap + (2.0 * cap - 1.0)) / 2.0;
        assert!((h.mean_ms() - expected).abs() < 0.01);
    }

    #[test]
    fn frame_history_samples_yield_oldest_first() {
        let mut h = FrameHistory::new();
        h.push(1.0);
        h.push(2.0);
        h.push(3.0);
        let collected: Vec<f32> = h.samples().collect();
        assert_eq!(collected, vec![1.0, 2.0, 3.0]);
    }
}
