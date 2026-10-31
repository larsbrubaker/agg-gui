//! Tests that [`MarkdownView`] lays a picture out by the pixel density its
//! file says (`crate::image_density`): a 2x picture, in a paragraph or a
//! table cell, takes half its pixel size in logical points at every device
//! scale, so a 2x screen draws it pixel for pixel; an unmarked picture keeps
//! one pixel per point.

use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::{Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::Widget;

use super::selection::SelectableKind;
use super::{ImagePixels, ImageState, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");

/// The picture's pixel size: wide enough that a 1x layout would differ.
const IMG_W: u32 = 120;
const IMG_H: u32 = 60;

const MARKDOWN: &str = "![para](para.png)\n\n| a |\n|---|\n| ![cell](cell.png) |\n";

/// Restores the thread's device scale even if an assertion fails.
struct ScaleGuard;
impl Drop for ScaleGuard {
    fn drop(&mut self) {
        crate::set_device_scale(1.0);
    }
}

/// A view of [`MARKDOWN`] whose pictures have arrived as solid red
/// `IMG_W`×`IMG_H` images at `density`, laid out in `size`.
fn view_at_density(density: u32, size: Size) -> MarkdownView {
    crate::widget::set_current_viewport(size);
    let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font"));
    let mut view = MarkdownView::new(MARKDOWN, font);
    view.layout(size);
    let pixels = Arc::new([255u8, 0, 0, 255].repeat((IMG_W * IMG_H) as usize));
    for entry in &view.image_cache {
        *entry.state.lock().expect("image state") = ImageState::Ready {
            image: ImagePixels {
                data: Arc::clone(&pixels),
                width: IMG_W,
                height: IMG_H,
                sequence: None,
                density,
            },
            seen: false,
        };
    }
    view.layout(size);
    view.set_bounds(Rect::new(0.0, 0.0, size.width, size.height));
    view
}

#[test]
fn a_2x_picture_lays_out_at_half_its_pixel_width_at_every_device_scale() {
    let _guard = ScaleGuard;
    for device_scale in [1.0, 2.0] {
        crate::set_device_scale(device_scale);
        let view = view_at_density(2, Size::new(600.0, 600.0));
        let half = f64::from(IMG_W) / 2.0;
        assert_eq!(
            view.image_widths(),
            vec![half, half],
            "device scale {device_scale}"
        );
    }
}

#[test]
fn an_unmarked_picture_keeps_one_pixel_per_point() {
    let _guard = ScaleGuard;
    for device_scale in [1.0, 2.0] {
        crate::set_device_scale(device_scale);
        let view = view_at_density(1, Size::new(600.0, 600.0));
        let full = f64::from(IMG_W);
        assert_eq!(
            view.image_widths(),
            vec![full, full],
            "device scale {device_scale}"
        );
    }
}

#[test]
fn a_2x_picture_still_shrinks_to_fit_a_narrow_column() {
    // 50 points of room: narrower than the picture's 60-point logical width.
    let view = view_at_density(2, Size::new(50.0, 600.0));
    let widths = view.image_widths();
    assert!(
        widths[0] < f64::from(IMG_W) / 2.0 && widths[0] > 0.0,
        "{widths:?}"
    );
}

/// The red run on the image's middle row, in device pixels, painted at
/// `device_scale` the way a shell does (the root scaled by the device scale).
fn painted_width(density: u32, device_scale: f64) -> usize {
    crate::set_device_scale(device_scale);
    let size = Size::new(300.0, 300.0);
    let mut view = view_at_density(density, size);
    let image = view
        .selectable_fragments
        .iter()
        .find(|fragment| matches!(fragment.kind, SelectableKind::Image { .. }))
        .expect("an image fragment")
        .clone();
    let (w, h) = (
        (size.width * device_scale) as u32,
        (size.height * device_scale) as u32,
    );
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, f64::from(w), f64::from(h));
        ctx.fill();
        ctx.scale(device_scale, device_scale);
        view.paint(&mut ctx);
    }
    let row = ((image.y + image.height * 0.5) * device_scale) as usize;
    let pixels = fb.pixels();
    (0..w as usize)
        .filter(|&x| {
            let i = (row * w as usize + x) * 4;
            pixels[i] > 200 && pixels[i + 1] < 60 && pixels[i + 2] < 60
        })
        .count()
}

#[test]
fn a_2x_picture_paints_pixel_for_pixel_on_a_2x_screen() {
    let _guard = ScaleGuard;
    // The paragraph picture's row: one picture wide.
    assert_eq!(painted_width(2, 2.0), IMG_W as usize);
    assert_eq!(painted_width(2, 1.0), IMG_W as usize / 2);
    assert_eq!(painted_width(1, 1.0), IMG_W as usize);
}
