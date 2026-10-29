//! Tests for explicit texture release (`texture_life.rs`): a context drawing
//! frames whose size changes every frame (a page animating its canvas) keeps
//! a bounded set of textures and destroys every one it lets go, steady
//! frames reuse their layer textures instead of making new ones, and the
//! buffers made for single draws (a blend-mode proxy blit, an SSAA blit) are
//! destroyed once the frame that drew with them is submitted.
//!
//! Why this matters: in the browser a dropped texture's video memory is
//! freed only when the JavaScript garbage collector collects it, so textures
//! the context lets go without destroying pile up until a phone's tab runs
//! out of memory. Like the other headless GPU tests these skip (pass
//! trivially) when no adapter is available.

use agg_gui::color::Color;
use agg_gui::draw_ctx::{DrawCtx, GradientSpread, GradientStop, LinearGradientPaint};
use agg_gui::CompOp;
use agg_rust::trans_affine::TransAffine;

use crate::headless::{HeadlessFrame, HeadlessGpu};
use crate::layer_text_readback_tests::Target;
use crate::{SsaaFramebuffer, TextureCounts, WgpuGfxCtx};

/// The shared device, or `None` (the test skips) when there is no adapter.
fn gpu() -> Option<&'static HeadlessGpu> {
    match HeadlessGpu::shared() {
        Ok(gpu) => Some(gpu),
        Err(e) => {
            eprintln!("SKIP: {e}");
            None
        }
    }
}

/// One frame's drawing at `w` x `h`: what a window resizing under a UI
/// makes textures for — transient layers (one nested), a retained layer
/// sized to the view (rebuilt whenever the size changes), a gradient fill
/// (its ramp texture) and a blend-mode fill (its coverage layer and
/// destination copy).
fn paint(ctx: &mut WgpuGfxCtx, w: f64, h: f64) {
    ctx.push_layer(w * 0.5, h * 0.5);
    ctx.set_fill_color(Color::rgba(0.2, 0.4, 0.8, 1.0));
    ctx.begin_path();
    ctx.rect(2.0, 2.0, w * 0.25, h * 0.25);
    ctx.fill();
    ctx.push_layer(w * 0.25, h * 0.25);
    ctx.begin_path();
    ctx.rect(1.0, 1.0, 4.0, 4.0);
    ctx.fill();
    ctx.pop_layer();
    ctx.pop_layer();

    ctx.push_retained_layer_with_alpha(7, w, h, 1.0);
    ctx.begin_path();
    ctx.rect(4.0, 4.0, 8.0, 8.0);
    ctx.fill();
    ctx.pop_layer();

    ctx.set_fill_linear_gradient(LinearGradientPaint {
        x1: 0.0,
        y1: 0.0,
        x2: w,
        y2: 0.0,
        transform: TransAffine::new(),
        spread: GradientSpread::Pad,
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: Color::rgba(1.0, 0.0, 0.0, 1.0),
            },
            GradientStop {
                offset: 1.0,
                color: Color::rgba(0.0, 0.0, 1.0, 1.0),
            },
        ],
    });
    ctx.begin_path();
    ctx.rect(0.0, h * 0.5, w, h * 0.25);
    ctx.fill();

    ctx.set_fill_color(Color::rgba(0.9, 0.5, 0.1, 1.0));
    ctx.set_blend_mode(CompOp::Multiply);
    ctx.begin_path();
    ctx.rect(w * 0.25, h * 0.25, w * 0.5, h * 0.5);
    ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
}

/// Every texture the context made is either held or destroyed: nothing is
/// left waiting once a frame has been submitted.
fn assert_all_released_or_held(counts: TextureCounts, frame: usize) {
    assert_eq!(
        counts.retired, 0,
        "frame {frame}: textures let go must be destroyed after the frame's submit: {counts:?}"
    );
    assert_eq!(
        counts.destroyed,
        counts.created - counts.held(),
        "frame {frame}: {counts:?}"
    );
}

#[test]
fn resizing_every_frame_keeps_a_bounded_set_of_textures() {
    let Some(gpu) = gpu() else { return };
    let mut frame = HeadlessFrame::new(gpu, 120, 90);
    let frames = 40;
    let mut most_held = 0;
    for i in 0..frames {
        // A new size every frame, as a page growing its canvas does.
        let (w, h) = (120 + 7 * i as u32, 90 + 5 * i as u32);
        frame.resize(w, h);
        frame.render(Color::white(), |ctx| paint(ctx, w as f64, h as f64));
        let counts = frame.ctx().texture_counts();
        assert_all_released_or_held(counts, i);
        most_held = most_held.max(counts.held());
    }
    let counts = frame.ctx().texture_counts();
    // Each frame needs new sizes for its layers, so the context really did
    // make textures every frame; what it keeps is one frame's worth (the
    // pooled transient layers, the retained layer) whatever the history.
    assert!(
        counts.created >= frames as u64 * 3,
        "every resized frame makes new layer textures: {counts:?}"
    );
    assert!(
        most_held <= 6,
        "a resizing context holds one frame's textures, not all of them: held up to {most_held}, {counts:?}"
    );
}

