//! `DrawCtx::root_transform` inside offscreen layers, checked against real
//! pixels on the software backends (`GfxCtx` and `LcdGfxCtx`).
//!
//! The contract: `root_transform()` maps a widget-local point to the ROOT
//! render target's device pixels (Y-up), no matter how many compositing or
//! clip layers the caller is nested in. Each layer records its origin in its
//! parent's device pixels, so the composition is "apply the in-layer CTM, then
//! add every enclosing layer's origin" — the origins must NOT be fed through
//! the in-layer scale. At scale 1 the two orders agree, which is why every
//! scenario here runs under a HiDPI-style `scale(2, 2)`.
//!
//! Each test draws an opaque red rect at a known local position inside the
//! layer(s), captures `root_transform()` at the same point, pops back to the
//! root, finds where the red actually landed in the root buffer, and asserts
//! the captured transform maps the local rect onto those pixels. The widget
//! paint-clip helpers (`crate::widget::paint`) depend on this; the end-to-end
//! widget check lives in `root_transform_paint_clip.rs`, and the wgpu
//! counterpart in `agg-gui-wgpu/src/root_transform_readback_tests.rs`.

use super::*;
use crate::draw_ctx::DrawCtx;
use crate::lcd_coverage::LcdBuffer;
use crate::lcd_gfx_ctx::LcdGfxCtx;
use crate::TransAffine;

/// Inclusive Y-up pixel bounding box `(min_x, min_y, max_x, max_y)`.
type PixelBox = (u32, u32, u32, u32);

/// Bounding box of every pixel `hit` accepts, or `None` if none does.
fn bbox_where(w: u32, h: u32, hit: impl Fn(u32, u32) -> bool) -> Option<PixelBox> {
    let mut found: Option<PixelBox> = None;
    for y in 0..h {
        for x in 0..w {
            if hit(x, y) {
                found = Some(match found {
                    None => (x, y, x, y),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                });
            }
        }
    }
    found
}

fn red_box_rgba(fb: &Framebuffer) -> Option<PixelBox> {
    bbox_where(fb.width(), fb.height(), |x, y| is_red(sample(fb, x, y)))
}

/// Red box in an `LcdBuffer` (3 bytes per pixel, row 0 = bottom).
fn red_box_lcd(buf: &LcdBuffer) -> Option<PixelBox> {
    let w = buf.width();
    let color = buf.color_plane();
    bbox_where(w, buf.height(), |x, y| {
        let i = ((y * w + x) * 3) as usize;
        color[i] > 200 && color[i + 1] < 50 && color[i + 2] < 50
    })
}

/// Assert `rt` maps the local rect `(x, y, w, h)` onto the device pixels it
/// actually rasterised to (`found`), within one pixel on every edge.
fn assert_maps_onto_pixels(
    label: &str,
    rt: &TransAffine,
    local: (f64, f64, f64, f64),
    found: Option<PixelBox>,
) {
    let Some((px0, py0, px1, py1)) = found else {
        panic!("{label}: the red probe rect never reached the root buffer");
    };
    let (x, y, w, h) = local;
    let (mut ax, mut ay) = (x, y);
    let (mut bx, mut by) = (x + w, y + h);
    rt.transform(&mut ax, &mut ay);
    rt.transform(&mut bx, &mut by);
    let mapped = (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by));
    // The pixel box is inclusive; its device-space span ends one past it.
    let actual = (px0 as f64, py0 as f64, (px1 + 1) as f64, (py1 + 1) as f64);
    let close = (mapped.0 - actual.0).abs() <= 1.0
        && (mapped.1 - actual.1).abs() <= 1.0
        && (mapped.2 - actual.2).abs() <= 1.0
        && (mapped.3 - actual.3).abs() <= 1.0;
    assert!(
        close,
        "{label}: root_transform maps local {local:?} to device \
         [{:.1}, {:.1}]-[{:.1}, {:.1}], but the rect landed at device \
         [{}, {}]-[{}, {}] in the root buffer (rt = {rt:?})",
        mapped.0, mapped.1, mapped.2, mapped.3, actual.0, actual.1, actual.2, actual.3,
    );
}

fn fill_red(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(x, y, w, h);
    ctx.fill();
}

/// Paint `scenario` into a fresh white 200×200 `GfxCtx` target through the
/// `DrawCtx` trait; return the transform it captured and where red landed.
fn run_gfx(
    scenario: impl FnOnce(&mut dyn DrawCtx) -> TransAffine,
) -> (TransAffine, Option<PixelBox>) {
    let mut fb = Framebuffer::new(200, 200);
    let rt = {
        let mut gfx = GfxCtx::new(&mut fb);
        let ctx: &mut dyn DrawCtx = &mut gfx;
        ctx.clear(Color::white());
        scenario(ctx)
    };
    (rt, red_box_rgba(&fb))
}

