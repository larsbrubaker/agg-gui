//! A frame drawn on a lost device returns without panicking, and the next
//! device draws the normal picture.
//!
//! When the graphics device is lost (a driver reset, a GPU removed, a browser
//! tab out of GPU memory) every buffer made on it afterwards is invalid, and
//! the shell finds out only when it next checks `device_lost`: until then the
//! UI keeps drawing frames on the dead device. Such a frame must do nothing
//! harmful: wgpu reports its errors to the device's uncaptured-error handler
//! (which the shells install, `UncapturedErrors`) and the frame is lost; the
//! shell then rebuilds the device and redraws. A buffer filled through a
//! mapping at creation (`create_buffer_init`) instead panics on the dead
//! device, because wgpu has no fallible way to take a mapped range, so none
//! of the frame's buffers may be filled that way.
//!
//! The loss here is real: `wgpu::Device::destroy` makes the device invalid at
//! once, so every later buffer is invalid, as after a driver-reported loss.
//! The poll that finds the destroyed device's queue empty destroys every
//! buffer made on it, readbacks mapped before the loss included, and taking
//! the mapped range of such a buffer panics inside wgpu: the screenshot
//! read-backs and capture harvests check the device's `DeviceLostFlag` first,
//! and a custom render pass sees the same flag. A tumble cube first painted
//! after the loss builds its GPU state on the dead device without panicking.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::widget::Widget;
use agg_gui::widgets::tumble_cube::math::{self, Mat3};
use agg_gui::widgets::tumble_cube::{TumbleCube, TumbleCubeCamera};
use agg_gui::{CompOp, Point, Rect, Size};
use agg_gui_wgpu::{
    DeviceLostFlag, HeadlessFrame, HeadlessGpu, SsaaFramebuffer, UncapturedErrors,
    WgpuCustomRender, WgpuCustomRenderCtx, WgpuGfxCtx, WgpuTumbleCubeRenderer,
};

const SIZE: u32 = 128;
const CUBE: u32 = 100;
const CLEAR: Color = Color::rgba(0.0, 1.0, 0.0, 1.0);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

struct FixedCamera(Mat3);

impl TumbleCubeCamera for FixedCamera {
    fn view_rotation(&self) -> Mat3 {
        self.0
    }
    fn begin_rotate(&mut self, _: Point) {}
    fn rotate(&mut self, _: Point) {}
    fn end_rotate(&mut self) {}
    fn animate_rotation(&mut self, _: Mat3) {}
}

fn new_cube() -> TumbleCube {
    let cam = Rc::new(RefCell::new(FixedCamera(math::look_at(
        [0.4, 1.0, -0.6],
        [0.0, 0.0, 1.0],
    ))));
    let mut cube = TumbleCube::new(cam).with_gpu_renderer(Box::new(WgpuTumbleCubeRenderer::new()));
    cube.layout(Size::new(SIZE as f64, SIZE as f64));
    cube
}

