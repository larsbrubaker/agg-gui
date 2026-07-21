//! Tests for the LCD Arc texture cache re-keying (`lcd_arc_get_or_upload` and
//! the pure `lcd_cache_decide` decision it delegates to).
//!
//! Guards two properties of the buffer-address-keyed cache in
//! [`crate::text_render`]:
//!
//! - **Correctness** — an in-place strip edit (`Arc::make_mut`) bumps the
//!   backbuffer's `content_version` while keeping the byte buffer's address
//!   stable, so the cache must re-upload the changed content instead of
//!   returning the stale texture (the wgpu-backend bug this rework fixes).
//! - **Reuse** — reusing the same content (same version, or the same live mask
//!   Arc) must return the *same* GPU texture allocation with no re-upload, and a
//!   version bump must overwrite that same allocation rather than destroy and
//!   recreate it every keystroke.
//!
//! The `lcd_cache_decide` tests are pure and always run.  The tests that touch a
//! real [`WgpuGfxCtx`] are gated on a headless GPU adapter being present and
//! skip (pass trivially) when none is — mirroring `layer_text_readback_tests`.

use std::sync::Arc;

use agg_gui::draw_ctx::DrawCtx;

use crate::text_render::{lcd_cache_decide, LcdCacheAction, LcdEntryMeta};
use crate::WgpuGfxCtx;

// ---------------------------------------------------------------------------
// Pure decision-function coverage (no GPU)
// ---------------------------------------------------------------------------

#[test]
fn decide_missing_entry_replaces() {
    assert_eq!(
        lcd_cache_decide(None, 7, 4, 4, false),
        LcdCacheAction::Replace
    );
    // Even the mask (version 0) path replaces when nothing is cached.
    assert_eq!(
        lcd_cache_decide(None, 0, 4, 4, true),
        LcdCacheAction::Replace
    );
}

#[test]
fn decide_dimension_change_replaces() {
    let entry = Some(LcdEntryMeta {
        version: 7,
        w: 4,
        h: 4,
    });
    // Width change and height change both force a fresh texture, regardless of
    // matching version or Arc identity.
    assert_eq!(
        lcd_cache_decide(entry, 7, 8, 4, true),
        LcdCacheAction::Replace
    );
    assert_eq!(
        lcd_cache_decide(entry, 7, 4, 8, true),
        LcdCacheAction::Replace
    );
}

#[test]
fn decide_backbuffer_version_match_reuses() {
    let entry = Some(LcdEntryMeta {
        version: 42,
        w: 4,
        h: 4,
    });
    // Same version, same dims → reuse, no upload. Arc identity is irrelevant on
    // the versioned path (the Arc relocates every edit).
    assert_eq!(
        lcd_cache_decide(entry, 42, 4, 4, false),
        LcdCacheAction::Reuse
    );
}

#[test]
fn decide_backbuffer_version_bump_rewrites() {
    let entry = Some(LcdEntryMeta {
        version: 42,
        w: 4,
        h: 4,
    });
    // An in-place strip edit bumps the version but keeps the same buffer/dims →
    // overwrite the existing allocation. This is the correctness fix: without
    // it the stale texture would be reused and typed characters never appear.
    assert_eq!(
        lcd_cache_decide(entry, 43, 4, 4, true),
        LcdCacheAction::Rewrite
    );
}

#[test]
fn decide_mask_identity_match_reuses() {
    let entry = Some(LcdEntryMeta {
        version: 0,
        w: 4,
        h: 4,
    });
    // Immutable mask, same live Arc (identity holds) → reuse.
    assert_eq!(
        lcd_cache_decide(entry, 0, 4, 4, true),
        LcdCacheAction::Reuse
    );
}

#[test]
fn decide_mask_aba_recycled_address_rewrites() {
    let entry = Some(LcdEntryMeta {
        version: 0,
        w: 4,
        h: 4,
    });
    // Immutable mask whose buffer address was recycled by a different Arc: the
    // old weak no longer upgrades to this data (identity false) → overwrite the
    // allocation with the new content rather than serve the stale texture.
    assert_eq!(
        lcd_cache_decide(entry, 0, 4, 4, false),
        LcdCacheAction::Rewrite
    );
}

