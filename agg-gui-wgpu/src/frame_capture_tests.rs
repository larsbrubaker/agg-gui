//! Tests for the one-shot frame capture (`frame_capture.rs`), driving the
//! production [`FrameCaptureService`] through a shell's per-frame calls on a
//! real (headless) device: a frame whose left half is red and right half blue
//! is asked for through `agg_gui::frame_capture`, and what reaches the
//! callback is checked pixel by pixel. Skips (passes trivially) without an
//! adapter, like the other readback tests.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::frame_capture::{request_frame_capture, FrameCaptureError, FrameCaptureResult};
use agg_gui::Rect;

use crate::layer_text_readback_tests::{px, try_device, Target};
use crate::{DeviceLostFlag, FrameCaptureService, WgpuGfxCtx};

const W: u32 = 64;
const H: u32 = 32;
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

type Seen = Rc<RefCell<Vec<FrameCaptureResult>>>;

fn ask(region: Option<Rect>) -> Seen {
    let _ = agg_gui::frame_capture::take_frame_capture_requests();
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    request_frame_capture(region, move |r| sink.borrow_mut().push(r));
    seen
}

fn ctx_for(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    format: wgpu::TextureFormat,
) -> WgpuGfxCtx {
    WgpuGfxCtx::new(
        Arc::clone(device),
        Arc::clone(queue),
        format,
        W as f32,
        H as f32,
    )
}

/// Paint the red | blue frame into `view`, as a shell's frame body does.
fn paint_frame(ctx: &mut WgpuGfxCtx, view: wgpu::TextureView) {
    ctx.begin_frame_cleared(view, Color::rgba(1.0, 0.0, 0.0, 1.0));
    ctx.reset(W as f32, H as f32);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 1.0, 1.0));
    ctx.rect(f64::from(W / 2), 0.0, f64::from(W / 2), f64::from(H));
    ctx.fill();
    ctx.end_frame();
}

/// One armed frame: begin, render offscreen, blit onto `surface_view`.
fn captured_frame(
    svc: &mut FrameCaptureService,
    ctx: &mut WgpuGfxCtx,
    surface_view: &wgpu::TextureView,
) {
    assert!(svc.begin_frame(), "the asked-for frame is armed");
    let view = svc.target_view(ctx, W, H);
    paint_frame(ctx, view);
    svc.finish_frame(ctx, surface_view, (W, H));
    ctx.release_frame_texture();
}

/// Poll later frames until every read has been delivered.
fn poll_until_delivered(svc: &mut FrameCaptureService, ctx: &WgpuGfxCtx) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while svc.poll(ctx) {
        assert!(
            std::time::Instant::now() < deadline,
            "capture never delivered"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn frame_pixels(seen: &Seen) -> (Vec<u8>, u32, u32) {
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1, "delivered exactly once");
    let frame = seen[0].as_ref().expect("the capture delivers pixels");
    assert_eq!(frame.rgba.len(), (frame.width * frame.height * 4) as usize);
    (frame.rgba.clone(), frame.width, frame.height)
}

#[test]
fn a_capture_asked_for_is_delivered_once_with_the_frame_and_the_window_shows_it() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let surface = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let mut svc = FrameCaptureService::new();
    let seen = ask(None);

    captured_frame(&mut svc, &mut ctx, &surface.view);
    assert!(
        seen.borrow().is_empty(),
        "delivered on a later frame, not this one"
    );
    poll_until_delivered(&mut svc, &ctx);

    let (rgba, w, h) = frame_pixels(&seen);
    assert_eq!((w, h), (W, H));
    for y in [0, H / 2, H - 1] {
        assert_eq!(px(&rgba, w, 2, y), RED);
        assert_eq!(px(&rgba, w, W - 3, y), BLUE);
    }
    // The window still got the frame, through the blit.
    let shown = surface.read();
    assert_eq!(px(&shown, W, 2, H / 2), RED);
    assert_eq!(px(&shown, W, W - 3, H / 2), BLUE);

    // The next frame was not asked for: it is not armed and delivers nothing.
    assert!(!svc.begin_frame());
    assert!(!svc.poll(&ctx));
    assert_eq!(seen.borrow().len(), 1);
}

