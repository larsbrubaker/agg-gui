//! `HueWheel` — paint-only annular hue ring child of `ColorWheelPicker`.
//!
//! Rasterises a 360° hue ring (outer radius `r_outer`, inner radius
//! `r_inner`) into an `Arc<Vec<u8>>` of RGBA8 pixels and blits it via
//! [`DrawCtx::draw_image_rgba_arc`].  Because the destination keys its
//! GPU texture cache on the `Arc`'s pointer identity, the wheel becomes
//! a single GPU texture upload at startup / resize, reused unchanged
//! every subsequent frame — that's the "hardware back-buffer" path.
//!
//! The widget is **paint-only**: [`Widget::hit_test`] returns `false`
//! so events fall through to the parent `ColorWheelPicker`, which owns
//! all hue-drag logic.
//!
//! Coordinate convention: the wheel uses the agg-gui Y-up local frame
//! (origin at the bottom-left of the widget).  Pixel rows are written
//! top-row-first as `draw_image_rgba_arc` requires; the buffer is
//! flipped during rasterisation so the on-screen wheel matches Y-up
//! math (`atan2(dy, dx)` for the parent's hit-test).

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::WidgetBase;
use crate::widget::Widget;

use super::hsv_math::hsv_to_rgb;

/// Hue ring child widget.  Sized to a square and centred by the parent
/// during layout; uses the smaller of `width` / `height` as the wheel
/// diameter.
pub struct HueWheel {
    bounds: Rect,
    base: WidgetBase,
    /// Always empty — this widget paints nothing through children.
    children: Vec<Box<dyn Widget>>,

    /// Cached RGBA8 pixels, top-row-first.  `None` until the first
    /// `paint()` runs at non-zero size.
    pixels: Option<Arc<Vec<u8>>>,
    /// **Device-pixel** dimensions of the cached buffer (oversampled
    /// by `device_scale()` so the GL blit is 1:1 with physical pixels
    /// on HiDPI displays).
    cached_size: (u32, u32),
    cached_scale: f64,
    /// Outer radius as a fraction of `min(width, height) / 2`.  NodeDesigner
    /// uses 85 / 95 on a 190 / 95 canvas; we follow that ratio.
    outer_ratio: f64,
    /// Inner radius as a fraction of `min(width, height) / 2`.
    inner_ratio: f64,
}

impl HueWheel {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
            pixels: None,
            cached_size: (0, 0),
            cached_scale: 1.0,
            outer_ratio: 85.0 / 95.0,
            inner_ratio: 60.0 / 95.0,
        }
    }

    /// Outer radius in widget-local logical pixels for the current
    /// bounds.  Used by the parent for hit-testing the ring annulus.
    pub fn outer_radius(&self) -> f64 {
        self.half_extent() * self.outer_ratio
    }

    /// Inner radius in widget-local logical pixels.
    pub fn inner_radius(&self) -> f64 {
        self.half_extent() * self.inner_ratio
    }

    /// Centre of the wheel in widget-local coordinates.
    pub fn center(&self) -> Point {
        Point::new(self.bounds.width * 0.5, self.bounds.height * 0.5)
    }

    fn half_extent(&self) -> f64 {
        0.5 * self.bounds.width.min(self.bounds.height)
    }

    /// Rebuild the RGBA8 buffer for the supplied pixel size.
    fn rasterise(w: u32, h: u32, outer_ratio: f64, inner_ratio: f64) -> Vec<u8> {
        let mut out = vec![0u8; (w * h * 4) as usize];
        if w == 0 || h == 0 {
            return out;
        }
        let cx = w as f64 * 0.5;
        let cy = h as f64 * 0.5;
        let half = cx.min(cy);
        let r_outer = half * outer_ratio;
        let r_inner = half * inner_ratio;
        // Anti-alias a 1px feather inside / outside the ring so neighbouring
        // pixels carry a partial coverage rather than a hard staircase.
        let feather = 1.0_f64;
        for row in 0..h {
            // Pixel sample at the row centre.  We're writing top-row-first
            // (Y-down image convention), but the wheel angle math should
            // match the Y-up world the parent uses for hit-testing.  Flip
            // the row index here so the colour at "top of the image" lines
            // up with positive-Y on screen.
            let py = (h as f64 - 1.0 - row as f64) + 0.5 - cy;
            for col in 0..w {
                let px = col as f64 + 0.5 - cx;
                let r_sq = px * px + py * py;
                let i = ((row * w + col) * 4) as usize;
                if r_sq > (r_outer + feather) * (r_outer + feather)
                    || r_sq < (r_inner - feather).max(0.0).powi(2)
                {
                    // Outside the annulus, fully transparent.
                    continue;
                }
                let r = r_sq.sqrt();
                let mut coverage = 1.0;
                if r > r_outer {
                    coverage *= (1.0 - (r - r_outer) / feather).clamp(0.0, 1.0);
                }
                if r < r_inner {
                    coverage *= (1.0 - (r_inner - r) / feather).clamp(0.0, 1.0);
                }
                // atan2 returns (-π, π] CCW from +X; convert to degrees
                // [0, 360).  This matches the wheel-picker convention
                // where hue=0 sits at +X (3 o'clock) and grows CCW.
                let mut angle = py.atan2(px).to_degrees();
                if angle < 0.0 {
                    angle += 360.0;
                }
                let (cr, cg, cb) = hsv_to_rgb(angle as f32, 1.0, 1.0);
                let a = (coverage * 255.0).round().clamp(0.0, 255.0) as u8;
                out[i] = (cr * 255.0).round().clamp(0.0, 255.0) as u8;
                out[i + 1] = (cg * 255.0).round().clamp(0.0, 255.0) as u8;
                out[i + 2] = (cb * 255.0).round().clamp(0.0, 255.0) as u8;
                out[i + 3] = a;
            }
        }
        out
    }

    fn ensure_cache(&mut self) {
        let scale = crate::device_scale::device_scale().max(1.0);
        let w = (self.bounds.width * scale).round() as u32;
        let h = (self.bounds.height * scale).round() as u32;
        if self.pixels.is_some()
            && self.cached_size == (w, h)
            && (self.cached_scale - scale).abs() < 1.0e-4
        {
            return;
        }
        let pixels = Self::rasterise(w, h, self.outer_ratio, self.inner_ratio);
        self.pixels = Some(Arc::new(pixels));
        self.cached_size = (w, h);
        self.cached_scale = scale;
    }
}

