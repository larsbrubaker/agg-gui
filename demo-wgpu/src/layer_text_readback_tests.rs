//! Headless GPU readback tests for text / backbuffer compositing INSIDE a
//! transparent layer, plus the layer-composite clip.
//!
//! These render through the real [`WgpuGfxCtx`] deferred-command pipeline onto
//! an offscreen texture, copy the pixels back to system memory, and assert on
//! them.  They guard the wgpu counterparts of the software-side contracts in
//! `agg-gui/src/tests/layer_compositing.rs`:
//!
//! - **Fix (2)** — LCD subpixel text inside a compositing layer washes out to
//!   white on the 3-pass write-mask path (that path never writes alpha, so the
//!   transparent layer keeps alpha=0 and the pop composite blends additively).
//!   Inside a layer we route text through the single-pass, alpha-writing
//!   grayscale / flatten pipelines instead.  The test proves black text in a
//!   0.5 layer lands at mid-gray with no channel fringing.
//! - **Fix (1)** — the two-plane `LcbMask` blit folds `global_alpha`.
//! - **Layer clip** — the pop composite honors the scissor active at push time.
//!
//! The tests are skipped (pass trivially) when no GPU adapter is available so
//! CI on a headless-without-GPU box does not spuriously fail; on a machine with
//! a working adapter they run for real.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::text::Font;

use crate::WgpuGfxCtx;

const TEST_FONT: &[u8] = include_bytes!("../../demo/assets/CascadiaCode.ttf");

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
        label: Some("readback-test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((Arc::new(device), Arc::new(queue)))
}

/// Offscreen render target + CPU readback.  Width is chosen a multiple of 64 so
/// `bytes_per_row = w*4` is already 256-aligned (no padding math needed).
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
            label: Some("readback-target"),
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

    /// Copy the rendered target back to a top-row-first RGBA8 `Vec`.
    fn read(&self) -> Vec<u8> {
        let bpr = self.w * 4;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback-buf"),
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

/// Fetch pixel `(x, y)` — the readback is top-row-first (y=0 is the visual top).
fn px(data: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).unwrap())
}

/// Fix (2): black LCD text inside a 0.5 alpha layer over white must land at
/// mid-gray with no channel fringing — never a washed-out lighter-than-mid or
/// a coloured fringe (the 3-pass-no-alpha failure mode).
#[test]
fn lcd_text_in_alpha_layer_is_gray_not_washed_out() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 32u32);
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);

    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);
    ctx.set_lcd_mode(true);

    ctx.clear(Color::white());
    ctx.set_font(font());
    ctx.set_font_size(22.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 1.0));
    ctx.push_layer_with_alpha(w as f64, h as f64, 0.5);
    // Baseline near the vertical middle (Y-up coords).
    ctx.fill_text("HHHH", 4.0, 10.0);
    ctx.pop_layer();
    ctx.flush_to_surface(&target.view);

    let data = target.read();

    // Find the darkest (lowest luminance) pixel — the glyph interior.
    // Luminance sums range 0..=765, so start the accumulator above that.
    let mut darkest = u16::MAX;
    let mut darkest_px = [255u8; 4];
    for y in 0..h {
        for x in 0..w {
            let p = px(&data, w, x, y);
            let lum = p[0] as u16 + p[1] as u16 + p[2] as u16;
            if lum < darkest {
                darkest = lum;
                darkest_px = p;
            }
        }
    }

    let [r, g, b, _a] = darkest_px;
    // Single 0.5 fade of black over white → ~127.  A washed-out result (alpha
    // never written, additive composite) stays near white (>180).
    assert!(
        (90..=165).contains(&r),
        "darkest text pixel should be mid-gray (~127); got {darkest_px:?}"
    );
    // Grayscale — no subpixel chroma fringing inside the layer.
    let spread = r.max(g).max(b) - r.min(g).min(b);
    assert!(
        spread <= 24,
        "text inside a layer must be grayscale (no fringing); channel spread \
         {spread} for {darkest_px:?}"
    );

    // A corner far from the text stays background white.
    let corner = px(&data, w, w - 1, 0);
    assert!(
        corner[0] > 230 && corner[1] > 230 && corner[2] > 230,
        "background corner must stay white; got {corner:?}"
    );
}

