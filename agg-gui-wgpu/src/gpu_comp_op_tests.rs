//! Port of agg-sharp `Tests/Agg.Tests/Agg.GoldenImages/GpuCompOpTests.cs`:
//! blend modes (`DrawCtx::set_blend_mode`) on a real device.  On a
//! transparent target, a source drawn through each operator over a
//! translucent destination comes out as the software blender
//! (`agg_rust::comp_op`, what `GfxCtx` blends with) computes it at the same
//! cover — inside, at anti-aliased edges, over the destination and over
//! nothing.
//!
//! The GPU's anti-aliasing is not AGG's, so the cover of each pixel is
//! measured on the device (the same geometry drawn opaque white) and handed
//! to the software blender; what is compared is the operator and its cover
//! handling, within 2 per channel (C#'s tolerance).
//!
//! C#'s `DrawWithCompOp(op, draw)` composites a block of draws as one layer;
//! agg-gui's blend mode is per draw call (canvas `globalCompositeOperation`),
//! so here one `fill()` is one composite.  The rings test therefore draws its
//! twelve rings as one path — the Rust counterpart of one software span pass.
//!
//! Skipped (pass trivially) when no GPU adapter is available.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::CompOp;
use agg_rust::basics::{is_stop, VertexSource};
use agg_rust::color::Rgba8;
use agg_rust::comp_op::PixfmtRgba32CompOp;
use agg_rust::ellipse::Ellipse;
use agg_rust::pixfmt_rgba::PixelFormat;
use agg_rust::rendering_buffer::RowAccessor;

use crate::layer_text_readback_tests::try_device;
use crate::WgpuGfxCtx;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 100;

fn destination() -> Color {
    Color::from_rgba8(200, 100, 50, 160)
}

fn source() -> Color {
    Color::from_rgba8(40, 180, 220, 120)
}

const ALL_OPERATORS: [CompOp; 25] = [
    CompOp::Clear,
    CompOp::Src,
    CompOp::Dst,
    CompOp::SrcOver,
    CompOp::DstOver,
    CompOp::SrcIn,
    CompOp::DstIn,
    CompOp::SrcOut,
    CompOp::DstOut,
    CompOp::SrcAtop,
    CompOp::DstAtop,
    CompOp::Xor,
    CompOp::Plus,
    CompOp::Minus,
    CompOp::Multiply,
    CompOp::Screen,
    CompOp::Overlay,
    CompOp::Darken,
    CompOp::Lighten,
    CompOp::ColorDodge,
    CompOp::ColorBurn,
    CompOp::HardLight,
    CompOp::SoftLight,
    CompOp::Difference,
    CompOp::Exclusion,
];

/// An offscreen RGBA8 target the size of C#'s capture, read back with a
/// 256-aligned row pitch.
struct Capture {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Capture {
    fn new(device: &Arc<wgpu::Device>, queue: &Arc<wgpu::Queue>) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("comp-op-capture"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
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
            device: Arc::clone(device),
            queue: Arc::clone(queue),
            texture,
            view,
        }
    }

    /// A context drawing onto this capture, cleared transparent.  With
    /// `readable` the context gets the target texture to copy from; without
    /// it the frame takes the proxy path a non-`COPY_SRC` surface takes.
    fn begin(&self, readable: bool) -> WgpuGfxCtx {
        let mut ctx = WgpuGfxCtx::new(
            Arc::clone(&self.device),
            Arc::clone(&self.queue),
            wgpu::TextureFormat::Rgba8Unorm,
            WIDTH as f32,
            HEIGHT as f32,
        );
        ctx.reset(WIDTH as f32, HEIGHT as f32);
        if readable {
            ctx.set_surface_texture(self.texture.clone());
        }
        ctx.clear(Color::rgba(0.0, 0.0, 0.0, 0.0));
        ctx
    }

