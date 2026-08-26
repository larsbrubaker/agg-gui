//! Tests for the scaled / region capture readback (`screenshot_scaled.rs`).
//!
//! Two layers:
//!
//! * **Pure unit tests** for the region clamp and the row-padding math — these
//!   run everywhere, no GPU needed.
//! * **Headless GPU tests** that drive the real [`WgpuGfxCtx`]: render a known
//!   two-colour scene into an offscreen "surface" texture, run the production
//!   `capture_screenshot` → `begin_capture_readback_scaled` →
//!   `poll_capture_readback_scaled` path, and assert on the returned pixels.
//!   Like `layer_text_readback_tests.rs` they skip (pass trivially) when no
//!   adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;

use crate::layer_text_readback_tests::try_device;
use crate::screenshot_scaled::{clamp_region, padded_bytes_per_row};
use crate::{RectInPixels, WgpuGfxCtx};

// ── Pure math ────────────────────────────────────────────────────────────────

#[test]
fn clamp_region_none_is_full_surface() {
    assert_eq!(
        clamp_region(None, 1280, 720),
        Some(RectInPixels::new(0, 0, 1280, 720))
    );
}

#[test]
fn clamp_region_inside_is_unchanged() {
    let r = RectInPixels::new(10, 20, 100, 50);
    assert_eq!(clamp_region(Some(r), 1280, 720), Some(r));
}

#[test]
fn clamp_region_trims_to_surface_edges() {
    // Overhangs both edges — width/height shrink, origin is kept.
    assert_eq!(
        clamp_region(Some(RectInPixels::new(1200, 700, 500, 500)), 1280, 720),
        Some(RectInPixels::new(1200, 700, 80, 20))
    );
}

#[test]
fn clamp_region_rejects_degenerate_and_offscreen() {
    // Empty extent.
    assert_eq!(
        clamp_region(Some(RectInPixels::new(0, 0, 0, 32)), 128, 128),
        None
    );
    assert_eq!(
        clamp_region(Some(RectInPixels::new(0, 0, 32, 0)), 128, 128),
        None
    );
    // Origin at / past the far edge leaves nothing.
    assert_eq!(
        clamp_region(Some(RectInPixels::new(128, 0, 8, 8)), 128, 128),
        None
    );
    assert_eq!(
        clamp_region(Some(RectInPixels::new(0, 500, 8, 8)), 128, 128),
        None
    );
    // Zero-sized surface.
    assert_eq!(clamp_region(None, 0, 720), None);
}

