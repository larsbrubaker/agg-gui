//! `SvTriangle` — paint-only saturation/value triangle child of `ColorWheelPicker`.
//!
//! Rasterises an equilateral triangle inscribed in the hue wheel, with
//! vertices at:
//!
//! - `v1` — pure hue (saturation = 1, value = 1) — sits on the ring at
//!   the current hue angle
//! - `v2` — white (saturation = 0, value = 1)
//! - `v3` — black (saturation = 0, value = 0)
//!
//! At each pixel inside the triangle we evaluate barycentric weights
//! `(w1, w2, w3)` and mix `w1 * hue + w2 * white + w3 * black`.  The
//! result lands in an `Arc<Vec<u8>>` so the on-screen render path is a
//! single GPU-cached texture blit — the same hardware back-buffer
//! contract `HueWheel` uses.
//!
//! The cache invalidates whenever the hue OR the pixel size changes;
//! callers update via [`SvTriangle::set_hue`] each layout pass.

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::WidgetBase;
use crate::widget::Widget;

use super::hsv_math::{barycentric, hsv_to_rgb, sv_triangle_vertices};

/// Saturation/value triangle child widget.
pub struct SvTriangle {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,

    pixels: Option<Arc<Vec<u8>>>,
    cached_hue_deg: f32,
    /// Pixel dimensions of the cached buffer.  This is **device**
    /// pixels — the rasteriser oversamples by `device_scale()` so the
    /// GL blit lands one rasterised pixel per physical pixel, keeping
    /// the gradient crisp on HiDPI displays.
    cached_size: (u32, u32),
    cached_scale: f64,

    /// Most recent hue requested by the parent picker.
    hue_deg: f32,
    /// Radius of the inscribed triangle, expressed as a fraction of
    /// `min(width, height) / 2`.  NodeDesigner sets the triangle radius
    /// to `inner_radius - 5` on a wheel where `inner_radius` = 60 on a
    /// 95-half canvas; that lands at ~58 % of the half-extent.
    triangle_ratio: f64,
}