    /// Flush `ctx` and read the target back, rows bottom-up as agg-sharp's
    /// `ImageBuffer` stores them, so `(x, y)` matches C#'s coordinates.
    fn finish(&self, mut ctx: WgpuGfxCtx) -> Vec<[u8; 4]> {
        ctx.flush_to_surface(&self.view);
        ctx.release_frame_texture();
        let bpr = (WIDTH * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("comp-op-readback"),
            size: (bpr * HEIGHT) as u64,
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
                    rows_per_image: Some(HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
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
        let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
        for y in 0..HEIGHT {
            // Readback row 0 is the top; C#'s row 0 is the bottom.
            let row = (HEIGHT - 1 - y) * bpr;
            for x in 0..WIDTH {
                let i = (row + x * 4) as usize;
                pixels.push([data[i], data[i + 1], data[i + 2], data[i + 3]]);
            }
        }
        pixels
    }

    fn capture(&self, draw: impl FnOnce(&mut WgpuGfxCtx)) -> Vec<[u8; 4]> {
        let mut ctx = self.begin(true);
        draw(&mut ctx);
        self.finish(ctx)
    }
}

fn at(image: &[[u8; 4]], x: u32, y: u32) -> [u8; 4] {
    image[(y * WIDTH + x) as usize]
}

/// Append agg-sharp's `Ellipse(x, y, r, r, steps, cw)` polygon to the path.
fn add_ellipse(ctx: &mut WgpuGfxCtx, x: f64, y: f64, r: f64, steps: u32, cw: bool) {
    let mut ellipse = Ellipse::new(x, y, r, r, steps, cw);
    ellipse.rewind(0);
    let (mut vx, mut vy) = (0.0, 0.0);
    let mut first = true;
    loop {
        let cmd = ellipse.vertex(&mut vx, &mut vy);
        if is_stop(cmd) {
            break;
        }
        if agg_rust::basics::is_vertex(cmd) {
            if first {
                ctx.move_to(vx, vy);
                first = false;
            } else {
                ctx.line_to(vx, vy);
            }
        }
    }
    ctx.close_path();
}

/// The destination: a translucent rectangle on whole pixels, covering x 0-120.
fn draw_destination(ctx: &mut WgpuGfxCtx) {
    ctx.begin_path();
    ctx.rect(0.0, 0.0, 120.0, HEIGHT as f64);
    ctx.set_fill_color(destination());
    ctx.fill();
}

/// `pixel` with `color` blended in through `op` at `cover`, as the software
/// renderer's `PixfmtRgba32CompOp` blends one span pixel.
fn software_blend(op: CompOp, pixel: [u8; 4], color: Color, cover: u8) -> [u8; 4] {
    let mut buf = pixel;
    let mut ra = RowAccessor::new();
    // SAFETY: `buf` is a live 4-byte, 1x1 RGBA buffer that outlives `ra`.
    unsafe { ra.attach(buf.as_mut_ptr(), 1, 1, 4) };
    let mut pf = PixfmtRgba32CompOp::new_with_op(&mut ra, op);
    let c: Rgba8 = color.to_rgba8();
    pf.blend_pixel(0, 0, &c, cover);
    buf
}

/// The pixel software gets: a transparent pixel, `destination` drawn
/// source-over, then `source` through `op` at `cover`.
fn expected(op: CompOp, destination: Option<Color>, source: Option<Color>, cover: u8) -> [u8; 4] {
    let mut pixel = [0u8; 4];
    if let Some(d) = destination {
        pixel = software_blend(CompOp::SrcOver, pixel, d, 255);
    }
    if let Some(s) = source {
        pixel = software_blend(op, pixel, s, cover);
    }
    pixel
}

fn assert_near(
    image: &[[u8; 4]],
    x: u32,
    y: u32,
    expected: [u8; 4],
    place: &str,
    failures: &mut Vec<String>,
) {
    let actual = at(image, x, y);
    if (0..4).any(|i| (actual[i] as i32 - expected[i] as i32).abs() > 2) {
        failures.push(format!(
            "{place}: GPU {actual:?} vs software {expected:?} (RGBA)"
        ));
    }
}

/// Draw the destination, then `draw_source` through `op`, and check every
/// pixel of `rows` against the software blender at the cover `draw_cover`
/// leaves in an opaque-white draw of the same geometry.
fn assert_matches_software(
    capture: &Capture,
    op: CompOp,
    source: Color,
    draw_source: &dyn Fn(&mut WgpuGfxCtx, Color),
    rows: &[u32],
    failures: &mut Vec<String>,
) {
    let cover = capture.capture(|ctx| draw_source(ctx, Color::white()));
    // The destination as the device draws it: its anti-aliased rim is the
    // device's, not AGG's.
    let dest = capture.capture(draw_destination);
    let image = capture.capture(|ctx| {
        draw_destination(ctx);
        ctx.set_blend_mode(op);
        draw_source(ctx, source);
        ctx.set_blend_mode(CompOp::SrcOver);
    });
    for &y in rows {
        for x in 0..WIDTH {
            let c = at(&cover, x, y)[3];
            // The software renderer never visits a pixel the shape does not
            // cover (its spans hold covered cells only).  C#'s blender is the
            // identity at cover 0 for every operator, so C# can call it there;
            // agg-rust's src-atop keeps C++'s blue-from-green typo, which
            // changes the pixel even at cover 0 — so ask the blender only
            // where the shape reaches, as the renderer does.
            let want = if c == 0 {
                at(&dest, x, y)
            } else {
                software_blend(op, at(&dest, x, y), source, c)
            };
            let place = format!("{op:?} at ({x}, {y}), cover {c}");
            assert_near(&image, x, y, want, &place, failures);
        }
    }
}

fn report(failures: Vec<String>) {
    let shown: Vec<&String> = failures.iter().take(20).collect();
    assert!(
        failures.is_empty(),
        "{} pixel(s) off:\n{shown:#?}",
        failures.len()
    );
}

#[test]
fn operator_matches_software_blender_at_every_cover() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let capture = Capture::new(&device, &queue);
    // Crosses the destination's right edge, so its anti-aliased rim lies over
    // the destination and over nothing.
    let shape = |ctx: &mut WgpuGfxCtx, color: Color| {
        ctx.begin_path();
        add_ellipse(ctx, 120.0, 50.0, 40.3, 100, false);
        ctx.set_fill_color(color);
        ctx.fill();
    };
    let mut failures = Vec::new();
    for op in ALL_OPERATORS {
        assert_matches_software(&capture, op, source(), &shape, &[50, 70, 89], &mut failures);
    }
    report(failures);
}

#[test]
fn rings_meet_the_destination_once() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let capture = Capture::new(&device, &queue);
    // Concentric rings, as compositing2 draws its gradients: their
    // anti-aliased seams overlap, and an operator applied ring by ring would
    // go through the seams twice.  Opaque, so the layer's coverage at a seam
    // is what the white measurement sees.
    let opaque = Color::from_rgba8(40, 180, 220, 255);
    let mut failures = Vec::new();
    for op in [CompOp::Xor, CompOp::Difference, CompOp::SrcIn] {
        assert_matches_software(&capture, op, opaque, &draw_rings, &[50, 57], &mut failures);
    }
    report(failures);
}