/// Fix (1) + flatten: a fully-covered black two-plane backbuffer blitted inside
/// a 0.5 alpha layer over white must land at mid-gray.  On the old 3-pass path
/// the layer alpha stayed 0 and the composite left the region white.
#[test]
fn lcd_backbuffer_in_alpha_layer_writes_alpha() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 32u32);
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);

    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);

    // Fully-covered opaque black text: premultiplied colour plane is (0,0,0),
    // per-channel alpha plane is (255,255,255).  Small 8×8 patch.
    let (pw, ph) = (8u32, 8u32);
    let mut color = Vec::with_capacity((pw * ph * 3) as usize);
    let mut alpha = Vec::with_capacity((pw * ph * 3) as usize);
    for _ in 0..(pw * ph) {
        color.extend_from_slice(&[0, 0, 0]);
        alpha.extend_from_slice(&[255, 255, 255]);
    }
    let color = Arc::new(color);
    let alpha = Arc::new(alpha);

    ctx.clear(Color::white());
    ctx.push_layer_with_alpha(w as f64, h as f64, 0.5);
    ctx.draw_lcd_backbuffer_arc(&color, &alpha, 0, pw, ph, 4.0, 4.0, pw as f64, ph as f64);
    ctx.pop_layer();
    ctx.flush_to_surface(&target.view);

    let data = target.read();

    // Scan for the darkest pixel (the covered patch). Luminance sums range
    // 0..=765, so the accumulator must start above that range.
    let mut darkest = u16::MAX;
    let mut darkest_px = [255u8; 4];
    for y in 0..h {
        for x in 0..w {
            let p = px(&data, w, x, y);
            let lum = p[0] as u16 + p[1] as u16 + p[2] as u16;
            if lum < darkest {
                darkest = lum;
                darkest_px = p;
            }
        }
    }
    let [r, g, b, _a] = darkest_px;
    assert!(
        (90..=165).contains(&r),
        "covered black backbuffer in a 0.5 layer must be mid-gray (~127), not \
         washed out; got {darkest_px:?}"
    );
    let spread = r.max(g).max(b) - r.min(g).min(b);
    assert!(spread <= 12, "flattened backbuffer must be gray; got {darkest_px:?}");
}

/// Layer clip: a layer pushed under a clip covering only the left half must not
/// composite over the right half on pop.
#[test]
fn pop_layer_composite_respects_parent_clip_wgpu() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 32u32);
    let target = Target::new(Arc::clone(&device), Arc::clone(&queue), w, h);

    let mut ctx = WgpuGfxCtx::new(
        Arc::clone(&device),
        Arc::clone(&queue),
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    ctx.reset(w as f32, h as f32);

    ctx.clear(Color::rgba(0.0, 0.0, 0.0, 1.0)); // black background
    ctx.clip_rect(0.0, 0.0, (w / 2) as f64, h as f64); // left half only
    ctx.push_layer_with_alpha(w as f64, h as f64, 1.0);
    ctx.set_fill_color(Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, w as f64, h as f64);
    ctx.fill();
    ctx.pop_layer();
    ctx.flush_to_surface(&target.view);

    let data = target.read();

    // Left half → red composited through.
    let left = px(&data, w, w / 4, h / 2);
    assert!(
        left[0] > 200 && left[1] < 60 && left[2] < 60,
        "inside the parent clip must be red; got {left:?}"
    );
    // Right half → clip rejected the composite; stays black background.
    let right = px(&data, w, 3 * w / 4, h / 2);
    assert!(
        right[0] < 40 && right[1] < 40 && right[2] < 40,
        "outside the parent clip must stay black background; got {right:?}"
    );
}
