//! Regression tests for releasing the per-frame surface handles held by
//! [`WgpuGfxCtx`] (`screenshot_readback.rs`: `release_frame_texture` /
//! `present`).
//!
//! Why this matters: on Windows/DX12 a live clone of a swap-chain back buffer
//! held after `present()` keeps the swap chain referenced, so the next
//! `Surface::configure` (DXGI `ResizeBuffers`) fails with
//! `DXGI_ERROR_INVALID_CALL` ("Invalid surface") and recreating the swap chain
//! fails with `E_ACCESSDENIED`.  The ctx stashes such a clone via
//! `set_surface_texture` for screenshot capture, so it must drop it before the
//! frame is presented.
//!
//! `wgpu::SurfaceTexture` cannot be built headlessly, so `present` itself is
//! covered through `release_frame_texture`, which it calls before
//! `frame.present()`.  Like the other headless GPU tests these skip (pass
//! trivially) when no adapter is available.

use std::sync::Arc;

use agg_gui::draw_ctx::DrawCtx;

use crate::layer_text_readback_tests::try_device;
use crate::WgpuGfxCtx;

/// Offscreen stand-in for a swap-chain frame texture (a real
/// `SurfaceTexture` cannot be created headlessly).
fn frame_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame-release-test-surface"),
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
    })
}

/// After `release_frame_texture` the ctx must hold no handle to the frame's
/// texture: neither the GPU-direct capture nor the CPU read-back can reach it,
/// and the begin-frame surface view is gone too.  A surviving handle is exactly
/// what broke DX12 `ResizeBuffers` after present.
#[test]
fn release_frame_texture_drops_surface_handles_for_dx12_resize() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (16u32, 16u32);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = frame_texture(&device, format, w, h);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        format,
        w as f32,
        h as f32,
    );
    ctx.set_surface_texture(texture);
    ctx.begin_frame(view);

    // Sanity: while stashed, the frame texture is reachable.
    assert!(
        ctx.capture_screenshot(),
        "capture must succeed while the frame texture is stashed"
    );

    // A shell that skips `end_frame` (or presents early) must still end up
    // holding nothing once the frame is released.
    ctx.release_frame_texture();

    assert!(
        ctx.surface_view.is_none(),
        "surface view must be released before present"
    );
    assert!(
        !ctx.capture_screenshot(),
        "capture must fail: the ctx must not hold the frame texture past present"
    );
    let (pixels, rw, rh) = ctx.read_screenshot();
    assert!(
        pixels.is_empty() && rw == 0 && rh == 0,
        "read_screenshot must return an empty buffer once the frame is released"
    );
}

/// Downstream shells we cannot edit may still call a bare
/// `SurfaceTexture::present()`, leaving the previous frame's texture stashed
/// when the next frame calls `set_surface_texture`.  That is the DX12 resize
/// hazard, so the ctx must notice and (once) warn.  A shell that releases
/// between frames must never trip it.
#[test]
fn set_surface_texture_warns_once_when_previous_frame_was_not_released() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let new_ctx = || WgpuGfxCtx::new(Arc::clone(&device), Arc::clone(&queue), format, 16.0, 16.0);

    // Correct shell: stash, release (what `present` does), stash.
    let mut good = new_ctx();
    good.set_surface_texture(frame_texture(&device, format, 16, 16));
    good.release_frame_texture();
    good.set_surface_texture(frame_texture(&device, format, 16, 16));
    assert!(
        !good.warned_unreleased_frame,
        "a released frame must not trigger the unreleased-frame warning"
    );

    // Misbehaving shell: stash twice with no release in between.
    let mut bad = new_ctx();
    bad.set_surface_texture(frame_texture(&device, format, 16, 16));
    assert!(!bad.warned_unreleased_frame, "first stash is always fine");
    bad.set_surface_texture(frame_texture(&device, format, 16, 16));
    assert!(
        bad.warned_unreleased_frame,
        "stashing over an unreleased frame texture must warn"
    );
    // Stays set (one warning per ctx) on further unreleased frames.
    bad.set_surface_texture(frame_texture(&device, format, 16, 16));
    assert!(bad.warned_unreleased_frame);
}
