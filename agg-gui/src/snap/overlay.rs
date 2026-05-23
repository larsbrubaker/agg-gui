//! `SnapOverlay` — global overlay widget that paints the snap engine's
//! visual guides on top of the rest of the UI.
//!
//! Reads from the thread-local guide buffer set by drag handlers via
//! [`set_guides`].  Doesn't intercept any input — purely visual.
//!
//! Apps add one `SnapOverlay` as the topmost child of their root
//! Stack (or any container that's the last to paint).  The widget's
//! bounds are typically the full canvas; guide coordinates flow
//! through unchanged because both the overlay and the rects use the
//! same root coordinate space.
//!
//! [`set_guides`]: super::registry::set_guides

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Rect, Size};
use crate::widget::Widget;

use super::registry::guides_snapshot;
use super::SnapGuide;

/// Visual guide overlay.  No state of its own — pure renderer over
/// the thread-local guide buffer.
pub struct SnapOverlay {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>, // always empty
}

impl SnapOverlay {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }

    /// Cyan-ish accent line for edge alignment guides.  Tuned to read
    /// as a guide line without competing with content underneath.
    fn alignment_color() -> Color {
        Color::rgba(0.15, 0.70, 0.95, 0.95)
    }

    /// Pink-ish dimension line for equal-spacing markers — distinct
    /// from alignment so the user can tell at a glance which kind of
    /// snap engaged.
    fn spacing_color() -> Color {
        Color::rgba(0.95, 0.35, 0.55, 0.95)
    }
}

impl Default for SnapOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for SnapOverlay {
    fn type_name(&self) -> &'static str {
        "SnapOverlay"
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
        // Fill the available area — guide coordinates are in the
        // root frame, so the overlay needs to span the whole canvas
        // to host them without clipping.
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Guides paint in the overlay pass — see `paint_global_overlay`.
    }
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        let guides = guides_snapshot();
        if guides.is_empty() {
            return;
        }
        ctx.set_line_width(1.0);
        for guide in guides {
            match guide {
                SnapGuide::VLine { x, y0, y1 } => {
                    ctx.set_stroke_color(Self::alignment_color());
                    ctx.begin_path();
                    ctx.move_to(x.round() + 0.5, y0);
                    ctx.line_to(x.round() + 0.5, y1);
                    ctx.stroke();
                }
                SnapGuide::HLine { y, x0, x1 } => {
                    ctx.set_stroke_color(Self::alignment_color());
                    ctx.begin_path();
                    ctx.move_to(x0, y.round() + 0.5);
                    ctx.line_to(x1, y.round() + 0.5);
                    ctx.stroke();
                }
                SnapGuide::HSpacing { y, x0, x1 } => {
                    ctx.set_stroke_color(Self::spacing_color());
                    let yy = y.round() + 0.5;
                    ctx.begin_path();
                    ctx.move_to(x0, yy);
                    ctx.line_to(x1, yy);
                    ctx.stroke();
                    // Tick marks at each end so the gap reads as a
                    // dimension, not just a stray line.
                    paint_tick_v(ctx, x0, yy, 4.0);
                    paint_tick_v(ctx, x1, yy, 4.0);
                }
                SnapGuide::VSpacing { x, y0, y1 } => {
                    ctx.set_stroke_color(Self::spacing_color());
                    let xx = x.round() + 0.5;
                    ctx.begin_path();
                    ctx.move_to(xx, y0);
                    ctx.line_to(xx, y1);
                    ctx.stroke();
                    paint_tick_h(ctx, xx, y0, 4.0);
                    paint_tick_h(ctx, xx, y1, 4.0);
                }
            }
        }
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn hit_test(&self, _p: crate::geometry::Point) -> bool {
        // Pure visual overlay — never block input from underlying
        // widgets.
        false
    }
}

fn paint_tick_v(ctx: &mut dyn DrawCtx, x: f64, y: f64, half: f64) {
    ctx.begin_path();
    ctx.move_to(x, y - half);
    ctx.line_to(x, y + half);
    ctx.stroke();
}

fn paint_tick_h(ctx: &mut dyn DrawCtx, x: f64, y: f64, half: f64) {
    ctx.begin_path();
    ctx.move_to(x - half, y);
    ctx.line_to(x + half, y);
    ctx.stroke();
}
