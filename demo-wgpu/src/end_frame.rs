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

use wgpu::util::DeviceExt;

use crate::pipelines::{LayerUniforms, LcbUniforms, LcdUniforms, SolidUniforms, TexUniforms, WgpuPipelines};
use crate::{DrawCommand, LayerRoundedClip, WgpuGfxCtx};

// ---------------------------------------------------------------------------
// Per-command prepared GPU resources
// ---------------------------------------------------------------------------

enum Prepared {
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
// Phase 1 — prepare GPU resources
// ---------------------------------------------------------------------------

fn prepare_all(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &WgpuPipelines,
    commands: &[DrawCommand],
    viewport: (f32, f32),
) -> Vec<Prepared> {
    // size_stack mirrors the layer stack at execute time — top of stack is the
    // viewport size that draws inside that target should use for their NDC math.
    let mut size_stack: Vec<(f32, f32)> = vec![viewport];
    let mut out: Vec<Prepared> = Vec::with_capacity(commands.len());

    for cmd in commands {
        let cur_vp = *size_stack.last().unwrap_or(&viewport);

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
                    resolution: [cur_vp.0, cur_vp.1],
                    _pad: [0.0; 2],
                    color: [color.r, color.g, color.b, a],
                };
                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&uniforms));
                let bg0 = mk_bg(device, &pipelines.solid_bgl, &ub);
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
                    resolution: [cur_vp.0, cur_vp.1],
                    _pad: [0.0; 2],
                    color: [color.r, color.g, color.b, a],
                };
                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&uniforms));
                let bg0 = mk_bg(device, &pipelines.aa_solid_bgl, &ub);
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
                let mut u = *uniforms;
                u.resolution = [cur_vp.0, cur_vp.1];
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
                let ramp_view = ramp_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let bg0 = mk_bg(device, &pipelines.gradient_bgl0, &ub);
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
                            resource: wgpu::BindingResource::Sampler(&pipelines.linear_sampler),
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

            DrawCommand::Textured { verts, texture, view, nearest, clip } => {
                let uniforms = TexUniforms {
                    resolution: [cur_vp.0, cur_vp.1],
                    _pad: [0.0; 2],
                };
                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&uniforms));
                let bg0 = mk_bg(device, &pipelines.tex_bgl0, &ub);
                let sampler = if *nearest {
                    &pipelines.nearest_sampler
                } else {
                    &pipelines.linear_sampler
                };
                let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.tex_bgl1,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                    ],
                });
                let vb = mk_vertex_buf(device, bytemuck::cast_slice(verts.as_slice()));
                out.push(Prepared::Textured {
                    _ub: ub,
                    _texture: Arc::clone(texture),
                    _view: view.clone(),
                    vb,
                    bg0,
                    bg1,
                    clip: *clip,
                });
            }

            DrawCommand::LcdMask { verts, texture, view, color, clip } => {
                let ubs: [wgpu::Buffer; 3] = std::array::from_fn(|ch| {
                    let u = LcdUniforms {
                        resolution: [cur_vp.0, cur_vp.1],
                        channel: ch as u32,
                        _pad: 0,
                        color: [color.r, color.g, color.b, color.a],
                    };
                    mk_uniform_buf(device, bytemuck::bytes_of(&u))
                });
                let bg0s: [wgpu::BindGroup; 3] =
                    std::array::from_fn(|ch| mk_bg(device, &pipelines.lcd_bgl0, &ubs[ch]));
                let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.lcd_bgl1,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipelines.nearest_sampler),
                        },
                    ],
                });
                let idx: [u32; 6] = [0, 1, 2, 0, 2, 3];
                out.push(Prepared::LcdMask {
                    _ubs: ubs,
                    _texture: Arc::clone(texture),
                    _view: view.clone(),
                    vb: mk_vertex_buf(device, bytemuck::cast_slice(verts.as_slice())),
                    ib: mk_index_buf(device, bytemuck::cast_slice(&idx)),
                    bg0s,
                    bg1,
                    clip: *clip,
                });
            }

            DrawCommand::LcbMask {
                verts,
                color_tex,
                color_view,
                alpha_tex,
                alpha_view,
                clip,
            } => {
                let ubs: [wgpu::Buffer; 3] = std::array::from_fn(|ch| {
                    let u = LcbUniforms {
                        resolution: [cur_vp.0, cur_vp.1],
                        channel: ch as u32,
                        _pad: 0,
                    };
                    mk_uniform_buf(device, bytemuck::bytes_of(&u))
                });
                let bg0s: [wgpu::BindGroup; 3] =
                    std::array::from_fn(|ch| mk_bg(device, &pipelines.lcb_bgl0, &ubs[ch]));
                let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.lcb_bgl1,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(color_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(alpha_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&pipelines.nearest_sampler),
                        },
                    ],
                });
                let idx: [u32; 6] = [0, 1, 2, 0, 2, 3];
                out.push(Prepared::LcbMask {
                    _ubs: ubs,
                    _color_tex: Arc::clone(color_tex),
                    _color_view: color_view.clone(),
                    _alpha_tex: Arc::clone(alpha_tex),
                    _alpha_view: alpha_view.clone(),
                    vb: mk_vertex_buf(device, bytemuck::cast_slice(verts.as_slice())),
                    ib: mk_index_buf(device, bytemuck::cast_slice(&idx)),
                    bg0s,
                    bg1,
                    clip: *clip,
                });
            }

            DrawCommand::PushLayer { texture, view, width, height } => {
                size_stack.push((*width as f32, *height as f32));
                out.push(Prepared::PushLayer {
                    _texture: Arc::clone(texture),
                    view: view.clone(),
                    size: (*width, *height),
                });
            }

            DrawCommand::PopLayer {
                texture,
                view,
                origin_x,
                origin_y,
                layer_w,
                layer_h,
                alpha,
                rounded_clip,
            } => {
                size_stack.pop();
                let parent_vp = *size_stack.last().unwrap_or(&viewport);
                let (mask_rect, mask_radius, mask_enabled) = match rounded_clip {
                    Some(LayerRoundedClip { x, y, w, h, r }) => {
                        ([*x, *y, *w, *h], *r, 1u32)
                    }
                    None => ([0.0; 4], 0.0, 0u32),
                };
                let u = LayerUniforms {
                    resolution: [parent_vp.0, parent_vp.1],
                    alpha: *alpha,
                    mask_enabled,
                    layer_size: [*layer_w as f32, *layer_h as f32],
                    mask_radius,
                    _pad0: 0.0,
                    mask_rect,
                };
                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&u));
                let bg0 = mk_bg(device, &pipelines.layer_bgl0, &ub);
                let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.layer_bgl1,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipelines.linear_sampler),
                        },
                    ],
                });

                // Composite quad in parent's Y-up local coords.  bl=v=1, tr=v=0
                // mirrors the wgpu UV convention (v=0 is top of a sampled texture).
                let x0 = *origin_x;
                let y0 = *origin_y;
                let x1 = x0 + *layer_w as f32;
                let y1 = y0 + *layer_h as f32;
                let verts: [f32; 24] = [
                    x0, y0, 0.0, 1.0,
                    x1, y0, 1.0, 1.0,
                    x1, y1, 1.0, 0.0,
                    x0, y0, 0.0, 1.0,
                    x1, y1, 1.0, 0.0,
                    x0, y1, 0.0, 0.0,
                ];
                let vb = mk_vertex_buf(device, bytemuck::cast_slice(&verts));
                out.push(Prepared::PopLayer {
                    _ub: ub,
                    _texture: Arc::clone(texture),
                    _view: view.clone(),
                    vb,
                    bg0,
                    bg1,
                });
            }

            DrawCommand::CompositeLayer {
                texture,
                view,
                origin_x,
                origin_y,
                layer_w,
                layer_h,
                alpha,
                rounded_clip,
            } => {
                // Same uniform/bind-group setup as PopLayer but no stack change
                // — current target's resolution is `cur_vp`.
                let (mask_rect, mask_radius, mask_enabled) = match rounded_clip {
                    Some(LayerRoundedClip { x, y, w, h, r }) => {
                        ([*x, *y, *w, *h], *r, 1u32)
                    }
                    None => ([0.0; 4], 0.0, 0u32),
                };
                let u = LayerUniforms {
                    resolution: [cur_vp.0, cur_vp.1],
                    alpha: *alpha,
                    mask_enabled,
                    layer_size: [*layer_w as f32, *layer_h as f32],
                    mask_radius,
                    _pad0: 0.0,
                    mask_rect,
                };
                let ub = mk_uniform_buf(device, bytemuck::bytes_of(&u));
                let bg0 = mk_bg(device, &pipelines.layer_bgl0, &ub);
                let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipelines.layer_bgl1,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&pipelines.linear_sampler),
                        },
                    ],
                });
                let x0 = *origin_x;
                let y0 = *origin_y;
                let x1 = x0 + *layer_w as f32;
                let y1 = y0 + *layer_h as f32;
                let verts: [f32; 24] = [
                    x0, y0, 0.0, 1.0,
                    x1, y0, 1.0, 1.0,
                    x1, y1, 1.0, 0.0,
                    x0, y0, 0.0, 1.0,
                    x1, y1, 1.0, 0.0,
                    x0, y1, 0.0, 0.0,
                ];
                let vb = mk_vertex_buf(device, bytemuck::cast_slice(&verts));
                out.push(Prepared::CompositeLayer {
                    _ub: ub,
                    _texture: Arc::clone(texture),
                    _view: view.clone(),
                    vb,
                    bg0,
                    bg1,
                });
            }

            DrawCommand::GlPaint { .. } => {
                // Phase 9 — skip for now.
            }
        }
    }

    out
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

fn mk_bg(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

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