// ---------------------------------------------------------------------------
// Buffer-address stability across `Arc::make_mut` (no GPU)
// ---------------------------------------------------------------------------

/// The whole re-keying scheme rests on this Rust guarantee: with an outstanding
/// `Weak`, `Arc::make_mut` on a uniquely-owned `Arc<Vec<u8>>` *moves* the `Vec`
/// header into a fresh control block but leaves the heap byte buffer in place.
/// If this ever changed, the buffer-address key would move on every edit and the
/// cache would thrash — so pin it down here.
#[test]
fn make_mut_keeps_buffer_address_stable() {
    let mut data = Arc::new(vec![0u8; 48]);
    let addr_before = data.as_ref().as_ptr() as usize;

    // Hold an outstanding Weak so make_mut takes the relocate-not-mutate path
    // (exactly what the GPU cache's stored `weak` does in production).
    let weak = Arc::downgrade(&data);
    let ptr_before = Arc::as_ptr(&data);

    Arc::make_mut(&mut data)[0] = 255;

    let addr_after = data.as_ref().as_ptr() as usize;
    let ptr_after = Arc::as_ptr(&data);

    assert_eq!(
        addr_before, addr_after,
        "Vec buffer address must be stable across make_mut relocation"
    );
    assert_ne!(
        ptr_before, ptr_after,
        "Arc control block should relocate when a Weak is outstanding"
    );
    // The old control block dissociated: the weak no longer sees a strong ref.
    assert_eq!(weak.strong_count(), 0);
}

// ---------------------------------------------------------------------------
// Headless-GPU cache behaviour
// ---------------------------------------------------------------------------

/// A live headless device + queue, or `None` when no adapter is present.
fn try_device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>)> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("lcd-cache-test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((Arc::new(device), Arc::new(queue)))
}

fn test_ctx(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> WgpuGfxCtx {
    let mut ctx = WgpuGfxCtx::new(device, queue, wgpu::TextureFormat::Rgba8Unorm, 64.0, 64.0);
    ctx.reset(64.0, 64.0);
    ctx
}

/// (a) Same buffer + same non-zero version twice → identical texture, no upload.
#[test]
fn gpu_same_buffer_same_version_reuses_texture() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let mut ctx = test_ctx(device, queue);

    let data = Arc::new(vec![50u8; 4 * 4 * 3]);
    let (tex1, _) = ctx.lcd_arc_get_or_upload(&data, 10, 4, 4);
    let (tex2, _) = ctx.lcd_arc_get_or_upload(&data, 10, 4, 4);
    assert!(
        Arc::ptr_eq(&tex1, &tex2),
        "same content version must reuse the exact texture allocation"
    );
}

/// (b) In-place edit (`make_mut`) + bumped version → SAME texture allocation
/// (reused, not recreated) even though the Arc control block relocated. The
/// buffer address stays put, so the cache keys to the same entry.
#[test]
fn gpu_version_bump_after_make_mut_reuses_allocation() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let mut ctx = test_ctx(device, queue);

    let mut data = Arc::new(vec![0u8; 4 * 4 * 3]);
    let (tex1, _) = ctx.lcd_arc_get_or_upload(&data, 100, 4, 4);
    let addr1 = data.as_ref().as_ptr() as usize;

    // Cache now holds a Weak → make_mut relocates the control block, buffer put.
    for b in Arc::make_mut(&mut data).iter_mut() {
        *b = 200;
    }
    let addr2 = data.as_ref().as_ptr() as usize;
    assert_eq!(addr1, addr2, "buffer address must survive the edit");

    let (tex2, _) = ctx.lcd_arc_get_or_upload(&data, 101, 4, 4);
    assert!(
        Arc::ptr_eq(&tex1, &tex2),
        "a version bump must overwrite the existing allocation, not recreate it"
    );
}

/// (c) Different dimensions → a distinct texture allocation.
#[test]
fn gpu_different_dims_new_texture() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let mut ctx = test_ctx(device, queue);

    let small = Arc::new(vec![7u8; 4 * 4 * 3]);
    let big = Arc::new(vec![7u8; 8 * 8 * 3]);
    let (t_small, _) = ctx.lcd_arc_get_or_upload(&small, 20, 4, 4);
    let (t_big, _) = ctx.lcd_arc_get_or_upload(&big, 21, 8, 8);
    assert!(
        !Arc::ptr_eq(&t_small, &t_big),
        "different-sized planes must get their own textures"
    );
}

