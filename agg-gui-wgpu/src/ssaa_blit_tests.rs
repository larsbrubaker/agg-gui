//! Tests for the bind groups of an SSAA blit (`ssaa_blit_binds.rs`): a
//! framebuffer blitted every frame, more than once a frame, makes no new
//! bind groups once the first frame has made its own, and every blit still
//! draws with its own uniforms.
//!
//! Why this matters: in Firefox a bind group is freed only when JavaScript's
//! garbage collector collects it, from a fixed per-device descriptor heap, so
//! the two bind groups a blit made every frame (the tumble cube, a shell's
//! offscreen scene) lost the device after enough minutes of redraws. Like
//! the other headless GPU tests these skip (pass trivially) when no adapter
//! is available.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::Rect;

use crate::headless::{HeadlessFrame, HeadlessGpu};
use crate::layer_text_readback_tests::{px, Target};
use crate::{SsaaFramebuffer, WgpuCustomRender, WgpuCustomRenderCtx, WgpuGfxCtx};

const RED: [u8; 4] = [255, 0, 0, 255];
const FB_SIDE: u32 = 8;

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

/// Fill the framebuffer with opaque red.
fn clear_red(encoder: &mut wgpu::CommandEncoder, fb: &SsaaFramebuffer) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ssaa-blit-test-clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: fb.render_view(),
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

/// A 3-D view's pass as the tumble cube makes it: keep the framebuffer
/// sized, draw into it, blit it onto the pass's target.
struct RedBlit {
    fb: Option<SsaaFramebuffer>,
}

impl WgpuCustomRender for RedBlit {
    fn render(&mut self, mut ctx: WgpuCustomRenderCtx<'_>) {
        let fb = match &mut self.fb {
            Some(fb) => {
                fb.ensure_size(ctx.device, FB_SIDE, FB_SIDE);
                fb
            }
            None => self.fb.insert(SsaaFramebuffer::new(
                ctx.device,
                FB_SIDE,
                FB_SIDE,
                wgpu::TextureFormat::Rgba8Unorm,
                false,
            )),
        };
        clear_red(ctx.encoder, fb);
        fb.blit_downsample_in(&mut ctx, 1);
    }
}

/// The same pass drawn twice a frame: once inside a 40 x 30 layer and once
/// on the 128 x 96 frame, so the two blits' uniforms (their target's size)
/// differ. If the second blit's uniforms reached the first, the first would
/// map its quad by the frame's size and miss the corner it should cover.
#[test]
fn a_blit_drawn_twice_a_frame_makes_no_bind_groups_after_the_first_frame() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (128u32, 96u32);
    let blit = Rc::new(RefCell::new(RedBlit { fb: None }));
    let mut frame = HeadlessFrame::new(gpu, w, h);
    let paint = |ctx: &mut WgpuGfxCtx| {
        ctx.push_layer(40.0, 30.0);
        ctx.push_custom_render(blit.clone(), Rect::new(0.0, 0.0, 8.0, 8.0));
        ctx.pop_layer();
        ctx.push_custom_render(blit.clone(), Rect::new(64.0, 40.0, 8.0, 8.0));
    };
    frame.render(Color::white(), paint);
    let counts = |blit: &Rc<RefCell<RedBlit>>| {
        let blit = blit.borrow();
        blit.fb.as_ref().map(|fb| fb.bind_group_counts())
    };
    let warm = counts(&blit).expect("the pass made its framebuffer");
    for i in 1..=4 {
        frame.render(Color::white(), paint);
        let now = counts(&blit).expect("the framebuffer is kept");
        assert_eq!(
            now.created(),
            warm.created(),
            "frame {i}: a steady frame's blits make no bind groups: {now:?} after {warm:?}"
        );
        let data = frame.read_rgba().expect("read back");
        assert_eq!(
            px(&data, w, 4, h - 5),
            RED,
            "frame {i}: the blit inside the layer covers its corner"
        );
        assert_eq!(
            px(&data, w, 68, h - 45),
            RED,
            "frame {i}: the blit on the frame covers its rect"
        );
    }
}

/// Blits into one encoder its caller submits, each onto a target of its own
/// size, keep their own uniforms, also when a frame blits more often than
/// the framebuffer has uniform slots (six blits, four slots: the first frame
/// grows them); the frames after the first make no bind groups.
#[test]
fn blits_recorded_into_one_encoder_keep_their_own_uniforms() {
    let Some(gpu) = gpu() else { return };
    let (device, queue) = (gpu.device(), gpu.queue());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let ctx = WgpuGfxCtx::new(Arc::clone(device), Arc::clone(queue), format, 64.0, 64.0);
    // Smallest first: were a later, larger blit's uniforms to reach an
    // earlier one, its 8 x 8 quad would shrink and miss pixel 7.
    let sides = [16u32, 24, 32, 40, 48, 64];
    let targets: Vec<Target> = sides
        .iter()
        .map(|&side| Target::new(Arc::clone(device), Arc::clone(queue), side, side))
        .collect();
    let mut fb = SsaaFramebuffer::new(device, FB_SIDE, FB_SIDE, format, false);
    let mut warm = None;
    for frame in 1..=4 {
        fb.ensure_size(device, FB_SIDE, FB_SIDE);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        clear_red(&mut encoder, &fb);
        let mut buffers = Vec::new();
        for (target, &side) in targets.iter().zip(&sides) {
            fb.blit_downsample_retiring(
                device,
                queue,
                &mut encoder,
                &target.view,
                (side, side),
                Rect::new(0.0, 0.0, 8.0, 8.0),
                None,
                ctx.pipelines(),
                1,
                &mut |buffer| buffers.push(buffer),
            );
        }
        queue.submit(std::iter::once(encoder.finish()));
        for buffer in buffers {
            buffer.destroy();
        }
        for (target, &side) in targets.iter().zip(&sides) {
            assert_eq!(
                px(&target.read(), side, 7, side - 8),
                RED,
                "frame {frame}: the {side} x {side} target's blit covers its 8 x 8 corner"
            );
        }
        let counts = fb.bind_group_counts();
        match warm {
            None => warm = Some(counts),
            Some(warm) => assert_eq!(
                counts.created(),
                warm.created(),
                "frame {frame}: {counts:?} after {warm:?}"
            ),
        }
    }
}
