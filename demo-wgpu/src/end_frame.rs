//! `end_frame` implementation — flushes all deferred [`DrawCommand`]s into a
//! single wgpu command submission.
//!
//! Two-phase approach to satisfy wgpu's borrow rules:
//!
//! 1. **Prepare** — walk `commands`, allocate GPU buffers, build bind groups.
//!    All owned resources are collected in a `Vec<Prepared>`.  A *size stack*
//!    is simulated so each command's uniforms get the resolution of whichever
//!    render target is current at that point in the command list.
//! 2. **Execute** — open a `RenderPass` per render target, walk the `Prepared`
//!    list, and issue draw calls.  PushLayer/PopLayer end the current pass and
//!    start a new one on the layer texture or parent target.
//!
//! Multi-pass orchestration: each layer push/pop boundary is a render-pass
//! boundary in wgpu (a `RenderPass<'enc>` exclusively borrows its encoder, so
//! switching attachments requires ending and re-beginning the pass).

use std::sync::Arc;

use crate::end_frame_prepare::prepare_all;
use crate::pipelines::WgpuPipelines;
use crate::WgpuGfxCtx;

// ---------------------------------------------------------------------------
// Per-command prepared GPU resources
// ---------------------------------------------------------------------------

pub(crate) enum Prepared {
    /// Pass-level clear — handled via `LoadOp::Clear` on the next pass open.
    Clear(wgpu::Color),
    /// Solid colour (no AA).
    Solid {
        _ub: wgpu::Buffer,
        vb: wgpu::Buffer,
        ib: wgpu::Buffer,
        index_count: u32,
        bg0: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// AA solid (per-vertex alpha from tess2 halo strips).
    AaSolid {
        _ub: wgpu::Buffer,
        vb: wgpu::Buffer,
        ib: wgpu::Buffer,
        index_count: u32,
        bg0: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// Linear or radial gradient.
    Gradient {
        _ub: wgpu::Buffer,
        _ramp_tex: wgpu::Texture,
        _ramp_view: wgpu::TextureView,
        vb: wgpu::Buffer,
        ib: wgpu::Buffer,
        index_count: u32,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// Textured quad (image blit).
    Textured {
        _ub: wgpu::Buffer,
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: wgpu::Buffer,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// LCD subpixel mask (3-pass).
    LcdMask {
        _ubs: [wgpu::Buffer; 3],
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: wgpu::Buffer,
        ib: wgpu::Buffer,
        bg0s: [wgpu::BindGroup; 3],
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// LCD backbuffer (3-pass, two-plane input).
    LcbMask {
        _ubs: [wgpu::Buffer; 3],
        _color_tex: Arc<wgpu::Texture>,
        _color_view: wgpu::TextureView,
        _alpha_tex: Arc<wgpu::Texture>,
        _alpha_view: wgpu::TextureView,
        vb: wgpu::Buffer,
        ib: wgpu::Buffer,
        bg0s: [wgpu::BindGroup; 3],
        bg1: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// Begin rendering into a new layer texture.
    PushLayer {
        _texture: Arc<wgpu::Texture>,
        view: wgpu::TextureView,
        size: (u32, u32),
    },
    /// End layer rendering and composite onto the parent target.
    PopLayer {
        _ub: wgpu::Buffer,
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: wgpu::Buffer,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
    },
    /// Composite a retained layer onto the current target — no layer-stack
    /// change.
    CompositeLayer {
        _ub: wgpu::Buffer,
        _texture: Arc<wgpu::Texture>,
        _view: wgpu::TextureView,
        vb: wgpu::Buffer,
        bg0: wgpu::BindGroup,
        bg1: wgpu::BindGroup,
    },
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

impl WgpuGfxCtx {
    pub(crate) fn flush_to_surface(&mut self, surface_view: &wgpu::TextureView) {
        let commands = std::mem::take(&mut self.commands);

        let prepared = prepare_all(
            &self.device,
            &self.queue,
            &self.pipelines,
            &commands,
            self.viewport,
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

        execute_prepared(
            &mut encoder,
            surface_view,
            &self.pipelines,
            &prepared,
            self.viewport,
        );

        self.queue.submit(std::iter::once(encoder.finish()));
    }
}
// ---------------------------------------------------------------------------
// Phase 2 — execute in render passes
// ---------------------------------------------------------------------------

fn execute_prepared<'a>(
    encoder: &mut wgpu::CommandEncoder,
    surface_view: &'a wgpu::TextureView,
    pipelines: &WgpuPipelines,
    prepared: &'a [Prepared],
    surface_viewport: (f32, f32),
) {
    // Initial clear: only honoured if the very first command is Clear.  Mid-frame
    // clears (after a draw) are skipped — the layer system makes them rare.
    let init_clear = match prepared.first() {
        Some(Prepared::Clear(c)) => Some(*c),
        _ => None,
    };

    // Stack of `(target_view, viewport_size)`.  Borrowed from `surface_view` (root)
    // or `Prepared::PushLayer.view` for active layers.
    let mut target_stack: Vec<(&'a wgpu::TextureView, (f32, f32))> =
        vec![(surface_view, surface_viewport)];

    let mut load_op: wgpu::LoadOp<wgpu::Color> = match init_clear {
        Some(c) => wgpu::LoadOp::Clear(c),
        None => wgpu::LoadOp::Load,
    };

    // After a PopLayer we must emit a composite quad at the start of the parent's
    // resumed pass — captured here between the closed layer pass and the reopened
    // parent pass.  The references point into `prepared`.
    let mut pending_composite: Option<(&'a wgpu::Buffer, &'a wgpu::BindGroup, &'a wgpu::BindGroup)> =
        None;

    let mut i = 0usize;

    // Each iteration of the outer loop runs exactly one render pass.  The inner
    // block scopes the pass so the encoder borrow ends when we exit it.
    while i < prepared.len() || pending_composite.is_some() {
        let &(target_view, target_vp) = target_stack.last().unwrap();

        {
            let mut pass = begin_pass(encoder, target_view, load_op);
            pass.set_viewport(0.0, 0.0, target_vp.0, target_vp.1, 0.0, 1.0);

            // First, if a PopLayer is pending, emit its composite quad at the
            // start of this resumed parent pass.
            if let Some((vb, bg0, bg1)) = pending_composite.take() {
                pass.set_scissor_rect(0, 0, target_vp.0 as u32, target_vp.1 as u32);
                pass.set_pipeline(&pipelines.layer_pipeline);
                pass.set_bind_group(0, bg0, &[]);
                pass.set_bind_group(1, bg1, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..6, 0..1);
            }

            // Drive the pass forward until end-of-list or a layer boundary.
            while i < prepared.len() {
                match &prepared[i] {
                    Prepared::PushLayer { .. } | Prepared::PopLayer { .. } => break,
                    other => {
                        execute_one(&mut pass, pipelines, other, target_vp);
                        i += 1;
                    }
                }
            }
            // pass is dropped here, releasing the encoder borrow.
        }

        // Subsequent passes use Load by default.
        load_op = wgpu::LoadOp::Load;

        // Process the boundary command (if any) to set up the next pass's state.
        if i < prepared.len() {
            match &prepared[i] {
                Prepared::PushLayer { view, size, .. } => {
                    target_stack.push((view, (size.0 as f32, size.1 as f32)));
                    load_op = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
                    i += 1;
                }
                Prepared::PopLayer { vb, bg0, bg1, .. } => {
                    target_stack.pop();
                    pending_composite = Some((vb, bg0, bg1));
                    i += 1;
                }
                _ => unreachable!("loop only breaks on layer boundary commands"),
            }
        }
    }
}

/// Issue draw calls for a single non-layer-boundary prepared command into an
/// open render pass.
fn execute_one(
    pass: &mut wgpu::RenderPass,
    pipelines: &WgpuPipelines,
    item: &Prepared,
    vp: (f32, f32),
) {
    match item {
        Prepared::Clear(_) => {
            // LoadOp::Clear was used at pass open; mid-frame Clears ignored.
        }
        Prepared::Solid { vb, ib, index_count, bg0, clip, .. } => {
            apply_clip(pass, *clip, vp);
            pass.set_pipeline(&pipelines.solid_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*index_count, 0, 0..1);
        }
        Prepared::AaSolid { vb, ib, index_count, bg0, clip, .. } => {
            apply_clip(pass, *clip, vp);
            pass.set_pipeline(&pipelines.aa_solid_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*index_count, 0, 0..1);
        }
        Prepared::Gradient { vb, ib, index_count, bg0, bg1, clip, .. } => {
            apply_clip(pass, *clip, vp);
            pass.set_pipeline(&pipelines.gradient_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*index_count, 0, 0..1);
        }
        Prepared::Textured { vb, bg0, bg1, clip, .. } => {
            apply_clip(pass, *clip, vp);
            pass.set_pipeline(&pipelines.tex_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.draw(0..6, 0..1);
        }
        Prepared::LcdMask { vb, ib, bg0s, bg1, clip, .. } => {
            apply_clip(pass, *clip, vp);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            let lcd_pipelines = [&pipelines.lcd_r, &pipelines.lcd_g, &pipelines.lcd_b];
            for ch in 0..3 {
                pass.set_pipeline(lcd_pipelines[ch]);
                pass.set_bind_group(0, &bg0s[ch], &[]);
                pass.draw_indexed(0..6, 0, 0..1);
            }
        }
        Prepared::LcbMask { vb, ib, bg0s, bg1, clip, .. } => {
            apply_clip(pass, *clip, vp);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            let lcb_pipelines = [&pipelines.lcb_r, &pipelines.lcb_g, &pipelines.lcb_b];
            for ch in 0..3 {
                pass.set_pipeline(lcb_pipelines[ch]);
                pass.set_bind_group(0, &bg0s[ch], &[]);
                pass.draw_indexed(0..6, 0, 0..1);
            }
        }
        Prepared::CompositeLayer { vb, bg0, bg1, .. } => {
            // Composite a retained layer onto the current target — no stack
            // change, full target as scissor.
            pass.set_scissor_rect(0, 0, vp.0 as u32, vp.1 as u32);
            pass.set_pipeline(&pipelines.layer_pipeline);
            pass.set_bind_group(0, bg0, &[]);
            pass.set_bind_group(1, bg1, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.draw(0..6, 0..1);
        }
        // Layer boundaries are handled in the outer driver, not here.
        Prepared::PushLayer { .. } | Prepared::PopLayer { .. } => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

fn apply_clip(pass: &mut wgpu::RenderPass, clip: Option<[i32; 4]>, vp: (f32, f32)) {
    let vp_w = vp.0 as u32;
    let vp_h = vp.1 as u32;
    if let Some(scissor) = clip {
        let (x, y, w, h) = WgpuGfxCtx::yup_to_ydown_scissor(scissor, vp_h);
        let w = w.min(vp_w.saturating_sub(x));
        let h = h.min(vp_h.saturating_sub(y));
        if w > 0 && h > 0 {
            pass.set_scissor_rect(x, y, w, h);
        }
    } else {
        pass.set_scissor_rect(0, 0, vp_w, vp_h);
    }
}
