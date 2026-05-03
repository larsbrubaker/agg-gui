//! Phase-1 (prepare) of the deferred command flush — see `end_frame.rs`.
//!
//! Walks the [`DrawCommand`] list, allocates every GPU buffer / bind group /
//! transient texture each command needs, and stores them in [`Prepared`]
//! variants for the execute phase to dispatch.  A *size stack* mirrors the
//! layer push/pop sequence so each command's uniforms get the resolution of
//! whichever render target is current at that point in the list.
//!
//! Pulled out of `end_frame.rs` to keep that file under the project's 800-line
//! limit; the two halves of the flush share the [`Prepared`] enum (defined in
//! `end_frame.rs`) but have no other coupling.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::end_frame::Prepared;
use crate::pipelines::{
    LayerUniforms, LcbUniforms, LcdUniforms, SolidUniforms, TexUniforms, WgpuPipelines,
};
use crate::{DrawCommand, LayerRoundedClip};

pub(crate) fn prepare_all(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &WgpuPipelines,
    commands: &[DrawCommand],
    viewport: (f32, f32),
) -> Vec<Prepared> {
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
                let bg1 = layer_texture_bg(device, &pipelines.layer_bgl1, view, &pipelines.linear_sampler);
                let verts = composite_quad_verts(*origin_x, *origin_y, *layer_w, *layer_h);
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
                let bg1 = layer_texture_bg(device, &pipelines.layer_bgl1, view, &pipelines.linear_sampler);
                let verts = composite_quad_verts(*origin_x, *origin_y, *layer_w, *layer_h);
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

            DrawCommand::DrawBarGrid { renderer, screen_rect, parent_clip } => {
                // Renderer owns its own pipeline + buffers; nothing to allocate
                // here.  Per-frame uniforms are built at execute time, when the
                // active render target's size is known.
                out.push(Prepared::DrawBarGrid {
                    renderer: std::rc::Rc::clone(renderer),
                    screen_rect: *screen_rect,
                    parent_clip: *parent_clip,
                });
            }
        }
    }

    out
}

/// Bind group with layer texture + sampler — shared between PopLayer and
/// CompositeLayer prepare paths.
fn layer_texture_bg(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
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
    })
}

/// Composite quad in parent's Y-up local coords.  `bl=v=1, tr=v=0` mirrors the
/// wgpu UV convention (v=0 is the top of a sampled texture).
fn composite_quad_verts(origin_x: f32, origin_y: f32, layer_w: u32, layer_h: u32) -> [f32; 24] {
    let x0 = origin_x;
    let y0 = origin_y;
    let x1 = x0 + layer_w as f32;
    let y1 = y0 + layer_h as f32;
    [
        x0, y0, 0.0, 1.0,
        x1, y0, 1.0, 1.0,
        x1, y1, 1.0, 0.0,
        x0, y0, 0.0, 1.0,
        x1, y1, 1.0, 0.0,
        x0, y1, 0.0, 0.0,
    ]
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
