//! Cross-backend pixel tests for radial gradients: the wgpu gradient shader
//! (`shaders.rs` `GRADIENT_WGSL`, uniforms from `gradient.rs`) against the
//! software `GfxCtx`, whose pixels come from `RadialGradientPaint::sample`
//! (agg-gui `paints.rs`). Covers the classic focal solve (focus inside), the
//! focal cone (focus on and outside the circle, transparent where no circle
//! reaches) and SVG 2's `fr`.
//!
//! Every pixel of a full-surface fill is compared, so there are no edge
//! coverage differences; what remains is the shader's f32 arithmetic and its
//! 256-entry ramp texture against the software path's f64 sample.
//!
//! Skipped (pass trivially) when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, GradientSpread, RadialGradientPaint};
use agg_gui::framebuffer::Framebuffer;
use agg_gui::gfx_ctx::GfxCtx;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::WgpuGfxCtx;

const SIZE: u32 = 64;

/// Per-channel tolerance in byte levels: one ramp texel of a red-to-blue
/// stop span plus rounding.
const TOLERANCE: i32 = 2;

/// An opaque background that differs from every stop color, so an
/// unpainted (transparent) region and a painted one can't be confused.
fn background() -> Color {
    Color::from_rgb8(40, 200, 60)
}

fn gradient(c: (f64, f64), r: f64, f: (f64, f64), fr: f64) -> RadialGradientPaint {
    let mut g = RadialGradientPaint::centered(
        c.0,
        c.1,
        r,
        &[
            (0.0, Color::from_rgb8(255, 0, 0)),
            (0.5, Color::from_rgb8(255, 255, 255)),
            (1.0, Color::from_rgb8(0, 0, 255)),
        ],
    );
    g.fx = f.0;
    g.fy = f.1;
    g.fr = fr;
    g.spread = GradientSpread::Pad;
    g
}

fn fill_surface(ctx: &mut dyn DrawCtx, g: &RadialGradientPaint) {
    ctx.clear(background());
    ctx.set_fill_radial_gradient(g.clone());
    ctx.begin_path();
    ctx.rect(0.0, 0.0, SIZE as f64, SIZE as f64);
    ctx.fill();
}

fn software(g: &RadialGradientPaint) -> Framebuffer {
    let mut fb = Framebuffer::new(SIZE, SIZE);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        fill_surface(&mut ctx, g);
    }
    fb
}

fn gpu(g: &RadialGradientPaint) -> Option<Vec<u8>> {
    let (device, queue) = try_device()?;
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), SIZE, SIZE);
    let mut ctx = WgpuGfxCtx::new(
        device,
        queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE as f32,
        SIZE as f32,
    );
    ctx.reset(SIZE as f32, SIZE as f32);
    fill_surface(&mut ctx, g);
    ctx.flush_to_surface(&target.view);
    Some(target.read())
}

/// Renders `g` both ways and requires every color channel to agree within
/// [`TOLERANCE`]. `expect_unpainted` additionally requires the software
/// render to leave some background showing (the cone's unreached region), so
/// the comparison covers the transparent branch too.
fn assert_backends_match(g: &RadialGradientPaint, expect_unpainted: bool, what: &str) {
    let expected = software(g);
    let Some(got) = gpu(g) else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let pixels = expected.pixels();
    let bg = background().to_rgba8();
    let mut unpainted = 0;
    let mut max_diff = 0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = ((y * SIZE + x) * 4) as usize;
            let want = &pixels[i..i + 3];
            if want == [bg.r, bg.g, bg.b] {
                unpainted += 1;
            }
            // Readback is top-row-first; the framebuffer is Y-up.
            let have = px(&got, SIZE, x, SIZE - 1 - y);
            for c in 0..3 {
                let diff = (have[c] as i32 - want[c] as i32).abs();
                max_diff = max_diff.max(diff);
                assert!(
                    diff <= TOLERANCE,
                    "{what} at {x}, {y} channel {c}: GPU {have:?} against software {want:?}"
                );
            }
        }
    }
    eprintln!("{what}: max software-vs-GPU channel difference {max_diff}");
    assert_eq!(
        unpainted > 0,
        expect_unpainted,
        "{what}: {unpainted} software pixels show the background"
    );
}

#[test]
fn radial_focus_inside_matches_software() {
    let g = gradient((32.0, 32.0), 24.0, (40.0, 36.0), 0.0);
    assert!(!g.uses_focal_cone());
    assert_backends_match(&g, false, "focus inside");
}

#[test]
fn radial_focus_on_circle_matches_software() {
    let g = gradient((32.0, 32.0), 20.0, (52.0, 32.0), 0.0);
    assert_eq!(g.focal_cone_coefficient(), Some((0.0, true)));
    assert_backends_match(&g, true, "focus on the circle");
}

#[test]
fn radial_focus_outside_matches_software() {
    let g = gradient((40.0, 32.0), 14.0, (8.0, 30.0), 0.0);
    assert!(matches!(g.focal_cone_coefficient(), Some((_, false))));
    assert_backends_match(&g, true, "focus outside");
}

#[test]
fn radial_focal_radius_matches_software() {
    let g = gradient((32.0, 32.0), 26.0, (24.0, 30.0), 6.0);
    assert!(matches!(g.focal_cone_coefficient(), Some((_, false))));
    assert_backends_match(&g, false, "fr > 0");
}

#[test]
fn radial_focal_radius_with_focus_outside_matches_software() {
    let g = gradient((44.0, 32.0), 12.0, (10.0, 32.0), 4.0);
    assert!(matches!(g.focal_cone_coefficient(), Some((_, false))));
    assert_backends_match(&g, true, "fr > 0, focus outside");
}
