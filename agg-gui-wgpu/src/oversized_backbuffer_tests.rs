//! A backbuffered widget wider than the device's largest texture, painted
//! through the real [`WgpuGfxCtx`] onto an offscreen target.
//!
//! The cached bitmap of a backbuffered widget is uploaded as one texture. Past
//! `max_texture_dimension_2d` that upload was a validation error, and wgpu's
//! default error handler panics: a Label stretched by a long line of text
//! crashed a station app on every click. The widget must paint directly
//! instead, and its pixels must still reach the target. The software half is
//! `agg-gui/src/tests/backbuffer_size_limit.rs`. Skipped without a GPU adapter.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult};
use agg_gui::geometry::{Rect, Size};
use agg_gui::widget::{paint_subtree, BackbufferCache, BackbufferMode, Widget};

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

/// White, with a black block at its left end: the part on screen.
struct WideWidget {
    bounds: Rect,
    cache: BackbufferCache,
    mode: BackbufferMode,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for WideWidget {
    fn type_name(&self) -> &'static str {
        "WideWidget"
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
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
        ctx.set_fill_color(Color::black());
        ctx.begin_path();
        ctx.rect(8.0, 8.0, 16.0, 16.0);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn backbuffer_cache_mut(&mut self) -> Option<&mut BackbufferCache> {
        Some(&mut self.cache)
    }
    fn backbuffer_mode(&self) -> BackbufferMode {
        self.mode
    }
}

/// Paint a `mode` widget one pixel wider than the device's texture limit on
/// a 64 x 32 target, and read the target back.
fn paint_past_the_limit(mode: BackbufferMode) -> Option<Vec<u8>> {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return None;
    };
    let (w, h) = (64u32, 32u32);
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);
    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);
    ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));

    let limit = device.limits().max_texture_dimension_2d;
    assert_eq!(ctx.max_image_dimension(), Some(limit));
    let mut widget = WideWidget {
        bounds: Rect::new(0.0, 0.0, limit as f64 + 1.0, h as f64),
        cache: BackbufferCache::default(),
        mode,
        children: Vec::new(),
    };
    widget.cache.invalidate();
    paint_subtree(&mut widget, &mut ctx);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

/// The block is black and the rest of the visible part is white: the widget
/// painted, directly, over the red clear.
fn assert_painted(data: &[u8], what: &str) {
    let w = 64;
    // Readback rows are top-first; the block covers y 8..24 (Y-up) of 32.
    let block = px(data, w, 16, 16);
    assert!(
        block[0] < 40 && block[1] < 40 && block[2] < 40,
        "{what}: the block is black, got {block:?}"
    );
    let background = px(data, w, 48, 16);
    assert!(
        background[0] > 215 && background[1] > 215 && background[2] > 215,
        "{what}: the widget's background is white, got {background:?}"
    );
}

#[test]
fn an_rgba_backbuffer_past_the_texture_limit_paints_directly() {
    if let Some(data) = paint_past_the_limit(BackbufferMode::Rgba) {
        assert_painted(&data, "Rgba");
    }
}

#[test]
fn an_lcd_backbuffer_past_the_texture_limit_paints_directly() {
    if let Some(data) = paint_past_the_limit(BackbufferMode::LcdCoverage) {
        assert_painted(&data, "LcdCoverage");
    }
}