#[test]
fn a_region_delivers_that_region_top_row_first() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let surface = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let mut svc = FrameCaptureService::new();
    // Window coordinates (Y-up, scale 1): straddles the colour split, in the
    // top quarter of the window.
    let seen = ask(Some(Rect::new(28.0, 24.0, 8.0, 8.0)));

    captured_frame(&mut svc, &mut ctx, &surface.view);
    poll_until_delivered(&mut svc, &ctx);

    let (rgba, w, h) = frame_pixels(&seen);
    assert_eq!((w, h), (8, 8));
    for y in 0..h {
        assert_eq!(px(&rgba, w, 3, y), RED, "x 31 is the red half");
        assert_eq!(px(&rgba, w, 4, y), BLUE, "x 32 is the blue half");
    }
}

#[test]
fn a_bgra_window_still_delivers_rgba() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let format = wgpu::TextureFormat::Bgra8Unorm;
    let surface = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frame_capture_test_bgra_surface"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = surface.create_view(&wgpu::TextureViewDescriptor::default());
    let mut ctx = ctx_for(&device, &queue, format);
    let mut svc = FrameCaptureService::new();
    let seen = ask(None);

    captured_frame(&mut svc, &mut ctx, &view);
    poll_until_delivered(&mut svc, &ctx);

    let (rgba, w, _) = frame_pixels(&seen);
    assert_eq!(px(&rgba, w, 2, 0), RED);
    assert_eq!(px(&rgba, w, W - 3, 0), BLUE);
}

#[test]
fn a_frame_nobody_asked_to_capture_takes_the_ordinary_path() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let _ = agg_gui::frame_capture::take_frame_capture_requests();
    let ctx = ctx_for(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let mut svc = FrameCaptureService::new();
    assert!(!svc.begin_frame(), "no request: the frame is not armed");
    assert!(!svc.is_armed());
    assert!(!svc.poll(&ctx), "nothing in flight");
    assert!(!svc.has_reads_in_flight());
}

#[test]
fn an_off_screen_region_is_an_error_not_a_read() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let surface = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let mut svc = FrameCaptureService::new();
    let seen = ask(Some(Rect::new(500.0, 0.0, 10.0, 10.0)));

    captured_frame(&mut svc, &mut ctx, &surface.view);
    assert!(!svc.has_reads_in_flight());
    assert_eq!(*seen.borrow(), vec![Err(FrameCaptureError::RegionEmpty)]);
}

#[test]
fn a_lost_device_delivers_an_error_instead_of_waiting() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let surface = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let lost = DeviceLostFlag::default();
    let mut ctx = ctx_for(&device, &queue, wgpu::TextureFormat::Rgba8Unorm).with_device_lost(&lost);
    let mut svc = FrameCaptureService::new();
    let seen = ask(None);

    captured_frame(&mut svc, &mut ctx, &surface.view);
    lost.record_for_test("Destroyed");
    assert!(!svc.poll(&ctx), "a lost device leaves nothing in flight");

    let seen = seen.borrow();
    assert_eq!(seen.len(), 1, "the callback runs once, at once");
    assert!(
        matches!(&seen[0], Err(FrameCaptureError::DeviceLost(r)) if r.contains("Destroyed")),
        "{:?}",
        seen[0]
    );
}

#[test]
fn a_rebuild_fails_the_captures_the_old_device_held() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let surface = Target::new(Arc::clone(&device), Arc::clone(&queue), W, H);
    let mut ctx = ctx_for(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    let mut svc = FrameCaptureService::new();
    let seen = ask(None);
    captured_frame(&mut svc, &mut ctx, &surface.view);

    svc.device_lost("the device was rebuilt");
    assert!(!svc.has_reads_in_flight());
    assert!(matches!(
        seen.borrow().as_slice(),
        [Err(FrameCaptureError::DeviceLost(_))]
    ));
}
