//! `end_frame` implementation — flushes all deferred [`DrawCommand`]s into a
//! single wgpu command submission.
//!
//! Uses a two-phase approach to satisfy wgpu's borrow rules:
//!
//! 1. **Prepare** — walk `commands`, allocate GPU buffers, build bind groups.
//!    All owned resources are collected in a `Vec<Prepared>`.
//! 2. **Execute** — open a `RenderPass`, walk the `Prepared` list, issue draw
//!    calls from immutably-borrowed resources.
//!
//! The `CommandEncoder` must be created AFTER all `wgpu::Buffer`s/`BindGroup`s
//! are owned (not borrowed) so that opening the `RenderPass` doesn't conflict
//! with resource creation.

use wgpu::util::DeviceExt;

use crate::pipelines::{SolidUniforms, WgpuPipelines};
use crate::{DrawCommand, WgpuGfxCtx};

// ---------------------------------------------------------------------------
// Per-command prepared GPU resources
// ---------------------------------------------------------------------------

/// Fully prepared GPU resources for one deferred draw command.
///
/// All wgpu objects that were borrowed to create bind groups are kept alive
/// here (prefixed with `_`) until after `queue.submit`.
enum Prepared {
    /// Pass-level clear handled via `LoadOp::Clear`.
    Clear(wgpu::Color),
    /// Solid-color triangles (no per-vertex alpha).
    Solid {
        _ub: wgpu::Buffer,
        vb: wgpu::Buffer,
        ib: wgpu::Buffer,
        index_count: u32,
        bg0: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// AA solid-color triangles (per-vertex alpha from tess2 halo strips).
    AaSolid {
        _ub: wgpu::Buffer,
        vb: wgpu::Buffer,
        ib: wgpu::Buffer,
        index_count: u32,
        bg0: wgpu::BindGroup,
        clip: Option<[i32; 4]>,
    },
    /// Linear or radial gradient fill.
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
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

impl WgpuGfxCtx {
    /// Flush all deferred draw commands to `surface_view`.
    ///
    /// Takes `self.commands` out so `device`/`queue`/`pipelines` can be
    /// borrowed freely during prepare + execute.
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
// Phase 1 — prepare GPU resources
// ---------------------------------------------------------------------------

fn prepare_all(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &WgpuPipelines,
    commands: &[DrawCommand],
    viewport: (f32, f32),
) -> Vec<Prepared> {
    let mut out = Vec::with_capacity(commands.len());

    for cmd in commands {
        match cmd {
            DrawCommand::Clear(color) => {
                out.push(Prepared::Clear(wgpu::Color {
                    r: color.r as f64,
                    g: color.g as f64,
                    b: color.b as f64,
                    a: color.a as f64,
                }));
            }

            DrawCommand::Solid { verts, indices, color, global_alpha, clip } => {
                if verts.is_empty() || indices.is_empty() {
                    continue;
                }
                let a = (color.a * global_alpha).clamp(0.0, 1.0);
                let uniforms = SolidUniforms {
                    resolution: [viewport.0, viewport.1],
                    _pad: [0.0; 2],
                    color: [color.r, color.g, color.b, a],
                };
                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&uniforms));
                let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.solid_bgl,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ub.as_entire_binding(),
                    }],
                });
                out.push(Prepared::Solid {
                    _ub: ub,
                    vb: mk_vertex_buf(device, bytemuck::cast_slice(verts.as_slice())),
                    ib: mk_index_buf(device, bytemuck::cast_slice(indices.as_slice())),
                    index_count: indices.len() as u32,
                    bg0,
                    clip: *clip,
                });
            }

            DrawCommand::AaSolid { verts, indices, color, global_alpha, clip } => {
                if verts.is_empty() || indices.is_empty() {
                    continue;
                }
                let a = (color.a * global_alpha).clamp(0.0, 1.0);
                let uniforms = SolidUniforms {
                    resolution: [viewport.0, viewport.1],
                    _pad: [0.0; 2],
                    color: [color.r, color.g, color.b, a],
                };
                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&uniforms));
                let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.aa_solid_bgl,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ub.as_entire_binding(),
                    }],
                });
                out.push(Prepared::AaSolid {
                    _ub: ub,
                    vb: mk_vertex_buf(device, bytemuck::cast_slice(verts.as_slice())),
                    ib: mk_index_buf(device, bytemuck::cast_slice(indices.as_slice())),
                    index_count: indices.len() as u32,
                    bg0,
                    clip: *clip,
                });
            }

            DrawCommand::Gradient { verts, indices, uniforms, ramp, clip } => {
                if verts.is_empty() || indices.is_empty() {
                    continue;
                }
                // Patch resolution into the gradient uniforms struct.  We can't
                // modify `uniforms` in place (it's immutable), so rebuild with the
                // correct resolution.
                let mut u = *uniforms;
                u.resolution = [viewport.0, viewport.1];

                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&u));
                let ramp_tex = device.create_texture_with_data(
                    queue,
                    &wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: crate::gradient::RAMP_W as u32,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    ramp,
                );
                let ramp_view =
                    ramp_tex.create_view(&wgpu::TextureViewDescriptor::default());

                let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.gradient_bgl0,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ub.as_entire_binding(),
                    }],
                });
                let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.gradient_bgl1,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&ramp_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(
                                &pipelines.linear_sampler,
                            ),
                        },
                    ],
                });
                out.push(Prepared::Gradient {
                    _ub: ub,
                    _ramp_tex: ramp_tex,
                    _ramp_view: ramp_view,
                    vb: mk_vertex_buf(device, bytemuck::cast_slice(verts.as_slice())),
                    ib: mk_index_buf(device, bytemuck::cast_slice(indices.as_slice())),
                    index_count: indices.len() as u32,
                    bg0,
                    bg1,
                    clip: *clip,
                });
            }

            // Later phases — skip gracefully.
            DrawCommand::Textured { .. }
            | DrawCommand::LcdMask { .. }
            | DrawCommand::LcbMask { .. }
            | DrawCommand::PushLayer { .. }
            | DrawCommand::PopLayer { .. }
            | DrawCommand::GlPaint { .. } => {}
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Phase 2 — execute in render pass
// ---------------------------------------------------------------------------

