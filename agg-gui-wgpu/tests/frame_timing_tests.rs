//! What a custom render pass learns about its device and frames
//! (`WgpuCustomRenderCtx::device_generation`, `frame` and
//! `last_frame_completion`), seen from inside a pass run by a
//! [`HeadlessFrame`] (the frame a shell runs).
//!
//! The device generation is what a 3-D view keys its GPU state on: a device
//! made after another, each on its own `wgpu::Instance` as every agg-gui
//! device is, must never share the earlier one's, or the view keeps using
//! resources of a device that is gone. The frame completion is the frame cost
//! a view's quality control reads on a GPU without timestamp queries; it must
//! arrive for submitted frames without anything waiting for it.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::cell::RefCell;
use std::rc::Rc;

use agg_gui::color::Color;
use agg_gui::Rect;
use agg_gui_wgpu::{
    FrameCompletion, HeadlessFrame, HeadlessGpu, WgpuCustomRender, WgpuCustomRenderCtx,
};

const SIZE: u32 = 16;

/// What the pass saw in one frame.
#[derive(Clone, Copy, Debug)]
struct Seen {
    device_generation: u64,
    frame: u64,
    completion: Option<FrameCompletion>,
}

/// A custom pass that records what its context told it and draws nothing.
#[derive(Default)]
struct Probe {
    seen: Vec<Seen>,
}

impl WgpuCustomRender for Probe {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>) {
        self.seen.push(Seen {
            device_generation: ctx.device_generation,
            frame: ctx.frame,
            completion: ctx.last_frame_completion,
        });
    }
}

/// Render one frame holding `probe` and return what it saw.
fn frame_with_probe(frame: &mut HeadlessFrame, probe: &Rc<RefCell<Probe>>) -> Seen {
    frame.render(Color::rgb(1.0, 1.0, 1.0), |ctx| {
        ctx.push_custom_render(probe.clone(), Rect::new(0.0, 0.0, SIZE as f64, SIZE as f64));
    });
    *probe.borrow().seen.last().expect("the custom pass ran")
}

/// The generation a custom pass sees in a frame on `gpu`.
fn generation_seen_on(gpu: &HeadlessGpu) -> u64 {
    let mut frame = HeadlessFrame::new(gpu, SIZE, SIZE);
    let probe = Rc::new(RefCell::new(Probe::default()));
    let seen = frame_with_probe(&mut frame, &probe);
    assert_eq!(seen.device_generation, frame.device_generation());
    seen.device_generation
}

#[test]
fn devices_made_one_after_another_never_share_a_generation() {
    let (a, b) = match (HeadlessGpu::create(), HeadlessGpu::create()) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    assert_ne!(a.device_generation(), 0, "0 is never a device's");
    assert!(
        b.device_generation() > a.device_generation(),
        "the later device has the later generation: {} then {}",
        a.device_generation(),
        b.device_generation()
    );
    // The pass sees its own device's generation, whichever device it is on.
    assert_eq!(generation_seen_on(&a), a.device_generation());
    assert_eq!(generation_seen_on(&b), b.device_generation());
}

#[test]
fn a_device_rebuilt_after_loss_has_a_new_generation() {
    let lost = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let before = generation_seen_on(&lost);
    // Lose the device, then rebuild as a shell recovers: a new device on a
    // new instance.
    lost.device().destroy();
    let rebuilt = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => panic!("rebuilding the device after a loss failed: {e}"),
    };
    let after = generation_seen_on(&rebuilt);
    assert_ne!(
        after, before,
        "the rebuilt device's passes see a new generation"
    );
    assert_eq!(after, rebuilt.device_generation());
}

#[test]
fn frames_on_one_device_share_its_generation() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    // Resources made on a device stay valid for every frame on it, so the
    // generation is the device's, not the frame's.
    assert_eq!(generation_seen_on(gpu), gpu.device_generation());
    assert_eq!(generation_seen_on(gpu), gpu.device_generation());
}

#[test]
fn completions_arrive_for_submitted_frames_without_waiting_for_them() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let mut frame = HeadlessFrame::new(gpu, SIZE, SIZE);
    assert_eq!(frame.ctx().frame_number(), 0, "no frame begun yet");
    assert!(frame.ctx().last_frame_completion().is_none());
    let probe = Rc::new(RefCell::new(Probe::default()));
    const FRAMES: u64 = 8;
    for _ in 0..FRAMES {
        frame_with_probe(&mut frame, &probe);
    }
    let seen = probe.borrow().seen.clone();

    // Frame numbers count the frames, one each.
    let numbers: Vec<u64> = seen.iter().map(|s| s.frame).collect();
    assert_eq!(numbers, (1..=FRAMES).collect::<Vec<_>>());
    assert_eq!(frame.ctx().frame_number(), FRAMES);

    let mut last_completed = 0;
    for s in &seen {
        let Some(done) = s.completion else {
            // The first frames may run before any report: a frame's
            // completion is never waited for.
            assert!(s.frame <= 2, "frame {} still had no completion", s.frame);
            continue;
        };
        // Never the frame being recorded: nothing waited for it.
        assert!(
            done.frame < s.frame,
            "{done:?} seen while recording {}",
            s.frame
        );
        // A headless frame waits for the frame before last (its pacing), so
        // that one's report has arrived.
        assert!(
            done.frame + 2 >= s.frame,
            "{done:?} is stale at frame {}",
            s.frame
        );
        assert!(done.frame >= last_completed, "completions go forward");
        last_completed = done.frame;
        assert!(
            done.submit_to_done_ms.is_finite()
                && done.submit_to_done_ms >= 0.0
                && done.submit_to_done_ms < 60_000.0,
            "plausible time: {done:?}"
        );
    }
    assert!(last_completed > 1, "completions kept arriving");

    // Once the GPU has finished everything, the latest is the last frame.
    let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());
    let last = frame.ctx().last_frame_completion().expect("a completion");
    assert_eq!(last.frame, FRAMES);
}