#[test]
fn padded_bytes_per_row_rounds_up_to_alignment() {
    assert_eq!(padded_bytes_per_row(64), 256); // already aligned
    assert_eq!(padded_bytes_per_row(256), 1024); // already aligned
    assert_eq!(padded_bytes_per_row(1), 256); // 4 -> 256
    assert_eq!(padded_bytes_per_row(100), 512); // 400 -> 512
    assert_eq!(padded_bytes_per_row(192), 768); // 768, exact
    for w in [1u32, 3, 63, 65, 129, 255, 257, 1000] {
        let bpr = padded_bytes_per_row(w);
        assert_eq!(bpr % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
        assert!(bpr >= w * 4);
        assert!(bpr - w * 4 < wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    }
}

// ── Headless GPU path ────────────────────────────────────────────────────────

/// Render "left half red, right half blue" into an offscreen texture that
/// stands in for the surface, then stash it as the ctx's surface texture and
/// snapshot it into the capture texture — i.e. exactly the state a real frame
/// leaves behind before a capture readback.
fn ctx_with_capture(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> WgpuGfxCtx {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scaled-test-surface"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(device),
        Arc::clone(queue),
        format,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);
    ctx.clear(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
    ctx.rect((w / 2) as f64, 0.0, (w / 2) as f64, h as f64);
    ctx.fill();
    ctx.flush_to_surface(&view);

    ctx.set_surface_texture(texture);
    assert!(ctx.capture_screenshot(), "capture_screenshot must succeed");
    ctx
}

/// Spin `poll_capture_readback_scaled` until the map resolves.  The poll is
/// non-blocking by design (that is the whole point of the API), so the test
/// re-polls on a wall-clock deadline rather than assuming any single poll
/// makes the map ready.
fn harvest(ctx: &mut WgpuGfxCtx) -> (Vec<u8>, u32, u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if let Some(out) = ctx.poll_capture_readback_scaled() {
            return out;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("scaled readback never completed");
}

fn px(data: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

/// A full-surface scaled capture downsamples to the requested size and keeps
/// the left/right colour split — proving the UV mapping and the linear
/// minification both work, and that only `dst_w * dst_h * 4` bytes come back.
#[test]
fn scaled_capture_downsamples_full_surface() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 32u32);
    let mut ctx = ctx_with_capture(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, w, h);

    // 8x4 output: bytes_per_row = 32, so the copy is row-padded to 256 —
    // the padding-strip path is exercised here, not just at the full size.
    assert!(!ctx.has_pending_scaled_readback());
    assert!(ctx.begin_capture_readback_scaled(None, 8, 4));
    assert!(ctx.has_pending_scaled_readback());
    // A second request while one is in flight is refused, like the
    // full-surface API.
    assert!(!ctx.begin_capture_readback_scaled(None, 8, 4));

    let (pixels, ow, oh) = harvest(&mut ctx);
    assert_eq!((ow, oh), (8, 4));
    assert_eq!(pixels.len(), 8 * 4 * 4, "output must be tight RGBA8");
    assert!(!ctx.has_pending_scaled_readback());

    for y in 0..oh {
        let left = px(&pixels, ow, 1, y);
        let right = px(&pixels, ow, 6, y);
        assert!(
            left[0] > 200 && left[2] < 55,
            "left half should stay red; got {left:?}"
        );
        assert!(
            right[2] > 200 && right[0] < 55,
            "right half should stay blue; got {right:?}"
        );
        assert_eq!(left[3], 255, "alpha must survive the blit");
    }
}

/// A `src_region` crop reads back only that sub-rect, in framebuffer (top-down)
/// pixels — the whole point of the API for a viewport-sized thumbnail.
#[test]
fn scaled_capture_honors_src_region() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 32u32);
    let mut ctx = ctx_with_capture(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, w, h);

    // Right half only (blue), deliberately overhanging the right edge so the
    // clamp runs too.
    assert!(ctx.begin_capture_readback_scaled(Some(RectInPixels::new(w / 2, 0, w, h)), 8, 4));
    let (pixels, ow, oh) = harvest(&mut ctx);
    assert_eq!((ow, oh), (8, 4));
    for y in 0..oh {
        for x in 0..ow {
            let p = px(&pixels, ow, x, y);
            assert!(
                p[2] > 200 && p[0] < 55,
                "cropped right half must be all blue; got {p:?} at ({x},{y})"
            );
        }
    }
}

/// An empty / off-surface region queues nothing and reports failure, matching
/// how `begin_capture_readback` signals "nothing captured".
#[test]
fn scaled_capture_rejects_degenerate_requests() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 32u32);
    let mut ctx = ctx_with_capture(&device, &queue, wgpu::TextureFormat::Rgba8Unorm, w, h);

    assert!(!ctx.begin_capture_readback_scaled(Some(RectInPixels::new(0, 0, 0, 8)), 8, 4));
    assert!(!ctx.begin_capture_readback_scaled(Some(RectInPixels::new(w, 0, 8, 8)), 8, 4));
    assert!(!ctx.begin_capture_readback_scaled(None, 0, 4));
    assert!(!ctx.begin_capture_readback_scaled(None, 8, 0));
    assert!(
        !ctx.has_pending_scaled_readback(),
        "a rejected request must not queue a readback"
    );
}

/// With no capture texture (nobody called `capture_screenshot`) the call is a
/// no-op, exactly like `begin_capture_readback`.
#[test]
fn scaled_capture_without_capture_texture_is_a_no_op() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        64.0,
        32.0,
    );
    assert!(!ctx.begin_capture_readback_scaled(None, 8, 4));
    assert!(ctx.poll_capture_readback_scaled().is_none());
}

/// Format parity: at 1:1 size on a **BGRA sRGB** surface the scaled path must
/// hand back the same bytes as the existing full-surface readback — same
/// channel order, same sRGB encoding — so a consumer's PNG pipeline needs no
/// change when it switches over.  This is the test that pins the shader's
/// BGRA swizzle and linear→sRGB re-encode.
#[test]
fn scaled_capture_matches_full_readback_bytes_on_srgb_bgra() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 32u32);
    let mut ctx = ctx_with_capture(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb, w, h);

    let (reference, rw, rh) = ctx.read_captured_screenshot_impl();
    assert_eq!((rw, rh), (w, h));

    assert!(ctx.begin_capture_readback_scaled(None, w, h));
    let (scaled, sw, sh) = harvest(&mut ctx);
    assert_eq!((sw, sh), (w, h));
    assert_eq!(scaled.len(), reference.len());

    // Tolerance of 2: the blit round-trips through the sampler's sRGB decode
    // and the shader's re-encode, which is exact only up to 8-bit rounding.
    let mut worst = 0i32;
    for (i, (a, b)) in scaled.iter().zip(reference.iter()).enumerate() {
        let d = (*a as i32 - *b as i32).abs();
        if d > worst {
            worst = d;
        }
        assert!(
            d <= 2,
            "byte {i} differs by {d} (scaled {a} vs full readback {b})"
        );
    }
    assert!(worst <= 2, "worst channel delta {worst}");
}
