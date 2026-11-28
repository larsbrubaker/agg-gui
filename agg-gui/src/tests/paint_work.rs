//! Tests for [`crate::paint_work`]: the per-thread counts of widget backbuffer
//! rasters and text-mask rasters.
//!
//! Each test paints through the production paths (`paint_subtree` into a
//! software `GfxCtx`, and `rasterize_text_lcd_cached`) and reads the counters
//! before and after, so a cached frame proves it did no raster work. The
//! counters are thread-local, so tests running in parallel threads do not see
//! each other's work.

use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;
use crate::lcd_coverage::rasterize_text_lcd_cached;
use crate::paint_work::{paint_work, PaintWork};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::Label;
use crate::{Color, Rect, Size};
use std::sync::Arc;

const FONT_BYTES: &[u8] = include_bytes!("../../../demo/assets/CascadiaCode.ttf");

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(FONT_BYTES).expect("font"))
}

/// Paints `label` once into a fresh framebuffer and returns the work done.
fn paint_once(label: &mut Label) -> PaintWork {
    let before = paint_work();
    let mut fb = Framebuffer::new(200, 60);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        paint_subtree(label, &mut ctx);
    }
    paint_work().since(before)
}

#[test]
fn paint_work_counts_backbuffer_rasters_only_when_the_cache_is_stale() {
    let mut label = Label::new("paint work", font())
        .with_font_size(16.0)
        .with_color(Color::black())
        .with_has_backbuffer(true);
    let used = label.layout(Size::new(200.0, 60.0));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));

    assert_eq!(
        paint_once(&mut label).backbuffer_rasters,
        1,
        "first paint rasters"
    );
    assert_eq!(
        paint_once(&mut label).backbuffer_rasters,
        0,
        "cached paint only blits"
    );

    label.set_text("paint work changed");
    let used = label.layout(Size::new(200.0, 60.0));
    label.set_bounds(Rect::new(0.0, 0.0, used.width, used.height));
    assert_eq!(
        paint_once(&mut label).backbuffer_rasters,
        1,
        "a new text re-rasters"
    );
    assert_eq!(
        paint_once(&mut label).backbuffer_rasters,
        0,
        "then it is cached again"
    );
}

#[test]
fn paint_work_counts_text_mask_misses_not_hits() {
    let font = font();
    // A text no other test on this thread has rastered.
    let text = "paint_work mask miss 7q";
    let before = paint_work();
    let _ = rasterize_text_lcd_cached(&font, text, 17.25);
    let after_miss = paint_work();
    assert_eq!(
        after_miss.since(before).text_mask_rasters,
        1,
        "a miss rasters once"
    );
    let _ = rasterize_text_lcd_cached(&font, text, 17.25);
    assert_eq!(
        paint_work().since(after_miss).text_mask_rasters,
        0,
        "a hit rasters nothing"
    );
}

#[test]
fn paint_work_since_is_a_field_wise_difference() {
    let earlier = PaintWork {
        backbuffer_rasters: 2,
        text_mask_rasters: 5,
    };
    let later = PaintWork {
        backbuffer_rasters: 7,
        text_mask_rasters: 6,
    };
    assert_eq!(
        later.since(earlier),
        PaintWork {
            backbuffer_rasters: 5,
            text_mask_rasters: 1
        }
    );
}