/// The pixel in the middle of the cube, which sits at the frame's bottom left.
fn cube_middle(pixels: &[u8]) -> [u8; 4] {
    let (x, y_down) = (CUBE / 2, SIZE - CUBE / 2);
    let i = ((y_down * SIZE + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// A render target a shell without a readable surface texture draws into.
fn target(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lost device target"),
        size: wgpu::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// A blend-mode fill on a context with no readable surface texture: the
/// frame is drawn into a proxy and blitted onto `view` with a uniform buffer
/// made for that draw.
fn blend_mode_frame(ctx: &mut WgpuGfxCtx, view: &wgpu::TextureView) {
    ctx.reset(SIZE as f32, SIZE as f32);
    ctx.begin_frame(view.clone());
    ctx.clear(Color::white());
    ctx.set_fill_color(Color::rgba(0.9, 0.5, 0.1, 1.0));
    ctx.set_blend_mode(CompOp::Multiply);
    ctx.begin_path();
    ctx.rect(16.0, 16.0, 64.0, 64.0);
    ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.end_frame();
}

/// A frame drawn onto `texture` as a shell's surface texture, so it can be
/// read back; `capture` then snapshots it for the capture read-backs. Returns
/// what [`WgpuGfxCtx::read_screenshot`] read.
fn screenshot_frame(
    ctx: &mut WgpuGfxCtx,
    texture: &wgpu::Texture,
    view: &wgpu::TextureView,
    capture: bool,
) -> (Vec<u8>, u32, u32) {
    ctx.reset(SIZE as f32, SIZE as f32);
    ctx.set_surface_texture(texture.clone());
    ctx.begin_frame(view.clone());
    ctx.clear(Color::white());
    ctx.set_fill_color(Color::rgba(0.2, 0.3, 0.9, 1.0));
    ctx.begin_path();
    ctx.rect(8.0, 8.0, 32.0, 32.0);
    ctx.fill();
    ctx.end_frame();
    let shot = ctx.read_screenshot();
    if capture {
        assert!(ctx.capture_screenshot(), "the frame is captured");
    }
    ctx.release_frame_texture();
    shot
}

/// A custom render pass that records what its context says about the
/// device's loss, and keeps the flag to ask again later.
#[derive(Default)]
struct LossProbe {
    seen: Vec<bool>,
    kept: Option<DeviceLostFlag>,
}

impl WgpuCustomRender for LossProbe {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>) {
        self.seen.push(ctx.device_lost.is_lost());
        self.kept.get_or_insert_with(|| ctx.device_lost.clone());
    }
}

/// A frame of the cube with the probe's pass after it.
fn probed_frame(frame: &mut HeadlessFrame, cube: &mut TumbleCube, probe: &Rc<RefCell<LossProbe>>) {
    let pass: agg_gui_wgpu::SharedCustomRenderer = probe.clone();
    frame.render(CLEAR, |ctx| {
        cube.paint(ctx);
        ctx.push_custom_render(pass, Rect::new(0.0, 0.0, SIZE as f64, SIZE as f64));
    });
}

/// An SSAA downsample blit into an encoder the caller submits.
fn ssaa_blit(gpu: &HeadlessGpu, fb: &SsaaFramebuffer, ctx: &WgpuGfxCtx, view: &wgpu::TextureView) {
    let device = gpu.device();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let mut buffers = Vec::new();
    fb.blit_downsample_retiring(
        device,
        gpu.queue(),
        &mut encoder,
        view,
        (SIZE, SIZE),
        Rect::new(0.0, 0.0, SIZE as f64, SIZE as f64),
        None,
        ctx.pipelines(),
        2,
        &mut |buffer| buffers.push(buffer),
    );
    gpu.queue().submit(std::iter::once(encoder.finish()));
    for buffer in buffers {
        buffer.destroy();
    }
}

#[test]
fn a_frame_on_a_lost_device_returns_and_the_next_device_draws_the_picture() {
    let gpu = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    // As the shells do: an error wgpu reports loses the frame, not the app.
    let errors = UncapturedErrors::install(gpu.device());

    // Everything the frame path keeps exists before the loss: the cube's GPU
    // state and framebuffer, a context with its pipelines, a framebuffer.
    let mut cube = new_cube();
    let mut frame = HeadlessFrame::new(&gpu, SIZE, SIZE);
    let probe = Rc::new(RefCell::new(LossProbe::default()));
    probed_frame(&mut frame, &mut cube, &probe);
    assert_eq!(probe.borrow().seen, [false], "a live device is not lost");
    let before = cube_middle(&frame.read_rgba().expect("read the first frame"));
    assert_ne!(
        before,
        [0, 255, 0, 255],
        "the cube draws on the first device"
    );
    let (_texture, view) = target(gpu.device());
    let mut ctx = WgpuGfxCtx::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        FORMAT,
        SIZE as f32,
        SIZE as f32,
    );
    blend_mode_frame(&mut ctx, &view);
    let fb = SsaaFramebuffer::new(gpu.device(), SIZE, SIZE, FORMAT, false);
    ssaa_blit(&gpu, &fb, &ctx, &view);

    // A shell's context with the device's loss flag, a read screenshot, and
    // two capture read-backs whose maps complete before the loss and are
    // harvested only after it.
    let (shot_texture, shot_view) = target(gpu.device());
    let mut shot_ctx = WgpuGfxCtx::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        FORMAT,
        SIZE as f32,
        SIZE as f32,
    )
    .with_device_lost(gpu.device_lost_flag());
    let (pixels, w, h) = screenshot_frame(&mut shot_ctx, &shot_texture, &shot_view, true);
    assert_eq!(
        (pixels.len(), w, h),
        ((SIZE * SIZE * 4) as usize, SIZE, SIZE)
    );
    assert_eq!(shot_ctx.read_captured_screenshot().0.len(), pixels.len());
    assert!(
        shot_ctx.begin_capture_readback(),
        "a capture read-back starts"
    );
    assert!(
        shot_ctx.begin_capture_readback_scaled(None, 16, 16),
        "a scaled capture read-back starts"
    );
    gpu.device()
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the capture read-backs' maps complete");
    assert!(!shot_ctx.device_lost().is_lost());
    assert_eq!(errors.count(), 0, "the frames before the loss are valid");

    // Lose the device for real.
    gpu.device().destroy();
    let device = gpu.device().clone();
    let filled_at_creation = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        use wgpu::util::DeviceExt;
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("made after the loss"),
            contents: &[0u8; 16],
            usage: wgpu::BufferUsages::UNIFORM,
        })
    }));
    assert!(
        filled_at_creation.is_err(),
        "a buffer made after destroy() is invalid, so filling it through a mapping panics"
    );

    // The next frame, through the same entry points, on the dead device.
    frame.render(CLEAR, |ctx| cube.paint(ctx));
    blend_mode_frame(&mut ctx, &view);
    ssaa_blit(&gpu, &fb, &ctx, &view);
    let read = frame.read_rgba();
    assert!(
        read.is_err(),
        "reading a frame back from a lost device reports the loss"
    );
    assert!(gpu.device_lost(), "the loss is recorded");

    // A cube first painted now builds its GPU state on the dead device.
    let mut fresh = new_cube();
    frame.render(CLEAR, |ctx| fresh.paint(ctx));

    // A custom render pass is told, and the flag it kept from the first frame
    // says so too.
    probed_frame(&mut frame, &mut cube, &probe);
    assert_eq!(probe.borrow().seen, [false, true], "the pass sees the loss");
    let kept = probe.borrow().kept.clone().expect("the pass kept the flag");
    assert!(kept.is_lost(), "a flag kept from before the loss sees it");

    // The read-backs, whose buffers the loss destroyed, return nothing.
    assert!(shot_ctx.device_lost().is_lost());
    assert_eq!(shot_ctx.poll_capture_readback(), None);
    assert!(
        !shot_ctx.has_pending_readback(),
        "the lost read-back is dropped"
    );
    assert_eq!(shot_ctx.poll_capture_readback_scaled(), None);
    let (pixels, w, h) = screenshot_frame(&mut shot_ctx, &shot_texture, &shot_view, false);
    assert_eq!(
        (pixels.len(), w, h),
        (0, 0, 0),
        "no screenshot of a lost device"
    );
    let (pixels, w, h) = shot_ctx.read_captured_screenshot();
    assert_eq!(
        (pixels.len(), w, h),
        (0, 0, 0),
        "no capture of a lost device"
    );

    // The shell rebuilds: a new device draws the same picture.
    let rebuilt = HeadlessGpu::create().expect("rebuild the device after the loss");
    let rebuilt_errors = UncapturedErrors::install(rebuilt.device());
    let mut frame = HeadlessFrame::new(&rebuilt, SIZE, SIZE);
    frame.render(CLEAR, |ctx| cube.paint(ctx));
    let after = cube_middle(&frame.read_rgba().expect("read the rebuilt frame"));
    assert_eq!(after, before, "the rebuilt device draws the same cube");
    frame.render(CLEAR, |ctx| fresh.paint(ctx));
    let fresh_after = cube_middle(&frame.read_rgba().expect("read the fresh cube's frame"));
    assert_eq!(
        fresh_after, before,
        "the cube first painted on the lost device draws too"
    );
    assert_eq!(rebuilt_errors.count(), 0, "the rebuilt frame is valid");
}