impl SvTriangle {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
            pixels: None,
            cached_hue_deg: f32::NAN,
            cached_size: (0, 0),
            cached_scale: 1.0,
            hue_deg: 0.0,
            triangle_ratio: 55.0 / 95.0,
        }
    }

    /// Update the hue.  If the new value differs from the cached value
    /// the next `paint()` will rebuild the pixel buffer.
    pub fn set_hue(&mut self, hue_deg: f32) {
        self.hue_deg = hue_deg;
    }

    /// Triangle radius (logical pixels) for the current bounds.
    pub fn triangle_radius(&self) -> f64 {
        0.5 * self.bounds.width.min(self.bounds.height) * self.triangle_ratio
    }

    /// Centre of the triangle in widget-local coords.
    pub fn center(&self) -> Point {
        Point::new(self.bounds.width * 0.5, self.bounds.height * 0.5)
    }

    /// Vertices in widget-local (Y-up) coords.
    pub fn vertices(&self) -> ((f64, f64), (f64, f64), (f64, f64)) {
        let c = self.center();
        sv_triangle_vertices(c.x, c.y, self.triangle_radius(), self.hue_deg)
    }

    fn cache_matches(&self, w: u32, h: u32, scale: f64) -> bool {
        self.pixels.is_some()
            && self.cached_size == (w, h)
            && (self.cached_scale - scale).abs() < 1.0e-4
            && (self.cached_hue_deg - self.hue_deg).abs() < 1.0e-4
    }

    fn rasterise(w: u32, h: u32, hue_deg: f32, triangle_ratio: f64) -> Vec<u8> {
        let mut out = vec![0u8; (w * h * 4) as usize];
        if w == 0 || h == 0 {
            return out;
        }
        let cx = w as f64 * 0.5;
        let cy = h as f64 * 0.5;
        let radius = 0.5 * (cx.min(cy) * 2.0) * triangle_ratio;
        let (v1, v2, v3) = sv_triangle_vertices(cx, cy, radius, hue_deg);
        let (hr, hg, hb) = hsv_to_rgb(hue_deg, 1.0, 1.0);

        // Bounding box of the triangle in pixel coords (Y-up).
        let min_x = v1.0.min(v2.0).min(v3.0).floor().max(0.0) as i32;
        let max_x = v1.0.max(v2.0).max(v3.0).ceil().min(w as f64) as i32;
        let min_y = v1.1.min(v2.1).min(v3.1).floor().max(0.0) as i32;
        let max_y = v1.1.max(v2.1).max(v3.1).ceil().min(h as f64) as i32;

        // Pre-compute edge "outward normals" in barycentric weight
        // space so we can do real 2×2 sub-pixel coverage with constant
        // setup per pixel.  The triangle edge `wi == 0` has gradient
        // length per pixel = `||∇wi||`; one pixel step crosses
        // `||∇wi||` of weight, so a pixel's coverage along edge `i` is
        // approximately `clamp(wi / ||∇wi|| + 0.5, 0, 1)`.  Multiply
        // the per-edge coverages for the conservative AA factor.
        let denom = ((v2.0 - v1.0) * (v3.1 - v1.1) - (v3.0 - v1.0) * (v2.1 - v1.1)).abs();
        let edge_len = |a: (f64, f64), b: (f64, f64)| {
            let dx = b.0 - a.0;
            let dy = b.1 - a.1;
            (dx * dx + dy * dy).sqrt()
        };
        let len_e1 = edge_len(v2, v3); // edge opposite v1 — slope of w1
        let len_e2 = edge_len(v3, v1); // edge opposite v2 — slope of w2
        let len_e3 = edge_len(v1, v2); // edge opposite v3 — slope of w3
        let scale_w1 = if denom > 1e-9 { denom / len_e1 } else { 0.0 };
        let scale_w2 = if denom > 1e-9 { denom / len_e2 } else { 0.0 };
        let scale_w3 = if denom > 1e-9 { denom / len_e3 } else { 0.0 };

        for y_up in min_y..max_y {
            // Convert Y-up pixel row → top-row-first image row index.
            let row = (h as i32 - 1 - y_up).max(0) as u32;
            if row >= h {
                continue;
            }
            for x in min_x..max_x {
                let px = x as f64 + 0.5;
                let py = y_up as f64 + 0.5;
                let (w1, w2, w3) = barycentric((px, py), v1, v2, v3);
                // Convert each barycentric weight into a *signed
                // distance in pixels* from the edge it represents,
                // then anti-alias with a half-pixel window. Pixels
                // wholly inside all three edges report coverage=1
                // (sharp interior); pixels straddling an edge get a
                // single-pixel feather so the staircase reads smooth
                // without bleeding the gradient outside the shape.
                let d1 = w1 * scale_w1;
                let d2 = w2 * scale_w2;
                let d3 = w3 * scale_w3;
                let min_d = d1.min(d2).min(d3);
                if min_d < -0.5 {
                    continue;
                }
                let coverage = (min_d + 0.5).clamp(0.0, 1.0);
                let r = (w1 * hr as f64 + w2 * 1.0 + w3 * 0.0).clamp(0.0, 1.0);
                let g = (w1 * hg as f64 + w2 * 1.0 + w3 * 0.0).clamp(0.0, 1.0);
                let b = (w1 * hb as f64 + w2 * 1.0 + w3 * 0.0).clamp(0.0, 1.0);
                let i = ((row * w + x as u32) * 4) as usize;
                out[i] = (r * 255.0).round() as u8;
                out[i + 1] = (g * 255.0).round() as u8;
                out[i + 2] = (b * 255.0).round() as u8;
                out[i + 3] = (coverage * 255.0).round() as u8;
            }
        }
        out
    }

    fn ensure_cache(&mut self) {
        let scale = crate::device_scale::device_scale().max(1.0);
        let w = (self.bounds.width * scale).round() as u32;
        let h = (self.bounds.height * scale).round() as u32;
        if self.cache_matches(w, h, scale) {
            return;
        }
        let pixels = Self::rasterise(w, h, self.hue_deg, self.triangle_ratio);
        self.pixels = Some(Arc::new(pixels));
        self.cached_size = (w, h);
        self.cached_scale = scale;
        self.cached_hue_deg = self.hue_deg;
    }
}

impl Default for SvTriangle {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for SvTriangle {
    fn type_name(&self) -> &'static str {
        "SvTriangle"
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
        // Buffer is in device pixels; blit at the widget's *logical*
        // size so the GPU samples one rasterised pixel per physical
        // pixel on HiDPI displays (no upscale blur).
        ctx.draw_image_rgba_arc(
            data,
            w,
            h,
            0.0,
            0.0,
            self.bounds.width,
            self.bounds.height,
        );
    }
    fn hit_test(&self, _: Point) -> bool {
        false
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
