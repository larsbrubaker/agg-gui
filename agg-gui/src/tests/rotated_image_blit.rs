//! The software `GfxCtx` blits an image under a rotated CTM as a rotated
//! quad, as the wgpu backend does and as agg-sharp's
//! `Graphics2D.Render(image, x, y, angle, scaleX, scaleY)` does. Found by the
//! ported `Golden2DPrimitiveTests.ImageBlits` (agg-gui-wgpu `golden/`), which
//! also pins where a blit at a fractional position starts.

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::framebuffer::Framebuffer;
use crate::gfx_ctx::GfxCtx;

#[test]
fn rust_only_software_image_blit_follows_a_rotated_transform() {
    let red = [255u8, 0, 0, 255].repeat(100);
    let mut fb = Framebuffer::new(48, 48);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        ctx.translate(20.0, 10.0);
        ctx.rotate(std::f64::consts::FRAC_PI_4);
        ctx.draw_image_rgba(&red, 10, 10, 0.0, 0.0, 10.0, 10.0);
    }
    // Y-up framebuffer rows: the pixel at (x, y) is row y.
    let px = |x: u32, y: u32| {
        let i = ((y * 48 + x) * 4) as usize;
        [fb.pixels()[i], fb.pixels()[i + 1], fb.pixels()[i + 2]]
    };
    // The square stands on its corner at (20, 10); its top corner is at
    // (20, 24.1), its side corners at (12.9, 17.1) and (27.1, 17.1).
    assert_eq!(px(20, 22), [255, 0, 0], "inside, near the top corner");
    assert_eq!(px(14, 17), [255, 0, 0], "inside, near the left corner");
    // An axis-aligned blit at the transformed origin would cover these.
    assert_eq!(
        px(28, 11),
        [255, 255, 255],
        "outside, right of the bottom corner"
    );
    assert_eq!(
        px(21, 10),
        [255, 255, 255],
        "outside, beside the bottom corner"
    );
}

#[test]
fn rust_only_software_image_blit_at_a_half_pixel_covers_the_pixel_on_its_edge() {
    let red = [255u8, 0, 0, 255].repeat(16);
    let mut fb = Framebuffer::new(16, 16);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        // Covers x in [4.5, 8.5): pixel centres 4.5 to 7.5, pixels 4 to 7.
        ctx.draw_image_rgba(&red, 4, 4, 4.5, 4.0, 4.0, 4.0);
    }
    let red_at = |x: u32| fb.pixels()[((5 * 16 + x) * 4) as usize + 1] == 0;
    assert!(
        red_at(4),
        "the pixel whose centre is on the left edge is covered"
    );
    assert!(red_at(7), "the last pixel centre inside is covered");
    assert!(!red_at(8), "the pixel past the right edge is not");
}
