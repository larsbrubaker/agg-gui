//! Frame pacing of [`HeadlessFrame::render`]: a headless harness keeps at most
//! two frames on the GPU, as a window's double-buffered swap chain does,
//! rather than running as far ahead of the GPU as the backend's queue allows.
//! Unpaced, MatterCAD's headless UI tests queued hundreds of frames each
//! (Metal allows 4096 outstanding command buffers), and several test processes
//! sharing the GPU spent most of their time blocked in the driver.
//!
//! Its own test binary, so no other test thread submits to the shared device
//! between a frame and the completion callback registered after it (the
//! callback attaches to the device's latest submission).
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui_wgpu::{HeadlessFrame, HeadlessGpu};

/// By the time frame `n` returns from `render`, frame `n - 1` has finished on
/// the GPU: the completion callback registered after frame `n - 1` was
/// submitted has run. And the index `render` returns ends its frame: waiting
/// on the last frame's index runs the last frame's callback.
#[test]
fn render_keeps_at_most_two_frames_on_the_gpu() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: {e}");
            return;
        }
    };
    let mut frame = HeadlessFrame::new(gpu, 64, 48);
    let finished = Arc::new(AtomicUsize::new(0));
    let mut last = None;
    for n in 1..=8usize {
        last = Some(frame.render(Color::rgb(0.0, 0.0, 1.0), |ctx| {
            ctx.set_fill_color(Color::rgb(1.0, 0.0, 0.0));
            ctx.begin_path();
            ctx.rect(8.0, 8.0, 32.0, 24.0);
            ctx.fill();
        }));
        if n >= 2 {
            assert!(
                finished.load(Ordering::SeqCst) >= n - 1,
                "frame {n} returned with frame {} still on the GPU",
                n - 1
            );
        }
        let done = Arc::clone(&finished);
        gpu.queue().on_submitted_work_done(move || {
            done.fetch_max(n, Ordering::SeqCst);
        });
    }
    gpu.device()
        .poll(wgpu::PollType::Wait {
            submission_index: last,
            timeout: None,
        })
        .expect("wait for the last frame");
    assert_eq!(finished.load(Ordering::SeqCst), 8);
}
