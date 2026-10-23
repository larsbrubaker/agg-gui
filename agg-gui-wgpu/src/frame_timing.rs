//! What a custom render pass learns about the device and the frames around
//! the one it records: which device it is on ([`next_device_generation`],
//! [`WgpuGfxCtx::device_generation`]), which frame it is recording
//! ([`WgpuGfxCtx::frame_number`]) and how long the latest finished frame took
//! from its submit to the GPU finishing it ([`FrameCompletion`]).
//!
//! # Device generations
//!
//! `wgpu::Device` compares equal by an id that is unique only within the
//! `wgpu::Instance` that made it, and agg-gui makes a new instance for every
//! device: a window's (`Gpu::new`, again for each device-loss rebuild), the
//! browser canvas's (agg-gui-web-shell) and the headless one
//! ([`crate::HeadlessGpu`]). A rebuilt device can therefore compare equal to
//! the lost one, and a renderer that keys its GPU resources on device
//! equality keeps using buffers and query sets from a dead device (wgpu then
//! panics, e.g. "QuerySet does not exist"). Every device agg-gui creates
//! takes the next number from one process-wide counter instead, so two
//! devices never share a generation; a renderer keys its state on that.
//!
//! # Frame completion
//!
//! [`crate::WgpuGfxCtx::end_frame`] submits the frame; right after the submit
//! the context registers `queue.on_submitted_work_done`, whose callback only
//! stores the frame's number and the time since the submit
//! ([`FrameTiming::after_submit`]). Nothing waits for it: the callback runs
//! whenever wgpu reports the work done. That is the whole window's GPU time
//! plus the queue's latency, not one widget's cost, and it is an upper bound:
//! native wgpu and its WebGL2 backend report finished work only at the next
//! submit or device poll, so a GPU that finished sooner shows the time until
//! then (about one frame interval in a window redrawing continuously); the
//! browser's WebGPU resolves the callback itself, promptly in Chrome and on
//! Firefox's next present or 100 ms device poll.
//!
//! The native and web shells and the headless frame drive this through the
//! context; custom passes see it on [`crate::WgpuCustomRenderCtx`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::WgpuGfxCtx;

/// The next generation [`next_device_generation`] hands out. Starts at 1, so
/// 0 is never a device's and a renderer can use it for "no device yet".
static NEXT_DEVICE_GENERATION: AtomicU64 = AtomicU64::new(1);

/// A new device generation: a number no other device in this process has had
/// or will have, increasing with each call. agg-gui takes one for every
/// device it creates ([`crate::Gpu::device_generation`],
/// [`crate::HeadlessGpu::device_generation`], the web shell's canvas device);
/// a shell that creates its own device takes one and records it on its
/// context with [`WgpuGfxCtx::set_device_generation`].
pub fn next_device_generation() -> u64 {
    // Relaxed: the counter orders nothing else; fetch_add alone makes every
    // value unique. A u64 counting one device at a time never wraps.
    NEXT_DEVICE_GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// One window frame the GPU finished: its number
/// ([`WgpuGfxCtx::frame_number`] while it was recorded) and the time from
/// agg-gui's submit of it to wgpu reporting it done.
///
/// Whole-window GPU time plus queue latency, not one widget's cost, and an
/// upper bound where wgpu reports finished work late (see the module docs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameCompletion {
    /// The frame's number, as [`crate::WgpuCustomRenderCtx::frame`] showed it
    /// to the passes recorded into it.
    pub frame: u64,
    /// Milliseconds from the frame's `queue.submit` to the GPU finishing it,
    /// as reported by wgpu; never negative.
    pub submit_to_done_ms: f64,
}

/// The per-context frame bookkeeping behind [`WgpuGfxCtx::device_generation`],
/// [`WgpuGfxCtx::frame_number`] and [`WgpuGfxCtx::last_frame_completion`].
pub(crate) struct FrameTiming {
    device_generation: u64,
    /// The number of the frame begun last; 0 before the first.
    frame: u64,
    /// A frame was begun and its submit has not been timed yet. Only the
    /// first submit after `begin_frame` is: an overlay frame drawn on top
    /// before presenting belongs to the same window frame.
    awaiting_submit: bool,
    /// Written by the completion callbacks, read by the context.
    latest: Arc<Mutex<Option<FrameCompletion>>>,
}

