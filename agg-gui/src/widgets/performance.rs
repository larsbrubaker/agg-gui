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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::Label;

mod run_mode;
pub use run_mode::{shared_run_mode, RunMode, RunModeDesc, RunModeRow};

// ── Frame history (rolling sample buffer) ─────────────────────────────────────

/// Rolling buffer of recent frame times in milliseconds.  Apps push from
/// the main loop; widgets read for display.  Sized for ~1 second at
/// 60 fps (matches the egui reference and the prior `demo_ui` copy).
pub struct FrameHistory {
    times: Vec<f32>,
    head: usize,
    len: usize,
    /// Monotonic change counter bumped by every [`push`].  Widgets use
    /// this to know when the data changed since their last paint and
    /// can request exactly one redraw instead of polling forever.
    revision: u64,
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
            revision: 0,
        }
    }

    /// Append `frame_ms`, dropping the oldest sample once the buffer is full.
    pub fn push(&mut self, frame_ms: f32) {
        self.times[self.head] = frame_ms;
        self.head = (self.head + 1) % Self::CAP;
        if self.len < Self::CAP {
            self.len += 1;
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Incremented every time a frame sample is appended.
    pub fn revision(&self) -> u64 {
        self.revision
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
    /// Children stored so the framework's tree walk recurses into them
    /// (glyph caches, hover, hit-test).  Indices:
    ///   `mean_idx`            — "Mean CPU usage" label (always present)
    ///   `selector_idx..`      — optional "Mode" label + RunModeRow +
    ///                           RunModeDesc (only when a run-mode cell
    ///                           is wired via `with_run_mode_selector`)
    children: Vec<Box<dyn Widget>>,
    history: SharedFrameHistory,
    sparkline_height: f64,
    label_height: f64,
    padding: f64,
    show_background: bool,
    live_redraw: bool,
    redraw_on_history_change: bool,
    last_painted_revision: Cell<u64>,
    font: Arc<Font>,
    /// Layout offsets for the optional selector section.  Populated by
    /// [`Self::with_run_mode_selector`]; zero when no selector is shown.
    selector: Option<SelectorLayout>,
    run_mode: Option<Rc<Cell<RunMode>>>,
}

/// Layout constants for the optional Reactive/Continuous selector group.
/// Indices point into `PerformanceView::children`.
struct SelectorLayout {
    mode_label_idx: usize,
    mode_row_idx: usize,
    desc_idx: usize,
    mode_label_height: f64,
    row_height: f64,
    desc_height: f64,
    inner_gap: f64,
    /// Separator stroke between selector group and CPU readout.  Drawn
    /// directly in `paint()` rather than as a child widget — a plain
    /// 1-px line doesn't need glyph caching or hit-testing.
    separator_pad: f64,
}

impl PerformanceView {
    /// Build a new view bound to `history`.  `font` is used for the
    /// "Mean CPU usage" label and (if enabled) the run-mode selector
    /// labels.
    pub fn new(font: Arc<Font>, history: SharedFrameHistory) -> Self {
        let mut label =
            Label::new("Mean CPU usage: 0.00 ms / frame", Arc::clone(&font)).with_font_size(11.0);
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
            redraw_on_history_change: false,
            last_painted_revision: Cell::new(0),
            font,
            selector: None,
            run_mode: None,
        }
    }

    /// Mount a Reactive / Continuous selector at the top of the widget.
    /// The two buttons read and write through `run_mode`, and a dynamic
    /// description label below them mirrors the current mode (and shows
    /// FPS in Continuous mode).  The host's main loop is expected to
    /// read the same cell to decide whether to pump frames.
    pub fn with_run_mode_selector(mut self, run_mode: Rc<Cell<RunMode>>) -> Self {
        // Reuse the existing "Mean CPU usage" Label as child[0]; append
        // the selector widgets so the visible order (top-down in Y-up)
        // is: Mode label, button row, description, [separator], mean
        // label, sparkline.
        let mode_label = Label::new("Mode", Arc::clone(&self.font)).with_font_size(11.0);
        let mode_row = RunModeRow::new(Arc::clone(&self.font), Rc::clone(&run_mode));
        let desc = RunModeDesc::new(
            Arc::clone(&self.font),
            Rc::clone(&run_mode),
            Rc::clone(&self.history),
        );

        let mean_idx = 0;
        let _ = mean_idx; // reserved for clarity
        let mode_label_idx = self.children.len();
        self.children.push(Box::new(mode_label));
        let mode_row_idx = self.children.len();
        self.children.push(Box::new(mode_row));
        let desc_idx = self.children.len();
        self.children.push(Box::new(desc));

        self.selector = Some(SelectorLayout {
            mode_label_idx,
            mode_row_idx,
            desc_idx,
            mode_label_height: 16.0,
            row_height: RunModeRow::ROW_HEIGHT,
            desc_height: 18.0,
            inner_gap: 4.0,
            separator_pad: 6.0,
        });
        self.run_mode = Some(run_mode);
        self
    }

    /// Read the live run-mode cell, if a selector is wired.
    pub fn run_mode(&self) -> Option<Rc<Cell<RunMode>>> {
        self.run_mode.clone()
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

    /// When `true`, the view invalidates itself once for each new
    /// [`FrameHistory`] revision it has not painted yet.
    ///
    /// Off by default because the agg-gui demo's Backend panel pushes a
    /// frame-history sample after each paint; enabling this there would
    /// make Reactive mode behave like Continuous mode.  Opt in for a
    /// dedicated popup / overlay whose whole job is to keep the graph
    /// visually caught up with samples generated by unrelated UI draws.
    pub fn with_history_redraw(mut self, on: bool) -> Self {
        self.redraw_on_history_change = on;
        self
    }

    fn total_height(&self) -> f64 {
        let base = self.label_height + self.sparkline_height + self.padding * 3.0;
        match &self.selector {
            Some(s) => {
                base + s.mode_label_height
                    + s.row_height
                    + s.desc_height
                    + s.inner_gap * 3.0
                    + s.separator_pad * 2.0
            }
            None => base,
        }
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
        let inner_w = (w - self.padding * 2.0).max(1.0);

        // Selector group sits at the top in Y-up (high local Y), in
        // visual order: "Mode" label, RunModeRow, RunModeDesc.  When the
        // selector is absent we fall straight through to the mean-label
        // placement and the geometry matches the pre-selector layout
        // exactly.
        let mut cursor_top = h - self.padding;
        if let Some(s) = &self.selector {
            // "Mode" label.
            let row_top = cursor_top;
            let row_bottom = row_top - s.mode_label_height;
            let label_size =
                self.children[s.mode_label_idx].layout(Size::new(inner_w, s.mode_label_height));
            let label_y = row_bottom + (s.mode_label_height - label_size.height) * 0.5;
            self.children[s.mode_label_idx].set_bounds(Rect::new(
                self.padding,
                label_y,
                label_size.width,
                label_size.height,
            ));
            cursor_top = row_bottom - s.inner_gap;

            // RunModeRow — full-width segmented control.
            let row_bottom = cursor_top - s.row_height;
            self.children[s.mode_row_idx].layout(Size::new(inner_w, s.row_height));
            self.children[s.mode_row_idx].set_bounds(Rect::new(
                self.padding,
                row_bottom,
                inner_w,
                s.row_height,
            ));
            cursor_top = row_bottom - s.inner_gap;

            // Description.  Let the desc widget self-size for wrap.
            let desc_size = self.children[s.desc_idx].layout(Size::new(inner_w, s.desc_height));
            let desc_h = desc_size.height.max(s.desc_height);
            let desc_bottom = cursor_top - desc_h;
            self.children[s.desc_idx].set_bounds(Rect::new(
                self.padding,
                desc_bottom,
                inner_w,
                desc_h,
            ));
            cursor_top = desc_bottom - s.separator_pad * 2.0 - s.inner_gap;
        }

        // Mean-CPU label sits below the optional selector group, above
        // the sparkline.  Without a selector this lands at exactly the
        // original position (top of widget, padded).
        let mean_row_top = cursor_top;
        let mean_row_bottom = mean_row_top - self.label_height;
        let mean_size = self.children[0].layout(Size::new(inner_w, self.label_height));
        let mean_y = mean_row_bottom + (self.label_height - mean_size.height) * 0.5;
        self.children[0].set_bounds(Rect::new(
            self.padding,
            mean_y,
            mean_size.width,
            mean_size.height,
        ));

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
        let (mean, revision) = {
            let hist = self.history.borrow();
            (hist.mean_ms(), hist.revision())
        };
        let text = format!("Mean CPU usage: {mean:.2} ms / frame");
        self.children[0].set_label_text(&text);
        self.children[0].set_label_color(v.text_dim);

        // Faint horizontal separator between the selector group and the
        // mean-CPU readout, mirroring the divider in the backend panel.
        if let Some(s) = &self.selector {
            let desc_bottom = self.children[s.desc_idx].bounds().y;
            let sep_y = desc_bottom - s.separator_pad;
            if sep_y > self.padding {
                ctx.set_stroke_color(v.separator);
                ctx.set_line_width(1.0);
                ctx.begin_path();
                ctx.move_to(self.padding, sep_y);
                ctx.line_to(w - self.padding, sep_y);
                ctx.stroke();
            }
        }
        if let Some(s) = &self.selector {
            // Re-tint the "Mode" label each frame so a theme switch
            // doesn't leave stale text colour on screen.
            self.children[s.mode_label_idx].set_label_color(v.text_dim);
        }

        // Sparkline area sits below the mean label.  Pin it to the bottom
        // (Y-up: low local Y) so it stays at the foot of the widget
        // regardless of whether the selector takes up vertical space.
        let sx = self.padding;
        let sy = self.padding;
        let sw = (w - self.padding * 2.0).max(1.0);
        let sh = self.sparkline_height;
        paint_sparkline(ctx, &self.history, sx, sy, sw, sh);
        self.last_painted_revision.set(revision);
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn needs_draw(&self) -> bool {
        // Reactive run-mode wins, period.  Hosts that wire the
        // selector explicitly opt in to "the user gets to decide
        // whether we loop"; in Reactive that means the widget must
        // NOT claim redraws of its own — otherwise the shell's
        // per-paint sample push turns `with_history_redraw(true)`
        // into an infinite loop and AtomArtist (which defaults to
        // Reactive) ends up painting continuously despite the user
        // explicitly picking Reactive.  In Continuous the host loop
        // pumps every frame anyway, so the internal claims here are
        // redundant but harmless.
        if let Some(rm) = &self.run_mode {
            if rm.get() == RunMode::Reactive {
                return false;
            }
        }
        // Default: passive. The agg-gui demo pushes a sample after each
        // paint, so making revision changes dirty by default would
        // turn Reactive mode into an accidental continuous loop.
        //
        // Dedicated performance overlays can opt into revision-driven
        // invalidation with `with_history_redraw(true)`, which redraws
        // exactly once when a pushed sample has not yet been painted.
        self.live_redraw
            || (self.redraw_on_history_change
                && self.history.borrow().revision() != self.last_painted_revision.get())
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
    fn frame_history_revision_increments_on_push() {
        let mut h = FrameHistory::new();
        assert_eq!(h.revision(), 0);
        h.push(10.0);
        assert_eq!(h.revision(), 1);
        h.push(20.0);
        assert_eq!(h.revision(), 2);
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
