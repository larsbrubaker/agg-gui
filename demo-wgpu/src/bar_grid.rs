//! `WgpuCubeWidget` — wgpu-port of the 3-D bar-grid demo widget.
//!
//! Mirrors the role of `bar_grid.rs` in `demo-gl`: the widget lives in this
//! shared crate so that `demo-native` and `demo-wasm` use exactly the same
//! compiled bytes.
//!
//! Renderer code (pipeline, shader, geometry, framebuffer) is in the sibling
//! [`crate::bar_grid_render`] module — split out to keep this file under the
//! 800-line per-module limit.  External callers continue to use the same
//! paths via the re-export of [`BarGridWgpuRenderer`] below.
//!
//! # Theme integration
//!
//! `bar_palette_for_theme()` (in `bar_grid_render`) reads
//! `agg_gui::current_visuals()` each frame, so a light/dark toggle recolours
//! the bars on the next paint without rebuilding the pipeline.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::Rect;
use agg_gui::widget::Widget;
use agg_gui::{Size, TransAffine};

pub use crate::bar_grid_render::BarGridWgpuRenderer;
use crate::{DrawCommand, WgpuGfxCtx};

thread_local! {
    /// Set each frame by [`WgpuCubeWidget::paint`].  Mirrors the GL backend
    /// constant of the same name so platform shells with debug-overlay code
    /// compiled against either backend keep working.
    pub static CUBE_SCREEN_RECT: Cell<Rect> = Cell::new(Rect::default());
}

// ---------------------------------------------------------------------------
// WgpuCubeWidget
// ---------------------------------------------------------------------------

pub struct WgpuCubeWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Lazy-init renderer shared with the deferred draw command.  Wrapped in
    /// `Rc<RefCell<Option<>>>` so the widget can keep ownership while the
    /// `DrawCommand::DrawBarGrid` queued for this frame holds a clone of the
    /// `Rc` and reads the renderer back at execute time.
    renderer: Rc<RefCell<Option<BarGridWgpuRenderer>>>,
    /// Shared SSAA samples cell.  Values map to a linear framebuffer scale
    /// via [`crate::ssaa::ssaa_linear_scale`]: `1`/`0` → 1× (Off), `4` → 2×
    /// (4× SSAA), `9` → 3× (9× SSAA), `16` → 4× (16× SSAA).  The widget
    /// rebuilds the renderer when the resulting linear scale changes.  UI
    /// controls (Off / 4× / 9× / 16× toolbar at the top of the 3-D
    /// Animation window) write to the same cell — same `Rc<Cell<u8>>` the
    /// demo-ui state layer persists, so a tweak round-trips to disk for free.
    sample_count: Rc<Cell<u8>>,
    /// Animation start time — owned by the widget so it survives renderer
    /// rebuilds (the SSAA toggle drops + recreates the renderer to apply
    /// the new sample count).  Passing the same `start` to each new
    /// `BarGridWgpuRenderer` keeps the bar wave phase continuous, so the
    /// only visible change at a toggle is the AA itself.
    start: web_time::Instant,
}

impl Default for WgpuCubeWidget {
    fn default() -> Self {
        Self::new(Rc::new(Cell::new(0)))
    }
}

impl WgpuCubeWidget {
    /// Build a new cube widget bound to a shared SSAA samples `Rc<Cell<u8>>`.
    /// The cell stores the UI-facing pixel multiplier (`0`/`1` = Off,
    /// `4` = 4× SSAA, `9` = 9× SSAA, `16` = 16× SSAA).  Values get clamped
    /// on the read side by [`crate::ssaa::ssaa_linear_scale`], so an old
    /// saved MSAA `8` (or any out-of-band value) maps to a sensible step
    /// instead of panicking.  The cell itself preserves the user's raw
    /// choice for state persistence.
    pub fn new(sample_count: Rc<Cell<u8>>) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            renderer: Rc::new(RefCell::new(None)),
            sample_count,
            start: web_time::Instant::now(),
        }
    }

    /// Borrow a clone of the shared sample-count cell.  UI controls that
    /// want to drive the SSAA setting (and have the persistence layer
    /// write through to disk) can grab a clone via this getter.
    pub fn sample_count_cell(&self) -> Rc<Cell<u8>> {
        Rc::clone(&self.sample_count)
    }
}

fn transformed_widget_rect(t: &TransAffine, width: f64, height: f64) -> Rect {
    let corners = [(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (mut x, mut y) in corners {
        t.transform(&mut x, &mut y);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

impl Widget for WgpuCubeWidget {
    fn type_name(&self) -> &'static str {
        "WgpuCubeWidget"
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
        available
    }

    fn needs_draw(&self) -> bool {
        true
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let t = ctx.transform();
        let screen_rect = transformed_widget_rect(&t, self.bounds.width, self.bounds.height);
        CUBE_SCREEN_RECT.with(|r| r.set(screen_rect));

        // Theme-aware backdrop — fills the gaps the bars don't cover.
        ctx.set_fill_color(ctx.visuals().window_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // Backend-specific path: downcast to WgpuGfxCtx and queue a deferred
        // bar-grid draw.  On non-wgpu backends the downcast yields `None`
        // and the widget is just the placeholder fill above — the demo
        // still lays out and renders, the bars simply don't appear.
        if let Some(any) = ctx.as_any_mut() {
            if let Some(wgpu_ctx) = any.downcast_mut::<WgpuGfxCtx>() {
                // Read the shared SSAA-samples cell and convert to the
                // linear framebuffer scale.  Rebuild the renderer if the
                // resulting scale no longer matches the active one.  The UI
                // toolbar (Off / 4× / 9× / 16×) writes to this cell, so the
                // toggle takes effect on the next paint with no restart.
                let raw = self.sample_count.get() as u32;
                let desired_scale = crate::ssaa::ssaa_linear_scale(raw);
                {
                    let mut slot = self.renderer.borrow_mut();
                    let needs_rebuild = match slot.as_ref() {
                        Some(r) => r.ssaa_scale() != desired_scale,
                        None => true,
                    };
                    if needs_rebuild {
                        *slot = Some(BarGridWgpuRenderer::new(
                            &wgpu_ctx.device,
                            wgpu_ctx.surface_format,
                            raw,
                            self.start,
                        ));
                    }
                }
                let parent_clip = wgpu_ctx.current_clip();
                wgpu_ctx.commands.push(DrawCommand::DrawBarGrid {
                    renderer: Rc::clone(&self.renderer),
                    screen_rect,
                    parent_clip,
                });
            }
        }
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_widget_rect_scales_logical_bounds_to_physical_pixels() {
        let transform = TransAffine::new_custom(2.0, 0.0, 0.0, 2.0, 40.0, 24.0);
        let rect = transformed_widget_rect(&transform, 100.0, 50.0);
        assert_eq!(rect.x, 40.0);
        assert_eq!(rect.y, 24.0);
        assert_eq!(rect.width, 200.0);
        assert_eq!(rect.height, 100.0);
    }
}