fn draw_rings(ctx: &mut WgpuGfxCtx, color: Color) {
    const RINGS: u32 = 12;
    ctx.begin_path();
    for i in 0..RINGS {
        add_ellipse(
            ctx,
            100.0,
            50.0,
            40.0 * (i + 1) as f64 / RINGS as f64,
            100,
            false,
        );
        if i > 0 {
            add_ellipse(ctx, 100.0, 50.0, 40.0 * i as f64 / RINGS as f64, 100, true);
        }
    }
    ctx.set_fill_color(color);
    ctx.fill();
}

fn draw_after_xor(ctx: &mut WgpuGfxCtx) {
    draw_destination(ctx);
    ctx.set_blend_mode(CompOp::Xor);
    ctx.begin_path();
    ctx.rect(80.0, 20.0, 30.0, 60.0);
    ctx.set_fill_color(source());
    ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.begin_path();
    ctx.rect(185.0, 20.0, 10.0, 60.0);
    ctx.fill();
}

#[test]
fn drawing_afterwards_is_source_over() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let capture = Capture::new(&device, &queue);
    let image = capture.capture(draw_after_xor);
    let mut failures = Vec::new();
    let after = expected(CompOp::SrcOver, None, Some(source()), 255);
    assert_near(
        &image,
        190,
        50,
        after,
        "source-over afterwards",
        &mut failures,
    );
    let untouched = expected(CompOp::SrcOver, Some(destination()), None, 255);
    assert_near(
        &image,
        50,
        50,
        untouched,
        "destination untouched",
        &mut failures,
    );
    report(failures);
}

/// A surface without `COPY_SRC` (the native shell's default) renders the
/// frame into a readable proxy and blits it on: same pixels as a readable
/// target.
#[test]
fn rust_only_unreadable_surface_composites_through_a_proxy() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let capture = Capture::new(&device, &queue);
    let readable = capture.capture(draw_after_xor);
    let mut ctx = capture.begin(false);
    draw_after_xor(&mut ctx);
    let proxied = capture.finish(ctx);
    let xor = expected(CompOp::Xor, Some(destination()), Some(source()), 255);
    let mut failures = Vec::new();
    assert_near(
        &proxied,
        90,
        50,
        xor,
        "xor over the destination",
        &mut failures,
    );
    if readable != proxied {
        failures.push("proxy frame differs from the readable frame".to_string());
    }
    report(failures);
}

/// The mode is saved and restored with the rest of the state, and a clip
/// limits the composite: the clear outside the clip leaves the destination.
#[test]
fn rust_only_blend_mode_follows_save_restore_and_clip() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let capture = Capture::new(&device, &queue);
    let image = capture.capture(|ctx| {
        draw_destination(ctx);
        ctx.save();
        ctx.set_blend_mode(CompOp::Clear);
        ctx.clip_rect(0.0, 0.0, 60.0, HEIGHT as f64);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 120.0, HEIGHT as f64);
        ctx.fill();
        ctx.restore();
        // Source-over again after the restore.
        ctx.begin_path();
        ctx.rect(150.0, 0.0, 20.0, HEIGHT as f64);
        ctx.set_fill_color(source());
        ctx.fill();
    });
    let mut failures = Vec::new();
    assert_near(
        &image,
        30,
        50,
        [0, 0, 0, 0],
        "cleared inside the clip",
        &mut failures,
    );
    let dest = expected(CompOp::SrcOver, Some(destination()), None, 255);
    assert_near(&image, 90, 50, dest, "kept outside the clip", &mut failures);
    let over = expected(CompOp::SrcOver, None, Some(source()), 255);
    assert_near(
        &image,
        160,
        50,
        over,
        "source-over after restore",
        &mut failures,
    );
    report(failures);
}
