//! `PreviewSwatches` — paint-only "old | new" comparison strip child.
//!
//! Draws a checkerboard background then two halves: the **left** half
//! is the colour the picker was opened with (saved snapshot), the
//! **right** half is the live working colour.  The user can A/B the
//! pending change at a glance, mirroring NodeDesigner's swatch row.
//!
//! Pixels land in an `Arc<Vec<u8>>` for the same hardware GPU texture
//! caching path used by the wheel / triangle / alpha widgets.  Cache
//! invalidates when either colour or the pixel size changes.

use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::layout_props::WidgetBase;
use crate::widget::Widget;

const CHECK_TILE: u32 = 6;
const CHECK_LIGHT: [u8; 3] = [191, 191, 191];
const CHECK_DARK: [u8; 3] = [115, 115, 115];

pub struct PreviewSwatches {
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,

    pixels: Option<Arc<Vec<u8>>>,
    /// Device-pixel dimensions of the cached buffer.
    cached_size: (u32, u32),
    cached_scale: f64,
    cached_old: [u8; 4],
    cached_new: [u8; 4],

    old_color: Color,
    new_color: Color,
}

impl PreviewSwatches {
    pub fn new() -> Self {
        Self {
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
            pixels: None,
            cached_size: (0, 0),
            cached_scale: 1.0,
            cached_old: [0, 0, 0, 0],
            cached_new: [0, 0, 0, 0],
            old_color: Color::transparent(),
            new_color: Color::transparent(),
        }
    }

    pub fn set_old(&mut self, c: Color) {
        self.old_color = c;
    }

    pub fn set_new(&mut self, c: Color) {
        self.new_color = c;
    }

    fn rgba_bytes(c: Color) -> [u8; 4] {
        [
            (c.r * 255.0).round().clamp(0.0, 255.0) as u8,
            (c.g * 255.0).round().clamp(0.0, 255.0) as u8,
            (c.b * 255.0).round().clamp(0.0, 255.0) as u8,
            (c.a * 255.0).round().clamp(0.0, 255.0) as u8,
        ]
    }

    fn cache_matches(&self, w: u32, h: u32, scale: f64, old: [u8; 4], new: [u8; 4]) -> bool {
        self.pixels.is_some()
            && self.cached_size == (w, h)
            && (self.cached_scale - scale).abs() < 1.0e-4
            && self.cached_old == old
            && self.cached_new == new
    }

    fn rasterise(w: u32, h: u32, old_rgba: [u8; 4], new_rgba: [u8; 4]) -> Vec<u8> {
        let mut out = vec![0u8; (w * h * 4) as usize];
        if w == 0 || h == 0 {
            return out;
        }
        let mid = w / 2;
        for row in 0..h {
            for col in 0..w {
                let bx = col / CHECK_TILE;
                let by = row / CHECK_TILE;
                let bg = if (bx + by) & 1 == 0 {
                    CHECK_LIGHT
                } else {
                    CHECK_DARK
                };
                let rgba = if col < mid { old_rgba } else { new_rgba };
                let t = rgba[3] as f32 / 255.0;
                let r = rgba[0] as f32 * t + bg[0] as f32 * (1.0 - t);
                let g = rgba[1] as f32 * t + bg[1] as f32 * (1.0 - t);
                let b = rgba[2] as f32 * t + bg[2] as f32 * (1.0 - t);
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
        let old = Self::rgba_bytes(self.old_color);
        let new = Self::rgba_bytes(self.new_color);
        if self.cache_matches(w, h, scale, old, new) {
            return;
        }
        let pixels = Self::rasterise(w, h, old, new);
        self.pixels = Some(Arc::new(pixels));
        self.cached_size = (w, h);
        self.cached_scale = scale;
        self.cached_old = old;
        self.cached_new = new;
    }
}

impl Default for PreviewSwatches {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for PreviewSwatches {
    fn type_name(&self) -> &'static str {
        "PreviewSwatches"
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