#[test]
fn steady_frames_reuse_their_layer_textures() {
    let Some(gpu) = gpu() else { return };
    let mut frame = HeadlessFrame::new(gpu, 160, 120);
    // The first frame makes the layers; later frames take them from the pool.
    frame.render(Color::white(), |ctx| paint(ctx, 160.0, 120.0));
    let first = frame.ctx().texture_counts();
    for i in 0..10 {
        frame.render(Color::white(), |ctx| paint(ctx, 160.0, 120.0));
        let counts = frame.ctx().texture_counts();
        assert_all_released_or_held(counts, i);
        // Only the per-frame textures (the gradient ramp and the blend-mode
        // copy) are new; the layers are reused.
        assert_eq!(
            counts.held(),
            first.held(),
            "frame {i}: steady frames hold the same textures: {counts:?} after {first:?}"
        );
        assert_eq!(
            counts.created - first.created,
            2 * (i as u64 + 1),
            "frame {i}: steady frames make only their one-frame textures: {counts:?} after {first:?}"
        );
    }
}

#[test]
fn a_resized_ssaa_framebuffer_destroys_its_old_attachments() {
    let Some(gpu) = gpu() else { return };
    let device = gpu.device();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut fb = SsaaFramebuffer::new(device, 64, 64, format, true);
    for i in 0..20u32 {
        fb.ensure_size(device, 64 + 9 * i, 64 + 4 * i);
        let counts = fb.texture_counts();
        // Colour and depth are held; the pair a resize replaced waits for
        // the next call (its frame may not be submitted yet).
        assert_eq!(counts.held(), 2, "call {i}: {counts:?}");
        assert!(counts.retired <= 2, "call {i}: {counts:?}");
        assert_eq!(
            counts.destroyed,
            counts.created - counts.held() - counts.retired,
            "call {i}: {counts:?}"
        );
    }
    let counts = fb.texture_counts();
    assert_eq!(counts.created, 2 * 20, "{counts:?}");
    assert_eq!(counts.destroyed, 2 * 20 - 2 - counts.retired, "{counts:?}");
}

/// A context with no readable surface texture renders a blend-mode frame
/// into a proxy and blits it onto the target with a uniform buffer made for
/// that draw; every frame's buffer is destroyed after its submit.
#[test]
fn a_blend_mode_proxy_blit_destroys_its_buffer_every_frame() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (96u32, 64u32);
    let target = Target::new(gpu.device().clone(), gpu.queue().clone(), w, h);
    let mut ctx = WgpuGfxCtx::new(
        gpu.device().clone(),
        gpu.queue().clone(),
        wgpu::TextureFormat::Rgba8Unorm,
        w as f32,
        h as f32,
    );
    for frame in 1..=10u64 {
        ctx.reset(w as f32, h as f32);
        ctx.clear(Color::white());
        paint(&mut ctx, w as f64, h as f64);
        ctx.flush_to_surface(&target.view);
        let buffers = ctx.buffer_counts();
        assert_eq!(buffers.let_go, frame, "frame {frame}: {buffers:?}");
        assert_eq!(buffers.waiting(), 0, "frame {frame}: {buffers:?}");
        assert_eq!(buffers.destroyed, frame, "frame {frame}: {buffers:?}");
    }
}

/// An SSAA blit into an encoder its caller submits hands its two per-draw
/// buffers to the caller, also when it blits twice into one encoder (each
/// blit has buffers of its own, so neither sees the other's quad); destroying
/// them after the submit leaves the frame valid.
#[test]
fn an_ssaa_blit_hands_its_buffers_to_the_caller_that_submits() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = Target::new(device.clone(), queue.clone(), 32, 32);
    let ctx = WgpuGfxCtx::new(device.clone(), queue.clone(), format, 32.0, 32.0);
    let mut fb = SsaaFramebuffer::new(device, 32, 32, format, false);
    for frame in 1..=10u64 {
        fb.ensure_size(device, 32, 32);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut buffers = Vec::new();
        for half in [0.0, 16.0] {
            fb.blit_downsample_retiring(
                device,
                &mut encoder,
                &target.view,
                (32, 32),
                agg_gui::Rect::new(half, 0.0, 16.0, 32.0),
                None,
                ctx.pipelines(),
                1,
                &mut |buffer| buffers.push(buffer),
            );
        }
        assert_eq!(
            buffers.len(),
            4,
            "frame {frame}: two blits, two buffers each"
        );
        queue.submit(std::iter::once(encoder.finish()));
        for buffer in buffers {
            buffer.destroy();
        }
    }
    let _ = target.read();
}