fn execute_prepared(
    encoder: &mut wgpu::CommandEncoder,
    target_view: &wgpu::TextureView,
    pipelines: &WgpuPipelines,
    prepared: &[Prepared],
    viewport: (f32, f32),
) {
    // Find the last Clear before any draw — use it as LoadOp so the clear is
    // free on tile-based GPUs rather than a separate pass.
    let init_clear: wgpu::Color = {
        let mut found = None;
        for item in prepared {
            match item {
                Prepared::Clear(c) => found = Some(*c),
                _ => break,
            }
        }
        found.unwrap_or(wgpu::Color::TRANSPARENT)
    };
    let has_clear = matches!(prepared.first(), Some(Prepared::Clear(_)));
    let load_op = if has_clear {
        wgpu::LoadOp::Clear(init_clear)
    } else {
        wgpu::LoadOp::Load
    };

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("main"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations { load: load_op, store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });

    let vp_w = viewport.0 as u32;
    let vp_h = viewport.1 as u32;
    pass.set_viewport(0.0, 0.0, viewport.0, viewport.1, 0.0, 1.0);

    for item in prepared {
        match item {
            Prepared::Clear(_) => {
                // Handled via LoadOp above.  Mid-frame Clears (after a draw) are
                // a Phase 8 concern — skipped for now.
            }

            Prepared::Solid { vb, ib, index_count, bg0, clip, .. } => {
                apply_clip(&mut pass, *clip, vp_w, vp_h);
                pass.set_pipeline(&pipelines.solid_pipeline);
                pass.set_bind_group(0, bg0, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }

            Prepared::AaSolid { vb, ib, index_count, bg0, clip, .. } => {
                apply_clip(&mut pass, *clip, vp_w, vp_h);
                pass.set_pipeline(&pipelines.aa_solid_pipeline);
                pass.set_bind_group(0, bg0, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }

            Prepared::Gradient { vb, ib, index_count, bg0, bg1, clip, .. } => {
                apply_clip(&mut pass, *clip, vp_w, vp_h);
                pass.set_pipeline(&pipelines.gradient_pipeline);
                pass.set_bind_group(0, bg0, &[]);
                pass.set_bind_group(1, bg1, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*index_count, 0, 0..1);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scissor helper
// ---------------------------------------------------------------------------

fn apply_clip(
    pass: &mut wgpu::RenderPass,
    clip: Option<[i32; 4]>,
    vp_w: u32,
    vp_h: u32,
) {
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

// ---------------------------------------------------------------------------
// Buffer allocation helpers
// ---------------------------------------------------------------------------

fn mk_vertex_buf(device: &wgpu::Device, data: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: data,
        usage: wgpu::BufferUsages::VERTEX,
    })
}

fn mk_index_buf(device: &wgpu::Device, data: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: data,
        usage: wgpu::BufferUsages::INDEX,
    })
}

fn mk_uniform_buf(device: &wgpu::Device, data: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: data,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}