/// `LcdGfxCtx` twin of [`run_gfx`].
fn run_lcd(
    scenario: impl FnOnce(&mut dyn DrawCtx) -> TransAffine,
) -> (TransAffine, Option<PixelBox>) {
    let mut buf = LcdBuffer::new(200, 200);
    let rt = {
        let mut lcd = LcdGfxCtx::new(&mut buf);
        let ctx: &mut dyn DrawCtx = &mut lcd;
        ctx.clear(Color::white());
        scenario(ctx)
    };
    (rt, red_box_lcd(&buf))
}

// ---------------------------------------------------------------------------
// GfxCtx — `push_layer` keeps the CTM scale inside the layer
// ---------------------------------------------------------------------------

/// Device scale 2 plus a logical offset puts the layer origin at device
/// (20, 30); inside, the local CTM is `scale(2)` then a further translate.
/// The rect lands at device x ∈ [36, 44), y ∈ [52, 60).
#[test]
fn gfx_push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (3.0, 4.0, 4.0, 4.0);
    let (rt, found) = run_gfx(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(60.0, 60.0);
        ctx.translate(5.0, 7.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.pop_layer();
        rt
    });
    assert_maps_onto_pixels("GfxCtx push_layer", &rt, local, found);
}

/// Two nested layers: the inner origin is recorded in the outer layer's
/// pixels, so the root position is the in-layer CTM plus both origins.
#[test]
fn gfx_nested_push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (1.0, 1.0, 5.0, 5.0);
    let (rt, found) = run_gfx(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(80.0, 80.0);
        ctx.translate(6.0, 4.0);
        ctx.push_layer(40.0, 40.0);
        ctx.translate(2.0, 3.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.pop_layer();
        ctx.pop_layer();
        rt
    });
    assert_maps_onto_pixels("GfxCtx nested push_layer", &rt, local, found);
}

/// A `clip_path` layer keeps the parent CTM shifted by the layer origin; the
/// path's device bounds start at (26, 34), off the CTM's own translation, so
/// the in-layer CTM carries a non-zero (negative) local translation too.
#[test]
fn gfx_clip_path_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (5.0, 6.0, 4.0, 4.0);
    let (rt, found) = run_gfx(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.save();
        ctx.begin_path();
        ctx.rect(3.0, 2.0, 30.0, 30.0);
        ctx.clip_path();
        ctx.translate(4.0, 5.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.restore();
        rt
    });
    assert_maps_onto_pixels("GfxCtx clip_path layer", &rt, local, found);
}

/// Mixed nesting: a `clip_path` layer inside a `push_layer`.
#[test]
fn gfx_clip_path_inside_push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (1.0, 2.0, 4.0, 4.0);
    let (rt, found) = run_gfx(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(80.0, 80.0);
        ctx.translate(3.0, 2.0);
        ctx.save();
        ctx.begin_path();
        ctx.rect(1.0, 1.0, 20.0, 20.0);
        ctx.clip_path();
        ctx.translate(2.0, 2.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.restore();
        ctx.pop_layer();
        rt
    });
    assert_maps_onto_pixels("GfxCtx clip_path inside push_layer", &rt, local, found);
}

// ---------------------------------------------------------------------------
// LcdGfxCtx — `push_layer` resets the CTM to identity, so the scenarios apply
// a scale inside the layer to give the local CTM a non-unit scale. (Its
// `clip_path` is the trait's no-op default and pushes no layer.)
// ---------------------------------------------------------------------------

#[test]
fn lcd_push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (3.0, 4.0, 4.0, 4.0);
    let (rt, found) = run_lcd(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(60.0, 60.0);
        ctx.scale(2.0, 2.0);
        ctx.translate(5.0, 7.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.pop_layer();
        rt
    });
    assert_maps_onto_pixels("LcdGfxCtx push_layer", &rt, local, found);
}

#[test]
fn lcd_nested_push_layer_root_transform_matches_pixels_at_scale_2() {
    let local = (1.0, 1.0, 5.0, 5.0);
    let (rt, found) = run_lcd(|ctx| {
        ctx.scale(2.0, 2.0);
        ctx.translate(10.0, 15.0);
        ctx.push_layer(100.0, 100.0);
        ctx.scale(2.0, 2.0);
        ctx.translate(6.0, 4.0);
        ctx.push_layer(40.0, 40.0);
        ctx.scale(2.0, 2.0);
        ctx.translate(2.0, 3.0);
        let rt = ctx.root_transform();
        fill_red(ctx, local.0, local.1, local.2, local.3);
        ctx.pop_layer();
        ctx.pop_layer();
        rt
    });
    assert_maps_onto_pixels("LcdGfxCtx nested push_layer", &rt, local, found);
}