/// (d) Mask path (version 0): the same live Arc reuses its texture.
#[test]
fn gpu_mask_same_arc_reuses_texture() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let mut ctx = test_ctx(device, queue);

    let mask = Arc::new(vec![9u8; 4 * 4 * 3]);
    let (m1, _) = ctx.lcd_arc_get_or_upload(&mask, 0, 4, 4);
    let (m2, _) = ctx.lcd_arc_get_or_upload(&mask, 0, 4, 4);
    assert!(
        Arc::ptr_eq(&m1, &m2),
        "an unchanged mask Arc must reuse its texture with no upload"
    );
}

// ---------------------------------------------------------------------------
// End-to-end correctness: rendered content must change after an in-place edit
// ---------------------------------------------------------------------------

/// Minimal offscreen render target + CPU readback (top-row-first RGBA8).
struct Target {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    w: u32,
    h: u32,
}

impl Target {
    fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, w: u32, h: u32) -> Self {
        assert_eq!((w * 4) % 256, 0, "width must keep bytes_per_row 256-aligned");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lcd-cache-readback"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            device,
            queue,
            texture,
            view,
            w,
            h,
        }
    }

    fn read(&self) -> Vec<u8> {
        let bpr = self.w * 4;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lcd-cache-readback-buf"),
            size: (bpr * self.h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: Some(self.h),
                },
            },
            wgpu::Extent3d {
                width: self.w,
                height: self.h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(enc.finish()));

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().unwrap().unwrap();
        let data = slice.get_mapped_range().to_vec();
        buffer.unmap();
        data
    }
}

fn center_luma(data: &[u8], w: u32, h: u32) -> u32 {
    let x = w / 2;
    let y = h / 2;
    let i = ((y * w + x) * 4) as usize;
    data[i] as u32 + data[i + 1] as u32 + data[i + 2] as u32
}

/// Reproduces the wgpu correctness bug this rework fixes: after mutating the
/// retained plane in place via `Arc::make_mut` (buffer address unchanged) and
/// bumping the content version, the composited output must reflect the NEW
/// content. Before the fix the pointer-keyed cache returned the stale texture,
/// so the rendered pixels never changed.
#[test]
fn gpu_in_place_edit_changes_rendered_output() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 64u32);
    let plane_w = 32u32;
    let plane_h = 32u32;

    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);
    let mut ctx = test_ctx(Arc::clone(&device), Arc::clone(&queue));

    // Dark, fully-covered plane. Alpha plane at full coverage on all channels.
    let mut color = Arc::new(vec![30u8; (plane_w * plane_h * 3) as usize]);
    let alpha = Arc::new(vec![255u8; (plane_w * plane_h * 3) as usize]);

    // Frame 1 — version 100.
    ctx.reset(w as f32, h as f32);
    ctx.clear(agg_gui::color::Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx.draw_lcd_backbuffer_arc_impl(
        &color, &alpha, 100, plane_w, plane_h, 8.0, 8.0, plane_w as f64, plane_h as f64,
    );
    ctx.flush_to_surface(&target.view);
    let luma_dark = center_luma(&target.read(), w, h);

    // In-place edit to a much brighter plane; buffer address stays put.
    let addr_before = color.as_ref().as_ptr() as usize;
    for b in Arc::make_mut(&mut color).iter_mut() {
        *b = 230;
    }
    assert_eq!(
        addr_before,
        color.as_ref().as_ptr() as usize,
        "buffer address must survive the in-place edit"
    );

    // Frame 2 — bumped version 101, same buffer address.
    ctx.reset(w as f32, h as f32);
    ctx.clear(agg_gui::color::Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx.draw_lcd_backbuffer_arc_impl(
        &color, &alpha, 101, plane_w, plane_h, 8.0, 8.0, plane_w as f64, plane_h as f64,
    );
    ctx.flush_to_surface(&target.view);
    let luma_bright = center_luma(&target.read(), w, h);

    assert!(
        luma_bright > luma_dark + 150,
        "rendered output must brighten after the in-place edit; \
         dark={luma_dark}, bright={luma_bright} (stale-texture bug if ~equal)"
    );
}
