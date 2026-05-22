//! Reactive / Continuous run-mode plumbing for `PerformanceView`.
//!
//! Splits the `RunMode` enum and the two helper widgets out of the main
//! `performance.rs` file purely for the 800-line per-file cap.  Everything
//! here is re-exported from `crate::widgets::performance` so callers (and
//! `agg_gui::widgets`) keep their existing import paths.
//!
//! Wiring:
//!   * `RunMode`            — host loop policy (Reactive | Continuous).
//!   * `shared_run_mode`    — convenience to build the `Rc<Cell<RunMode>>`
//!                            handle that the selector reads / writes.
//!   * `RunModeRow`         — two-button segmented control.
//!   * `RunModeDesc`        — dynamic description label (FPS in Continuous).
//!
//! The host's main loop is expected to read the same `Rc<Cell<RunMode>>`
//! to decide whether to pump frames; the widgets themselves only update the
//! cell on click.  Reactive ≠ "stops the perf graph"; whether the graph keeps
//! updating depends on `PerformanceView::with_history_redraw` and whatever
//! the host's loop is doing.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::animation;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::{Button, Label};

use super::SharedFrameHistory;

// ── RunMode enum + helper ─────────────────────────────────────────────────────

/// How the host's event loop drives repaints.
///
/// `Reactive` is the agg-gui default: paint only when widgets request a draw
/// (input, animation, or an explicit invalidation).  `Continuous` keeps the
/// loop spinning every frame — useful for hosts that want a live perf graph
/// or that drive a real-time simulation regardless of input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunMode {
    Reactive,
    Continuous,
}

/// Convenience: wrap a `RunMode` in the `Rc<Cell<…>>` plumbing that
/// [`super::PerformanceView::with_run_mode_selector`] expects.
pub fn shared_run_mode(initial: RunMode) -> Rc<Cell<RunMode>> {
    Rc::new(Cell::new(initial))
}

// ── RunModeRow (two-button segmented control) ─────────────────────────────────

/// Reactive / Continuous segmented selector composed from two real `Button`
/// children that share an `Rc<Cell<RunMode>>`.  Used inside `PerformanceView`
/// when [`super::PerformanceView::with_run_mode_selector`] is wired; safe to
/// use standalone for hosts that want the picker without the perf graph.
pub struct RunModeRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl RunModeRow {
    /// Total row height (button + a few pixels of vertical breathing room).
    pub const ROW_HEIGHT: f64 = 32.0;
    const BTN_H: f64 = 24.0;

    pub fn new(font: Arc<Font>, run_mode: Rc<Cell<RunMode>>) -> Self {
        let segments: [(&'static str, RunMode); 2] = [
            ("Reactive", RunMode::Reactive),
            ("Continuous", RunMode::Continuous),
        ];
        let children: Vec<Box<dyn Widget>> = segments
            .iter()
            .map(|(label, this_mode)| {
                let mode_active = Rc::clone(&run_mode);
                let mode_click = Rc::clone(&run_mode);
                let this = *this_mode;
                let btn = Button::new(*label, Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_subtle()
                    .with_active_fn(move || mode_active.get() == this)
                    .on_click(move || {
                        if mode_click.get() != this {
                            mode_click.set(this);
                            animation::request_draw();
                        }
                    });
                Box::new(btn) as Box<dyn Widget>
            })
            .collect();
        Self {
            bounds: Rect::default(),
            children,
        }
    }
}

impl Widget for RunModeRow {
    fn type_name(&self) -> &'static str {
        "RunModeRow"
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
        let row_h = Self::ROW_HEIGHT;
        self.bounds = Rect::new(0.0, 0.0, available.width, row_h);
        let gy = (row_h - Self::BTN_H) * 0.5;
        let gap = 4.0;
        let btn_w = ((available.width - gap) * 0.5).max(40.0);
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(btn_w, Self::BTN_H));
            child.set_bounds(Rect::new(
                i as f64 * (btn_w + gap),
                gy,
                btn_w,
                Self::BTN_H,
            ));
        }
        Size::new(available.width, row_h)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Buttons paint themselves through the framework's tree walk.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── RunModeDesc (dynamic description label) ───────────────────────────────────

/// Dynamic label that mirrors the current run mode.
///   Reactive   — "Only running UI code when there are animations or input."
///   Continuous — "Running continuously as fast as possible.  FPS: X.X"
///
/// Lives in agg-gui so hosts that wire `RunMode` get the same wording every
/// app uses without re-implementing the FPS readout.
pub struct RunModeDesc {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    run_mode: Rc<Cell<RunMode>>,
    history: SharedFrameHistory,
    label: Label,
}

impl RunModeDesc {
    pub fn new(font: Arc<Font>, run_mode: Rc<Cell<RunMode>>, history: SharedFrameHistory) -> Self {
        let mut label = Label::new("", Arc::clone(&font))
            .with_font_size(10.0)
            .with_wrap(true);
        label.buffered = false;
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            run_mode,
            history,
            label,
        }
    }
}

impl Widget for RunModeDesc {
    fn type_name(&self) -> &'static str {
        "RunModeDesc"
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
        // Use the longer (reactive) string for height measurement so the
        // layout is stable when the user flips modes mid-run.
        self.label
            .set_text("Only running UI code when there are animations or input.".to_owned());
        let s = self.label.layout(Size::new(available.width, f64::MAX / 2.0));
        self.label
            .set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        let h = s.height.max(available.height).max(14.0);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let text = match self.run_mode.get() {
            RunMode::Reactive => {
                "Only running UI code when there are animations or input.".to_owned()
            }
            RunMode::Continuous => {
                let hist = self.history.borrow();
                let fps = if hist.mean_ms() < 0.001 {
                    0.0
                } else {
                    1000.0 / hist.mean_ms()
                };
                format!("Running continuously as fast as possible. FPS: {fps:.1}")
            }
        };
        self.label.set_text(text);
        self.label.set_color(v.text_dim);

        let lh = self.label.bounds().height;
        let ly = ((self.bounds.height - lh) * 0.5).max(0.0);
        ctx.save();
        ctx.translate(0.0, ly);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn needs_draw(&self) -> bool {
        // Continuous mode shows an FPS readout that changes every frame;
        // request a redraw so the number stays current even when nothing
        // else in the tree is dirty.  Reactive mode keeps the description
        // string constant — no extra wakeups needed.
        matches!(self.run_mode.get(), RunMode::Continuous)
    }
}
