//! `AlphaTrack` — paint-only checkerboard + alpha-gradient strip child.
//!
//! Renders a horizontal track that ramps the current colour from
//! `alpha = 0` (left) to `alpha = 1` (right) over an opaque checkerboard
//! so the transparency reads correctly.  Pixels land in an
//! `Arc<Vec<u8>>` which `draw_image_rgba_arc` keys on for GPU texture
//! caching — same hardware back-buffer story as `HueWheel`.
//!
//! Cache invalidates when the base RGB colour or pixel size changes.
//! The parent `ColorWheelPicker` pushes a new base colour each layout
//! pass via [`AlphaTrack::set_base_rgb`].

use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::WidgetBase;
use crate::widget::Widget;

const CHECK_TILE: u32 = 6;
const CHECK_LIGHT: [u8; 3] = [191, 191, 191];
const CHECK_DARK: [u8; 3] = [115, 115, 115];

pub struct AlphaTrack {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,

    pixels: Option<Arc<Vec<u8>>>,
    /// Device-pixel dimensions of the cached buffer.
    cached_size: (u32, u32),
    cached_scale: f64,
    cached_rgb: (u8, u8, u8),

    base_rgb: (u8, u8, u8),
}

impl AlphaTrack {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
            pixels: None,
            cached_size: (0, 0),
            cached_scale: 1.0,
            cached_rgb: (0, 0, 0),
            base_rgb: (255, 255, 255),
        }
    }

    /// Update the base RGB that the alpha gradient interpolates toward.
    /// Pushed by the picker after every hue/SV change.
    pub fn set_base_rgb(&mut self, r: f32, g: f32, b: f32) {
        let r = (r * 255.0).round().clamp(0.0, 255.0) as u8;
        let g = (g * 255.0).round().clamp(0.0, 255.0) as u8;
        let b = (b * 255.0).round().clamp(0.0, 255.0) as u8;
        self.base_rgb = (r, g, b);
    }

    fn cache_matches(&self, w: u32, h: u32, scale: f64) -> bool {
        self.pixels.is_some()
            && self.cached_size == (w, h)
            && (self.cached_scale - scale).abs() < 1.0e-4
            && self.cached_rgb == self.base_rgb
    }

    fn rasterise(w: u32, h: u32, rgb: (u8, u8, u8)) -> Vec<u8> {
        let mut out = vec![0u8; (w * h * 4) as usize];
        if w == 0 || h == 0 {
            return out;
        }
        let (sr, sg, sb) = (rgb.0 as f32, rgb.1 as f32, rgb.2 as f32);
        for row in 0..h {
            for col in 0..w {
                // Checkerboard background.
                let bx = col / CHECK_TILE;
                let by = row / CHECK_TILE;
                let bg = if (bx + by) & 1 == 0 {
                    CHECK_LIGHT
                } else {
                    CHECK_DARK
                };
                // Alpha follows column position.
                let t = if w > 1 {
                    col as f32 / (w - 1) as f32
                } else {
                    0.0
                };
                // Straight-alpha source over premultiplied checker.
                let r = sr * t + bg[0] as f32 * (1.0 - t);
                let g = sg * t + bg[1] as f32 * (1.0 - t);
                let b = sb * t + bg[2] as f32 * (1.0 - t);
                let i = ((row * w + col) * 4) as usize;
                out[i] = r.round().clamp(0.0, 255.0) as u8;
                out[i + 1] = g.round().clamp(0.0, 255.0) as u8;
                out[i + 2] = b.round().clamp(0.0, 255.0) as u8;
                out[i + 3] = 255;
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
        let pixels = Self::rasterise(w, h, self.base_rgb);
        self.pixels = Some(Arc::new(pixels));
        self.cached_size = (w, h);
        self.cached_scale = scale;
        self.cached_rgb = self.base_rgb;
    }
}

impl Default for AlphaTrack {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for AlphaTrack {
    fn type_name(&self) -> &'static str {
        "AlphaTrack"
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
        Size::new(available.width.max(0.0), available.height.max(0.0))
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
        ctx.draw_image_rgba_arc(data, w, h, 0.0, 0.0, self.bounds.width, self.bounds.height);
    }
    fn hit_test(&self, _: Point) -> bool {
        false
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
