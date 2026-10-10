//! agg-sharp `Tests/Agg.Tests/Agg.WebGpuRender/WebGpuControlDeviceLossTests.cs`:
//! device-loss detection driven for real — the device is destroyed, wgpu
//! raises the device-lost callback, and the flag a frame checks
//! (`Gpu::device_lost`, the same watcher as [`HeadlessGpu::device_lost`]) has
//! to see it; then a rebuilt device and context render again.
//!
//! The C# test runs `WebGpuControl` over an off-screen HWND. A window surface
//! cannot be made from a test thread here (winit owns the main thread on
//! macOS), so this runs on a headless device of its own
//! ([`HeadlessGpu::create`]) — its own test binary, so destroying it touches
//! no other test's device. The window half of the rebuild (agg-gui-shell
//! `recover_lost_device`: a new `Gpu` for the window, then `Painter::rebuild`
//! making a fresh `WgpuGfxCtx`) is mirrored by the rebuilt device and fresh
//! context below. `PresentModeIsConfigurable` needs a swap chain and is
//! covered by `gpu.rs`'s `pick_present_mode` tests.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::sync::{Arc, Mutex};

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui_wgpu::{HeadlessFrame, HeadlessGpu};

/// Record the device's first uncaptured error — C#'s `LastUncapturedError`.
fn capture_errors(gpu: &HeadlessGpu) -> Arc<Mutex<Option<String>>> {
    let slot = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&slot);
    gpu.device()
        .on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            sink.lock().unwrap().get_or_insert_with(|| e.to_string());
        }));
    slot
}

/// One frame: a red square on blue, read back so the frame really ran.
fn render_frame(gpu: &HeadlessGpu) -> Vec<u8> {
    let mut frame = HeadlessFrame::new(gpu, 32, 32);
    frame.render(Color::rgb(0.0, 0.0, 1.0), |ctx| {
        ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
        ctx.begin_path();
        ctx.rect(8.0, 8.0, 16.0, 16.0);
        ctx.fill();
    });
    frame.read_rgba().expect("read back the frame")
}

fn px(data: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * 32 + x) * 4;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

#[test]
fn destroyed_device_is_rebuilt_on_the_next_frame() {
    let original = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let original_errors = capture_errors(&original);
    assert!(!original.device_lost());

    // One healthy frame first, so recovery is being asked of a device in its
    // normal steady state.
    let healthy = render_frame(&original);
    assert_eq!(px(&healthy, 16, 16), [255, 0, 0, 255]);
    assert!(original_errors.lock().unwrap().is_none());

    original.device().destroy();
    // The callback is delivered from the device's maintenance; a poll on the
    // destroyed device may report an error, which is not what is under test.
    let _ = original.device().poll(wgpu::PollType::wait_indefinitely());

    assert!(
        original.device_lost(),
        "Device::destroy must raise the device-lost callback, or the recovery has nothing to trigger on"
    );

    // The frame that notices rebuilds everything hanging off the dead device:
    // a new device, and a new context on it (a context's caches are keyed on
    // its device, so reusing it would hand the new device dead resources).
    let rebuilt = HeadlessGpu::create().expect("rebuild the device");
    let rebuilt_errors = capture_errors(&rebuilt);
    assert!(!Arc::ptr_eq(original.device(), rebuilt.device()));
    assert!(!rebuilt.device_lost());

    // And the rebuilt device renders: this is the frame a repainting app would draw.
    let recovered = render_frame(&rebuilt);
    assert_eq!(px(&recovered, 16, 16), [255, 0, 0, 255]);
    assert_eq!(px(&recovered, 1, 1), [0, 0, 255, 255]);
    assert!(rebuilt_errors.lock().unwrap().is_none());
}
