//! Software-renderer `clip_path` tests.
//!
//! `GfxCtx::clip_path` (see `gfx_ctx/layers.rs`) pushes a masked compositing
//! layer that lives until the `restore()` matching the preceding `save()`.
//! These tests drive it through the `DrawCtx` trait and read pixels straight
//! out of the framebuffer.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::TransAffine;

fn is_blue(pixel: [u8; 4]) -> bool {
    pixel[2] > 200 && pixel[0] < 50 && pixel[1] < 50
}

/// Red background, blue fill clipped to a centred circle.
///
/// Drives the context through `&mut dyn DrawCtx` so the trait plumbing
/// (`DrawCtx::clip_path`) is covered, not just the inherent method.
fn clipped_circle_fb() -> Framebuffer {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        assert!(ctx.supports_clip_path());
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.restore();
    }
    fb
}

#[test]
fn clip_path_masks_fill_to_the_circle() {
    let fb = clipped_circle_fb();
    assert!(is_blue(sample(&fb, 32, 32)), "centre must be blue");
    assert!(is_red(sample(&fb, 1, 1)), "corner must stay red");
    // Just past the circle edge along +x.
    assert!(
        is_red(sample(&fb, 52, 32)),
        "outside the circle must be red"
    );
}

#[test]
fn clip_is_released_after_restore() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.restore();
        // Clip released: this fill reaches the corner.
        ctx.set_fill_color(Color::rgba(0.0, 1.0, 0.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
    }
    let corner = sample(&fb, 1, 1);
    assert!(
        corner[1] > 200 && corner[0] < 50,
        "corner must be green after restore; got {corner:?}"
    );
}

#[test]
fn clip_path_respects_translate_and_rotate() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.translate(32.0, 32.0);
        ctx.rotate(std::f64::consts::FRAC_PI_4);
        // A 20×20 square centred on the origin, rotated 45° → a diamond.
        ctx.begin_path();
        ctx.rect(-10.0, -10.0, 20.0, 20.0);
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(-100.0, -100.0, 200.0, 200.0);
        ctx.fill();
        ctx.restore();
    }
    // Diamond half-diagonal ≈ 14.1: (32, 32) inside, (32 ± 12, 32 ± 12) outside
    // (a corner of the unrotated square).
    assert!(is_blue(sample(&fb, 32, 32)), "centre must be blue");
    assert!(is_blue(sample(&fb, 32, 43)), "on-axis point must be blue");
    assert!(
        is_red(sample(&fb, 44, 44)),
        "the rotated square's diagonal corner must stay red"
    );
}

#[test]
fn nested_clip_paths_intersect() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 32.0, 64.0); // left half
        ctx.clip_path();
        ctx.save();
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 32.0); // bottom half
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.restore();
        ctx.restore();
    }
    assert!(is_blue(sample(&fb, 16, 16)), "bottom-left must be blue");
    assert!(is_red(sample(&fb, 48, 16)), "bottom-right must be red");
    assert!(is_red(sample(&fb, 16, 48)), "top-left must be red");
    assert!(is_red(sample(&fb, 48, 48)), "top-right must be red");
}

#[test]
fn empty_path_clip_draws_nothing() {
    let mut fb = Framebuffer::new(32, 32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.begin_path();
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 32.0, 32.0);
        ctx.fill();
        ctx.restore();
    }
    assert!(is_red(sample(&fb, 16, 16)), "empty clip must draw nothing");
}

fn is_green(pixel: [u8; 4]) -> bool {
    pixel[1] > 200 && pixel[0] < 60 && pixel[2] < 60
}

