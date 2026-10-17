//! A lost device fails a headless frame and its read-back at once, naming
//! wgpu's reason, instead of parking the harness in a GPU wait that never
//! ends: [`HeadlessFrame::try_render`] and [`HeadlessFrame::read_rgba`]
//! return [`HeadlessError::DeviceLost`], and [`HeadlessGpu::device_lost_reason`]
//! says why. The device is destroyed to stand in for a lost GPU, so this is
//! its own test binary on a device of its own ([`HeadlessGpu::create`]):
//! destroying it touches no other test's device.
//!
//! Also: a frame's wait budget is the caller's and survives a resize (on
//! the shared device, which nothing here destroys).
//!
//! The bounded-wait decision itself (a GPU that never finishes times out
//! within the budget) is pinned without a GPU in `src/gpu_wait_tests.rs`.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::sync::Arc;
use std::time::{Duration, Instant};

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui_wgpu::{HeadlessError, HeadlessFrame, HeadlessGpu, GPU_WAIT_BUDGET};

fn paint_square(ctx: &mut dyn DrawCtx) {
    ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
    ctx.begin_path();
    ctx.rect(8.0, 8.0, 16.0, 16.0);
    ctx.fill();
}

#[test]
fn a_lost_device_fails_the_frame_and_the_read_back_with_its_reason() {
    let gpu = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    // Work submitted to the destroyed device is rejected as uncaptured
    // errors, which wgpu's default handler turns into panics; the loss, not
    // those, is under test.
    gpu.device()
        .on_uncaptured_error(Arc::new(|_: wgpu::Error| {}));

    let mut frame = HeadlessFrame::new(&gpu, 32, 32);
    assert_eq!(frame.target().wait_budget(), GPU_WAIT_BUDGET);
    frame
        .try_render(Color::rgb(0.0, 0.0, 1.0), |ctx| paint_square(ctx))
        .expect("a healthy frame renders");
    frame
        .try_render(Color::rgb(0.0, 0.0, 1.0), |ctx| paint_square(ctx))
        .expect("a healthy frame paces against the one before it");
    assert_eq!(gpu.device_lost_reason(), None);

    gpu.device().destroy();
    // wgpu delivers the loss from the device's maintenance once its queue
    // is empty; a poll on the destroyed device may report an error, which is
    // not what is under test.
    let _ = gpu.device().poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    });
    let reason = gpu
        .device_lost_reason()
        .expect("destroying the device reports its loss");
    assert!(
        reason.contains("Destroyed"),
        "the reason names wgpu's DeviceLostReason: {reason}"
    );

    let started = Instant::now();
    let rendered = frame.try_render(Color::rgb(0.0, 0.0, 1.0), |ctx| paint_square(ctx));
    assert_eq!(
        rendered.map(|_| ()),
        Err(HeadlessError::DeviceLost(reason.clone()))
    );
    assert_eq!(
        frame.read_rgba(),
        Err(HeadlessError::DeviceLost(reason.clone()))
    );
    // `render` keeps its signature: it logs the loss and returns.
    frame.render(Color::rgb(0.0, 0.0, 1.0), |ctx| paint_square(ctx));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "a known loss fails at once, not after a GPU wait ({:?})",
        started.elapsed()
    );
}

#[test]
fn the_wait_budget_is_the_callers_and_survives_a_resize() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: {e}");
            return;
        }
    };
    let mut frame = HeadlessFrame::new(gpu, 16, 16);
    frame.set_wait_budget(Duration::from_secs(5));
    frame.resize(24, 20);
    assert_eq!(frame.size(), (24, 20));
    assert_eq!(frame.target().wait_budget(), Duration::from_secs(5));
    frame
        .try_render(Color::rgb(0.0, 0.0, 1.0), |ctx| paint_square(ctx))
        .expect("a frame renders within its budget");
    assert_eq!(frame.read_rgba().map(|p| p.len()), Ok(24 * 20 * 4));
}