impl Default for HueWheel {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for HueWheel {
    fn type_name(&self) -> &'static str {
        "HueWheel"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn layout(&mut self, available: Size) -> Size {
        // The parent (`ColorWheelPicker`) sizes us explicitly; we just
        // accept whatever rect it gives us.  When the parent asks for our
        // preferred size (`layout` called with non-zero available), report
        // a square at the smaller of the two dimensions.
        let side = available.width.min(available.height).max(0.0);
        Size::new(side, side)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.ensure_cache();
        let (w, h) = self.cached_size;
        if w == 0 || h == 0 {
            return;
        }
        let data = match &self.pixels {
            Some(d) => d,
            None => return,
        };
        // Buffer is device-pixel resolution; blit at the widget's
        // *logical* size for 1:1 sampling on HiDPI displays.
        ctx.draw_image_rgba_arc(data, w, h, 0.0, 0.0, self.bounds.width, self.bounds.height);
    }
    /// Paint-only: events fall through to the parent picker.
    fn hit_test(&self, _: Point) -> bool {
        false
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Convert a wheel-local point (relative to the wheel centre) to a hue
/// angle in degrees `[0, 360)`.  Returns `None` if the point sits outside
/// the ring annulus.
pub fn hue_from_local_point(
    p_relative_to_center: Point,
    inner_radius: f64,
    outer_radius: f64,
) -> Option<f32> {
    let r = (p_relative_to_center.x.powi(2) + p_relative_to_center.y.powi(2)).sqrt();
    if r < inner_radius * 0.7 || r > outer_radius * 1.3 {
        // Outside the ring (with a small grace zone so quick drags don't
        // lose the cursor) — caller treats this as "ignore".
        return None;
    }
    let mut deg = p_relative_to_center
        .y
        .atan2(p_relative_to_center.x)
        .to_degrees();
    if deg < 0.0 {
        deg += 360.0;
    }
    Some(deg as f32)
}

/// Test if `local` (in wheel-local coords) falls within the ring annulus
/// (no grace zone — used to gate the initial mouse-down).
pub fn point_in_ring(p_relative_to_center: Point, inner_radius: f64, outer_radius: f64) -> bool {
    let r2 = p_relative_to_center.x.powi(2) + p_relative_to_center.y.powi(2);
    r2 >= inner_radius * inner_radius && r2 <= outer_radius * outer_radius
}

/// Helper used by the parent picker: emit pixels for a single solid colour
/// — used to overlay the selector dot on the wheel via the standard draw
/// primitives.
#[inline]
pub fn rgb_from_hue_deg(hue_deg: f32) -> Color {
    let (r, g, b) = hsv_to_rgb(hue_deg, 1.0, 1.0);
    Color::rgb(r, g, b)
}
