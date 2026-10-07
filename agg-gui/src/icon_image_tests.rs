//! Tests for [`super::IconImage`]: physical-resolution SVG rasters, the
//! per-size cache, the straight-alpha pixel convention and the pixel filter.

use std::sync::Arc;

use super::*;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;

const RED_SQUARE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
    <rect width="10" height="10" fill="#ff0000" fill-opacity="0.5"/>
</svg>"##;

#[test]
fn svg_icon_rasterises_at_physical_size() {
    let icon = IconImage::from_svg_data(RED_SQUARE, Size::new(16.0, 16.0)).expect("parse");
    let (_, w, h) = icon.raster_at_scale(1.0).expect("1x raster");
    assert_eq!((w, h), (16, 16));
    let (_, w, h) = icon.raster_at_scale(2.0).expect("2x raster");
    assert_eq!((w, h), (32, 32));
    let (_, w, h) = icon.raster_at_scale(1.5).expect("1.5x raster");
    assert_eq!((w, h), (24, 24));
}

#[test]
fn svg_icon_raster_is_straight_alpha() {
    let icon = IconImage::from_svg_data(RED_SQUARE, Size::new(4.0, 4.0)).expect("parse");
    let (data, _, _) = icon.raster_at_scale(1.0).expect("raster");
    // Half-transparent pure red (the rasteriser maps opacity 0.5 to alpha
    // 127): straight alpha keeps R at 255.
    assert_eq!(&data[0..4], &[255, 0, 0, 127]);
}

#[test]
fn raster_cache_reuses_the_same_arc_per_size() {
    let icon = IconImage::from_svg_data(RED_SQUARE, Size::new(8.0, 8.0)).expect("parse");
    let (a, _, _) = icon.raster_at_scale(2.0).expect("raster");
    let (_, _, _) = icon.raster_at_scale(1.0).expect("raster");
    let (b, _, _) = icon.raster_at_scale(2.0).expect("raster");
    assert!(Arc::ptr_eq(&a, &b), "2x raster survives a 1x request");
    let clone = icon.clone();
    let (c, _, _) = clone.raster_at_scale(2.0).expect("raster");
    assert!(Arc::ptr_eq(&a, &c), "clones share the cache");
    assert_eq!(icon, clone);
}

#[test]
fn pixel_filter_runs_on_straight_alpha_rasters() {
    let icon = IconImage::from_svg_data(RED_SQUARE, Size::new(4.0, 4.0))
        .expect("parse")
        .with_pixel_filter(Arc::new(|px: &mut [u8]| {
            for p in px.chunks_exact_mut(4) {
                p[0] = 0;
                p[2] = 255;
            }
        }));
    let (data, _, _) = icon.raster_at_scale(1.0).expect("raster");
    assert_eq!(&data[0..4], &[0, 0, 255, 127]);
}

#[test]
fn rgba_icon_validates_dimensions() {
    let px = Arc::new(vec![255u8; 2 * 2 * 4]);
    assert!(IconImage::from_rgba(Arc::clone(&px), 2, 2, Size::new(2.0, 2.0)).is_some());
    assert!(IconImage::from_rgba(Arc::clone(&px), 3, 2, Size::new(2.0, 2.0)).is_none());
    assert!(IconImage::from_rgba(px, 0, 0, Size::new(2.0, 2.0)).is_none());
}

#[test]
fn draw_blits_the_icon_into_its_logical_rect() {
    let icon = IconImage::from_svg_data(
        br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
            <rect width="10" height="10" fill="#00ff00"/></svg>"##,
        Size::new(4.0, 4.0),
    )
    .expect("parse");
    let mut fb = Framebuffer::new(10, 10);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        icon.draw(&mut ctx, 2.0, 3.0);
    }
    let at = |x: u32, y: u32| {
        let i = ((y * fb.width() + x) * 4) as usize;
        fb.pixels()[i..i + 4].to_vec()
    };
    assert_eq!(at(3, 4), vec![0, 255, 0, 255], "inside the icon rect");
    assert_eq!(at(7, 4), vec![0, 0, 0, 0], "right of the icon rect");
    assert_eq!(at(3, 1), vec![0, 0, 0, 0], "below the icon rect");
}
