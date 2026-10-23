//! `WgpuCustomRenderCtx::lost_frames`: a custom render pass that keeps its
//! finished picture between frames learns that a frame it recorded into was
//! rejected by the GPU, so the picture never reached its texture.
//!
//! The frames run in a [`HeadlessFrame`] (the frame a shell runs) on a device
//! of this binary's own, carrying [`UncapturedErrors::install`] (the handler
//! the window's `Gpu` installs) recorded on the frame's context as the native
//! and web shells record theirs. One frame's custom pass records a draw with
//! no pipeline set into the frame's own encoder, so the frame's submission is
//! the one wgpu rejects.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use agg_gui::color::Color;
use agg_gui::Rect;
use agg_gui_wgpu::{
    HeadlessFrame, HeadlessGpu, UncapturedErrors, WgpuCustomRender, WgpuCustomRenderCtx,
};

const SIZE: u32 = 32;

/// A custom pass that remembers the `lost_frames` it saw, and when told to,
/// records a draw wgpu rejects: a render pass on the frame's target that
/// draws with no pipeline set.
#[derive(Default)]
struct Probe {
    seen: Rc<Cell<Option<u64>>>,
    sabotage: Rc<Cell<bool>>,
}

impl WgpuCustomRender for Probe {
    fn render(&mut self, ctx: WgpuCustomRenderCtx<'_>) {
        self.seen.set(Some(ctx.lost_frames));
        if !self.sabotage.get() {
            return;
        }
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lostFramesInvalidDraw"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.draw(0..3, 0..1);
    }
}

/// Render one frame holding the probe and return the `lost_frames` it saw,
/// after the GPU has finished the frame (so any rejection is reported).
fn frame_with_probe(
    gpu: &HeadlessGpu,
    frame: &mut HeadlessFrame,
    probe: &Rc<RefCell<Probe>>,
) -> u64 {
    let seen = probe.borrow().seen.clone();
    seen.set(None);
    frame.render(Color::rgb(1.0, 1.0, 1.0), |ctx| {
        ctx.push_custom_render(probe.clone(), Rect::new(0.0, 0.0, SIZE as f64, SIZE as f64));
    });
    let _ = gpu.device().poll(wgpu::PollType::wait_indefinitely());
    seen.get().expect("the custom pass ran this frame")
}

#[test]
fn a_rejected_frame_moves_lost_frames_and_clean_frames_leave_it_alone() {
    let gpu = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let errors = UncapturedErrors::install(gpu.device());
    let mut frame = HeadlessFrame::new(&gpu, SIZE, SIZE);
    frame.ctx_mut().set_uncaptured_errors(&errors);
    let probe = Rc::new(RefCell::new(Probe::default()));
    let sabotage = probe.borrow().sabotage.clone();

    // Clean frames: nothing rejected, the value stays where it started.
    let start = frame_with_probe(&gpu, &mut frame, &probe);
    assert_eq!(start, 0, "a fresh device has lost no frames");
    assert_eq!(frame_with_probe(&gpu, &mut frame, &probe), start);
    assert_eq!(errors.count(), 0, "clean frames report no errors");

    // The sabotaged frame records before its own rejection is reported, so
    // it still sees the old value ...
    sabotage.set(true);
    assert_eq!(frame_with_probe(&gpu, &mut frame, &probe), start);
    sabotage.set(false);
    assert!(errors.count() > 0, "wgpu rejected the sabotaged frame");

    // ... and the next frame sees it moved.
    let after = frame_with_probe(&gpu, &mut frame, &probe);
    assert!(
        after > start,
        "lost_frames moved after a rejected frame: {start} -> {after}"
    );
    assert_eq!(after, errors.count());
    assert_eq!(frame.ctx().lost_frames(), after);

    // Clean frames after the loss leave it where it is.
    assert_eq!(frame_with_probe(&gpu, &mut frame, &probe), after);
    assert_eq!(frame_with_probe(&gpu, &mut frame, &probe), after);
}

#[test]
fn a_context_with_no_error_record_reports_no_lost_frames() {
    let gpu = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let mut frame = HeadlessFrame::new(&gpu, SIZE, SIZE);
    let probe = Rc::new(RefCell::new(Probe::default()));
    assert_eq!(frame_with_probe(&gpu, &mut frame, &probe), 0);
    assert_eq!(frame.ctx().lost_frames(), 0);
}
