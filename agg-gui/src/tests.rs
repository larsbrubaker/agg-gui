//! Coordinate system invariant tests.
//!
//! These tests guard the first-quadrant (Y-up) invariant at the framebuffer
//! and GfxCtx layers. They run on every commit.

use crate::{Color, Framebuffer, GfxCtx};

/// Sample RGBA at pixel (x, y) in a framebuffer.
/// (x=0, y=0) is the bottom-left corner in Y-up space.
fn sample(fb: &Framebuffer, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * fb.width() + x) * 4) as usize;
    let p = fb.pixels();
    [p[idx], p[idx + 1], p[idx + 2], p[idx + 3]]
}

fn is_white(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] > 200 && pixel[2] > 200
}

fn is_red(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] < 50 && pixel[2] < 50
}

fn is_dark(pixel: [u8; 4]) -> bool {
    pixel[0] < 50 && pixel[1] < 50 && pixel[2] < 50
}

/// A point drawn at Y=10 in a 100×100 buffer must be near the BOTTOM of the
/// buffer (low row index), not the top. This verifies the Y-up invariant at
/// the framebuffer level.
#[test]
fn test_y_up_point_at_bottom() {
    let mut fb = Framebuffer::new(100, 100);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    // Draw a white circle at (50, 10) — near the bottom in Y-up space.
    ctx.set_fill_color(Color::white());
    ctx.begin_path();
    ctx.circle(50.0, 10.0, 5.0);
    ctx.fill();
    drop(ctx);

    // Row 10 (from buffer start) = Y=10 = near the BOTTOM of the window.
    // The circle center pixel should be white.
    let center = sample(&fb, 50, 10);
    assert!(is_white(center), "Y=10 should be near the bottom of the buffer (Y-up); got {center:?}");

    // The top (Y=90) should still be dark — we only drew near the bottom.
    let top_center = sample(&fb, 50, 90);
    assert!(is_dark(top_center), "Y=90 should be dark (nothing drawn there); got {top_center:?}");
}

/// A CCW rotation of +90° rotates a right-pointing vector to point upward.
/// In Y-up space this means +X → +Y when rotating by +π/2.
///
/// We draw a narrow horizontal bar (pointing right), rotate +90°, and verify
/// that after rotation it's a vertical bar pointing upward.
#[test]
fn test_rotation_ccw_positive() {
    let size = 200u32;
    let mut fb = Framebuffer::new(size, size);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    let cx = size as f64 / 2.0;
    let cy = size as f64 / 2.0;

    // Rotate +90° (CCW) around the center.
    // A horizontal bar at Y=cy becomes a vertical bar extending upward (+Y).
    ctx.translate(cx, cy);
    ctx.rotate(std::f64::consts::FRAC_PI_2); // +90°

    // Draw a horizontal bar: spans x ∈ [10, 50], y ∈ [-3, 3]
    // After +90° rotation in Y-up: x → -y, y → x
    // So the bar covers x ∈ [-3, 3], y ∈ [10, 50] in screen space.
    ctx.set_fill_color(Color::white());
    ctx.begin_path();
    ctx.rect(10.0, -3.0, 40.0, 6.0);
    ctx.fill();
    drop(ctx);

    // After rotation, the bar should be vertical, running UPWARD from center.
    // Check a pixel above center (x=cx, y=cy+25) — should be white.
    let above_center = sample(&fb, cx as u32, cy as u32 + 25);
    assert!(is_white(above_center), "+90° CCW rotation should produce upward bar; pixel above center is {above_center:?}");

    // A pixel to the RIGHT of center (x=cx+25, y=cy) should be dark —
    // the bar was rotated away from horizontal.
    let right_of_center = sample(&fb, cx as u32 + 25, cy as u32);
    assert!(is_dark(right_of_center), "After +90° rotation, horizontal should be gone; pixel to right is {right_of_center:?}");
}

/// A point drawn at (10, 10) in Y-up space is near the bottom-left corner
/// and should appear in the first few rows and columns of the raw pixel buffer.
#[test]
fn test_bottom_left_origin() {
    let mut fb = Framebuffer::new(200, 200);
    let mut ctx = GfxCtx::new(&mut fb);
    ctx.clear(Color::black());

    ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0)); // red
    ctx.begin_path();
    ctx.circle(10.0, 10.0, 6.0);
    ctx.fill();
    drop(ctx);

    // The center at (10, 10) in Y-up = row 10 from buffer start = near BOTTOM.
    let center = sample(&fb, 10, 10);
    assert!(is_red(center), "Bottom-left origin test: (10,10) should be red; got {center:?}");

    // The top-right region should be dark.
    let top_right = sample(&fb, 190, 190);
    assert!(is_dark(top_right), "Top-right should be empty; got {top_right:?}");
}

/// `pixels_flipped()` should reverse the row order so that Y=0 (bottom) ends
/// up at the top of the returned buffer — correct for HTML Canvas `putImageData`.
#[test]
fn test_pixels_flipped_reversal() {
    let w = 4u32;
    let h = 4u32;
    let mut fb = Framebuffer::new(w, h);

    // Paint row Y=0 (bottom) red and row Y=3 (top) blue.
    {
        let pixels = fb.pixels_mut();
        // Row 0 (Y=0, buffer start) → red
        for x in 0..w as usize {
            let i = x * 4;
            pixels[i] = 255; pixels[i+1] = 0; pixels[i+2] = 0; pixels[i+3] = 255;
        }
        // Row 3 (Y=3, buffer end) → blue
        let base = 3 * w as usize * 4;
        for x in 0..w as usize {
            let i = base + x * 4;
            pixels[i] = 0; pixels[i+1] = 0; pixels[i+2] = 255; pixels[i+3] = 255;
        }
    }

    let flipped = fb.pixels_flipped();

    // In the flipped buffer, the first row (screen top) should be what was
    // row 3 (Y=3, top in Y-up = top on screen) → blue.
    assert_eq!(&flipped[0..4], &[0u8, 0, 255, 255], "Flipped[0] should be blue (was Y=3 = top)");

    // The last row of the flipped buffer (screen bottom) should be red (was Y=0).
    let last_row_start = (h as usize - 1) * w as usize * 4;
    assert_eq!(&flipped[last_row_start..last_row_start+4], &[255u8, 0, 0, 255], "Flipped last row should be red (was Y=0 = bottom)");
}