/// Canvas `clip()` leaves the current path intact, so the common port pattern
/// `begin_path(); <outline>; fill(); save(); clip_path(); <interior>;
/// restore(); stroke();` strokes the outline it clipped with.
#[test]
fn clip_path_keeps_the_current_path_for_a_later_stroke() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 1.0, 1.0, 1.0));
        ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.fill();
        ctx.save();
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.restore();
        // The circle must still be the current path here.
        ctx.set_stroke_color(Color::rgba(0.0, 1.0, 0.0, 1.0));
        ctx.set_line_width(6.0);
        ctx.stroke();
    }
    let edge = sample(&fb, 48, 32);
    assert!(
        is_green(edge),
        "stroke must follow the clipping circle; got {edge:?}"
    );
    assert!(
        is_blue(sample(&fb, 32, 32)),
        "centre must stay blue; got {:?}",
        sample(&fb, 32, 32)
    );
}

/// A balanced `save()`/`restore()` pair *inside* a clip layer must not pop the
/// clip: drawing after the inner restore is still clipped.
#[test]
fn balanced_save_restore_inside_clip_does_not_pop_it() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        ctx.save();
        ctx.set_fill_color(Color::rgba(0.0, 1.0, 0.0, 1.0));
        ctx.restore();
        // Still inside the clip layer.
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.restore();
    }
    assert!(is_blue(sample(&fb, 32, 32)), "centre must be blue");
    assert!(
        is_red(sample(&fb, 1, 1)),
        "the clip must still be active after the inner restore; got {:?}",
        sample(&fb, 1, 1)
    );
}

#[test]
fn clip_path_inside_a_compositing_layer() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.push_layer(64.0, 64.0);
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.restore();
        ctx.pop_layer();
    }
    assert!(is_blue(sample(&fb, 32, 32)), "centre must be blue");
    assert!(is_red(sample(&fb, 1, 1)), "corner must stay red");
}

#[test]
fn compositing_layer_inside_a_clip_path() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.begin_path();
        ctx.circle(32.0, 32.0, 16.0);
        ctx.clip_path();
        ctx.push_layer(64.0, 64.0);
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.pop_layer();
        ctx.restore();
    }
    assert!(is_blue(sample(&fb, 32, 32)), "centre must be blue");
    assert!(is_red(sample(&fb, 1, 1)), "corner must stay red");
}

/// Under a scaling CTM the mask is rasterised in physical pixels, so a
/// logical-space circle clips the physical pixels it actually covers.
#[test]
fn clip_path_under_a_scaled_ctm() {
    let mut fb = Framebuffer::new(64, 64);
    {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
        ctx.save();
        ctx.set_transform(TransAffine::new_scaling(2.0, 2.0));
        // Logical circle (16, 16) r 8 → physical (32, 32) r 16.
        ctx.begin_path();
        ctx.circle(16.0, 16.0, 8.0);
        ctx.clip_path();
        ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 64.0, 64.0);
        ctx.fill();
        ctx.restore();
    }
    assert!(is_blue(sample(&fb, 32, 32)), "physical centre must be blue");
    assert!(
        is_blue(sample(&fb, 32, 44)),
        "physical radius 12 must be inside; got {:?}",
        sample(&fb, 32, 44)
    );
    assert!(
        is_red(sample(&fb, 32, 52)),
        "physical radius 20 must be outside; got {:?}",
        sample(&fb, 32, 52)
    );
    assert!(is_red(sample(&fb, 1, 1)), "corner must stay red");
}

/// The clip edge is anti-aliased: some pixel straddling the circle boundary
/// blends between the clipped fill and the background instead of snapping to
/// one of them.
#[test]
fn clip_path_edge_is_anti_aliased() {
    let fb = clipped_circle_fb();
    // Walk outward along +x through the boundary at x = 48.
    let blended = (44..54)
        .map(|x| sample(&fb, x, 32))
        .find(|p| p[0] > 0 && p[0] < 255 && p[2] > 0 && p[2] < 255);
    assert!(
        blended.is_some(),
        "no anti-aliased pixel on the clip edge: {:?}",
        (44..54).map(|x| sample(&fb, x, 32)).collect::<Vec<_>>()
    );
}