impl FrameTiming {
    /// Bookkeeping for a new context, with a generation of its own until the
    /// creator records its device's ([`WgpuGfxCtx::set_device_generation`]):
    /// a context serves one device for its whole life, so a fresh number is
    /// never shared with another device either.
    pub(crate) fn new() -> Self {
        Self {
            device_generation: next_device_generation(),
            frame: 0,
            awaiting_submit: false,
            latest: Arc::new(Mutex::new(None)),
        }
    }

    /// A window frame begins: the next number, timed at its first submit.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
        self.awaiting_submit = true;
    }

    /// The frame's commands were just submitted to `queue`, starting at
    /// `submitted`. Registers a completion callback that records the time
    /// when wgpu reports the work done; never waits. Only the first submit of
    /// a begun frame is timed.
    pub(crate) fn after_submit(&mut self, queue: &wgpu::Queue, submitted: web_time::Instant) {
        if !std::mem::take(&mut self.awaiting_submit) {
            return;
        }
        let frame = self.frame;
        let latest = Arc::clone(&self.latest);
        queue.on_submitted_work_done(move || {
            let took = web_time::Instant::now().saturating_duration_since(submitted);
            let done = FrameCompletion {
                frame,
                submit_to_done_ms: took.as_secs_f64() * 1000.0,
            };
            if let Ok(mut latest) = latest.lock() {
                // wgpu reports a queue's work in submission order; the check
                // keeps the latest frame should two reports race.
                if latest.is_none_or(|l| l.frame < frame) {
                    *latest = Some(done);
                }
            }
        });
    }

    pub(crate) fn device_generation(&self) -> u64 {
        self.device_generation
    }

    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }

    pub(crate) fn last_completion(&self) -> Option<FrameCompletion> {
        self.latest.lock().ok().and_then(|l| *l)
    }
}

/// The per-frame values `end_frame` hands every custom pass, gathered once
/// per flush.
#[derive(Clone, Copy)]
pub(crate) struct PassFrameInfo {
    pub(crate) lost_frames: u64,
    pub(crate) device_generation: u64,
    pub(crate) frame: u64,
    pub(crate) last_frame_completion: Option<FrameCompletion>,
}

impl WgpuGfxCtx {
    /// Record the generation of this context's device (from
    /// [`next_device_generation`], taken when the device was created), so
    /// custom render passes see it as
    /// [`crate::WgpuCustomRenderCtx::device_generation`]. Without it the
    /// context uses a generation of its own, taken when it was made.
    pub fn set_device_generation(&mut self, generation: u64) {
        self.frame_timing.device_generation = generation;
    }

    /// [`Self::set_device_generation`] in builder form, for the line that
    /// makes the context.
    pub fn with_device_generation(mut self, generation: u64) -> Self {
        self.set_device_generation(generation);
        self
    }

    /// The generation of this context's device: a number no other device in
    /// the process shares, unlike `wgpu::Device` equality (see the
    /// `frame_timing` module docs).
    pub fn device_generation(&self) -> u64 {
        self.frame_timing.device_generation()
    }

    /// The number of the frame begun last with [`Self::begin_frame`]: 1 for
    /// the first, one more for each after (an overlay frame,
    /// [`Self::begin_overlay_frame`], keeps the number of the frame it draws
    /// over). 0 before any.
    pub fn frame_number(&self) -> u64 {
        self.frame_timing.frame()
    }

    /// The latest window frame the GPU has finished, with its submit-to-done
    /// time; `None` until wgpu has reported one. See [`FrameCompletion`].
    pub fn last_frame_completion(&self) -> Option<FrameCompletion> {
        self.frame_timing.last_completion()
    }

    /// The values every custom pass of the frame being flushed sees.
    pub(crate) fn pass_frame_info(&self) -> PassFrameInfo {
        PassFrameInfo {
            lost_frames: self.lost_frames(),
            device_generation: self.device_generation(),
            frame: self.frame_number(),
            last_frame_completion: self.last_frame_completion(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_generations_are_never_handed_out_twice() {
        let first = next_device_generation();
        let second = next_device_generation();
        assert!(first >= 1, "0 is never a device's generation");
        assert!(second > first, "{first} then {second}");
    }
}
