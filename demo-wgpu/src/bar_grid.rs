//! `BarGridWgpuRenderer` and `WgpuCubeWidget` — wgpu port of the 3-D bar-grid
//! animation widget.
//!
//! Mirrors the role of `bar_grid.rs` in `demo-gl`: both the renderer and the
//! widget live in this shared crate so that `demo-native` and `demo-wasm` use
//! exactly the same compiled bytes.
//!
//! # Status
//!
//! This is currently a **placeholder stub**: the widget renders the theme
//! window-fill colour over its rect (matching the GL backend's "gaps between
//! bars" fallback) and exposes an empty `gl_paint` so that the demo's 3-D
//! Animation tab continues to lay out and render normally.  The 791-line GLSL
//! → WGSL port of the actual instanced bar-grid scene (sine-field height map,
//! per-bar gradient, palette refresh) is deferred — when implemented it will
//! plug into `BarGridWgpuRenderer` and be invoked from this widget's
//! `gl_paint`.

use std::cell::Cell;

use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::Rect;
use agg_gui::widget::Widget;
use agg_gui::{GlPaint, Size, TransAffine};

thread_local! {
    /// Set each frame by [`WgpuCubeWidget::paint`].  Mirrors the GL backend
    /// constant of the same name so platform shells with debug-overlay code
    /// compiled against either backend keep working.
    pub static CUBE_SCREEN_RECT: Cell<Rect> = Cell::new(Rect::default());
}

/// Wgpu renderer for the instanced 3-D bar grid.
///
/// Phase 9 stub — empty marker struct.  Construction is deferred until the
/// renderer actually issues GPU draw calls.
pub struct BarGridWgpuRenderer;

impl BarGridWgpuRenderer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BarGridWgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Widget node for the 3-D Animation tab.  Currently renders a placeholder
/// fill; the actual bar-grid GPU draw will be added when the WGSL shader port
/// lands (see module-level docs).
pub struct WgpuCubeWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    renderer: Option<BarGridWgpuRenderer>,
}

impl Default for WgpuCubeWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuCubeWidget {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            renderer: None,
        }
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

    /// Match the GL widget's continuous redraw signal — when the bar grid is
    /// implemented this keeps the host loop animating; until then it costs a
    /// frame per tick when the tab is visible.
    fn needs_draw(&self) -> bool {
        true
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let t = ctx.transform();
        let screen_rect = transformed_widget_rect(&t, self.bounds.width, self.bounds.height);
        CUBE_SCREEN_RECT.with(|r| r.set(screen_rect));

        // Theme-aware placeholder fill — same approach as the GL backend's
        // `window_fill` background under the bar geometry.
        ctx.set_fill_color(ctx.visuals().window_fill);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();

        // The GL widget calls `ctx.gl_paint(screen_rect, self)` here to render
        // bars on top of the placeholder.  Skipped on the wgpu backend until
        // the WGSL bar-grid renderer is implemented.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Empty `GlPaint` impl so dispatchers that downcast to a wgpu-specific paint
/// context don't observe a different widget type.  Becomes meaningful when the
/// WGSL port lands.
impl GlPaint for WgpuCubeWidget {
    fn gl_paint(
        &mut self,
        _gl: &dyn std::any::Any,
        _screen_rect: Rect,
        _full_w: i32,
        _full_h: i32,
        _parent_clip: Option<[i32; 4]>,
    ) {
        // No-op until the bar-grid wgpu renderer is implemented.
        let _ = &self.renderer;
    }
}
