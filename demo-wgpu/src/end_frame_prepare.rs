//! Phase-1 (prepare) of the deferred command flush — see `end_frame.rs`.
//!
//! Walks the [`DrawCommand`] list, allocates every GPU buffer / bind group /
//! transient texture each command needs, and stores them in [`Prepared`]
//! variants for the execute phase to dispatch.  A *size stack* mirrors the
//! layer push/pop sequence so each command's uniforms get the resolution of
//! whichever render target is current at that point in the list.
//!
//! ## Per-frame buffer pool
//!
//! All vertex / index / uniform data is written into three persistent
//! [`crate::buffer_arena::GpuArena`] instances owned by `WgpuGfxCtx`.  Each
//! allocation is a `queue.write_buffer` into a chunk that was created once
//! (per chunk) and is reused frame-after-frame.  This replaced the previous
//! `device.create_buffer_init(...)`-per-command pattern, which was the
//! dominant cost in `prepare_all` (~9 ms for ~213 commands on a release
//! build) — every `create_buffer_init` is a full GPU memory allocation
//! under a mutex inside the wgpu driver.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::buffer_arena::FrameArenas;
use crate::end_frame::{Prepared, PreparedSlice};
use crate::pipelines::{
    AaTexUniforms, LayerUniforms, LcbUniforms, LcdUniforms, SolidUniforms, TexUniforms,
    WgpuPipelines,
};
use crate::{DrawCommand, LayerRoundedClip};

/// Reinterpret a slice of `AaTexVertex` as raw bytes for upload.  The
/// type is `#[repr(C)]` with only `f32` fields (pos:vec2 + uv:vec2 =
/// 16 bytes, no padding, no Drop), so a flat byte view is sound.  We
/// hand-roll this rather than pulling `bytemuck` into the agg-gui crate
/// just for one type.
fn aa_tex_verts_as_bytes(verts: &[agg_gui::gl_renderer::AaTexVertex]) -> &[u8] {
    let ptr = verts.as_ptr() as *const u8;
    let len = std::mem::size_of_val(verts);
    // SAFETY: see doc above.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

pub(crate) fn prepare_all(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipelines: &WgpuPipelines,
    arenas: &mut FrameArenas,
    commands: &[DrawCommand],
    viewport: (f32, f32),
    aa_step_bg1: &Arc<wgpu::BindGroup>,
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

            DrawCommand::Solid {
                verts,
                indices,
                color,
                global_alpha,
                clip,
            } => {
                if verts.is_empty() || indices.is_empty() {
                    continue;
                }
                let a = (color.a * global_alpha).clamp(0.0, 1.0);
                let uniforms = SolidUniforms {
                    resolution: [cur_vp.0, cur_vp.1],
                    _pad: [0.0; 2],
                    color: [color.r, color.g, color.b, a],
                };
                let ub = alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&uniforms));
                let bg0 = mk_uniform_bg(device, &pipelines.solid_bgl, &ub);
                let vb = alloc_vertex(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(verts.as_slice()),
                );
                let ib = alloc_index(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(indices.as_slice()),
                );
                out.push(Prepared::Solid {
                    vb,
                    ib,
                    index_count: indices.len() as u32,
                    bg0,
                    clip: *clip,
                });
            }

            DrawCommand::AaSolid {
                verts,
                indices,
                color,
                global_alpha,
                clip,
            } => {
                if verts.is_empty() || indices.is_empty() {
                    continue;
                }
                let a = (color.a * global_alpha).clamp(0.0, 1.0);
                let uniforms = SolidUniforms {
                    resolution: [cur_vp.0, cur_vp.1],
                    _pad: [0.0; 2],
                    color: [color.r, color.g, color.b, a],
                };
                let ub = alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&uniforms));
                let bg0 = mk_uniform_bg(device, &pipelines.aa_solid_bgl, &ub);
                let vb = alloc_vertex(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(verts.as_slice()),
                );
                let ib = alloc_index(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(indices.as_slice()),
                );
                out.push(Prepared::AaSolid {
                    vb,
                    ib,
                    index_count: indices.len() as u32,
                    bg0,
                    clip: *clip,
                });
            }

            DrawCommand::AaTexture {
                verts,
                indices,
                color,
                global_alpha,
                clip,
            } => {
                if verts.is_empty() || indices.is_empty() {
                    continue;
                }
                let a = (color.a * global_alpha).clamp(0.0, 1.0);
                let uniforms = AaTexUniforms {
                    resolution: [cur_vp.0, cur_vp.1],
                    _pad: [0.0; 2],
                    color: [color.r, color.g, color.b, a],
                };
                let ub = alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&uniforms));
                let bg0 = mk_uniform_bg(device, &pipelines.aa_texture_bgl0, &ub);
                let vb = alloc_vertex(device, queue, arenas, aa_tex_verts_as_bytes(verts));
                let ib = alloc_index(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(indices.as_slice()),
                );
                out.push(Prepared::AaTexture {
                    vb,
                    ib,
                    index_count: indices.len() as u32,
                    bg0,
                    bg1: Arc::clone(aa_step_bg1),
                    clip: *clip,
                });
            }

            DrawCommand::Gradient {
                verts,
                indices,
                uniforms,
                ramp,
                clip,
            } => {
                if verts.is_empty() || indices.is_empty() {
                    continue;
                }
                let mut u = *uniforms;
                u.resolution = [cur_vp.0, cur_vp.1];
                let ub = alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&u));
                // Ramp texture is a one-off per command (data depends on the
                // gradient stops); textures aren't pooled here yet because
                // they're already arena-allocated inside wgpu and cost less
                // than per-call uniform/vertex buffer creation.
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
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    ramp,
                );
                let ramp_view = ramp_tex.create_view(&wgpu::TextureViewDescriptor::default());
                let bg0 = mk_uniform_bg(device, &pipelines.gradient_bgl0, &ub);
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
                let vb = alloc_vertex(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(verts.as_slice()),
                );
                let ib = alloc_index(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(indices.as_slice()),
                );
                out.push(Prepared::Gradient {
                    _ramp_tex: ramp_tex,
                    _ramp_view: ramp_view,
                    vb,
                    ib,
                    index_count: indices.len() as u32,
                    bg0,
                    bg1,
                    clip: *clip,
                });
            }

            DrawCommand::Textured {
                verts,
                texture,
                view,
                nearest,
                tint,
                clip,
            } => {
                let uniforms = TexUniforms {
                    resolution: [cur_vp.0, cur_vp.1],
                    _pad: [0.0; 2],
                    tint: *tint,
                };
                let ub = alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&uniforms));
                let bg0 = mk_uniform_bg(device, &pipelines.tex_bgl0, &ub);
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
                let vb = alloc_vertex(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(verts.as_slice()),
                );
                out.push(Prepared::Textured {
                    _texture: Arc::clone(texture),
                    _view: view.clone(),
                    vb,
                    bg0,
                    bg1,
                    clip: *clip,
                });
            }

            DrawCommand::LcdMask {
                verts,
                texture,
                view,
                color,
                clip,
            } => {
                let ubs: [PreparedSlice; 3] = std::array::from_fn(|ch| {
                    let u = LcdUniforms {
                        resolution: [cur_vp.0, cur_vp.1],
                        channel: ch as u32,
                        _pad: 0,
                        color: [color.r, color.g, color.b, color.a],
                    };
                    alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&u))
                });
                let bg0s: [wgpu::BindGroup; 3] =
                    std::array::from_fn(|ch| mk_uniform_bg(device, &pipelines.lcd_bgl0, &ubs[ch]));
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
                let vb = alloc_vertex(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(verts.as_slice()),
                );
                let ib = alloc_index(device, queue, arenas, bytemuck::cast_slice(&idx));
                out.push(Prepared::LcdMask {
                    _texture: Arc::clone(texture),
                    _view: view.clone(),
                    vb,
                    ib,
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
                let ubs: [PreparedSlice; 3] = std::array::from_fn(|ch| {
                    let u = LcbUniforms {
                        resolution: [cur_vp.0, cur_vp.1],
                        channel: ch as u32,
                        _pad: 0,
                    };
                    alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&u))
                });
                let bg0s: [wgpu::BindGroup; 3] =
                    std::array::from_fn(|ch| mk_uniform_bg(device, &pipelines.lcb_bgl0, &ubs[ch]));
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
                let vb = alloc_vertex(
                    device,
                    queue,
                    arenas,
                    bytemuck::cast_slice(verts.as_slice()),
                );
                let ib = alloc_index(device, queue, arenas, bytemuck::cast_slice(&idx));
                out.push(Prepared::LcbMask {
                    _color_tex: Arc::clone(color_tex),
                    _color_view: color_view.clone(),
                    _alpha_tex: Arc::clone(alpha_tex),
                    _alpha_view: alpha_view.clone(),
                    vb,
                    ib,
                    bg0s,
                    bg1,
                    clip: *clip,
                });
            }

            DrawCommand::PushLayer {
                texture,
                view,
                width,
                height,
            } => {
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
                    Some(LayerRoundedClip { x, y, w, h, r }) => ([*x, *y, *w, *h], *r, 1u32),
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
                let ub = alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&u));
                let bg0 = mk_uniform_bg(device, &pipelines.layer_bgl0, &ub);
                let bg1 = layer_texture_bg(
                    device,
                    &pipelines.layer_bgl1,
                    view,
                    &pipelines.linear_sampler,
                );
                let verts = composite_quad_verts(*origin_x, *origin_y, *layer_w, *layer_h);
                let vb = alloc_vertex(device, queue, arenas, bytemuck::cast_slice(&verts));
                out.push(Prepared::PopLayer {
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
                    Some(LayerRoundedClip { x, y, w, h, r }) => ([*x, *y, *w, *h], *r, 1u32),
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
                let ub = alloc_uniform(device, queue, arenas, bytemuck::bytes_of(&u));
                let bg0 = mk_uniform_bg(device, &pipelines.layer_bgl0, &ub);
                let bg1 = layer_texture_bg(
                    device,
                    &pipelines.layer_bgl1,
                    view,
                    &pipelines.linear_sampler,
                );
                let verts = composite_quad_verts(*origin_x, *origin_y, *layer_w, *layer_h);
                let vb = alloc_vertex(device, queue, arenas, bytemuck::cast_slice(&verts));
                out.push(Prepared::CompositeLayer {
                    _texture: Arc::clone(texture),
                    _view: view.clone(),
                    vb,
                    bg0,
                    bg1,
                });
            }

            DrawCommand::DrawBarGrid {
                renderer,
                screen_rect,
                parent_clip,
            } => {
                // Renderer owns its own pipeline + buffers; nothing to allocate
                // here.  Per-frame uniforms are built at execute time, when the
                // active render target's size is known.
                out.push(Prepared::DrawBarGrid {
                    renderer: std::rc::Rc::clone(renderer),
                    screen_rect: *screen_rect,
                    parent_clip: *parent_clip,
                });
            }

            DrawCommand::Custom {
                renderer,
                screen_rect,
                parent_clip,
            } => {
                // Identical structure to DrawBarGrid: pass-break + reopen.
                out.push(Prepared::Custom {
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
        x0, y0, 0.0, 1.0, x1, y0, 1.0, 1.0, x1, y1, 1.0, 0.0, x0, y0, 0.0, 1.0, x1, y1, 1.0, 0.0,
        x0, y1, 0.0, 0.0,
    ]
}

/// Bind group with a single uniform buffer at binding 0, using `slice`'s
/// arena buffer + offset + exact size (not `as_entire_binding` — uniforms
/// are packed back-to-back into one chunk, so a "rest-of-buffer" binding
/// would read past the end of our struct).
fn mk_uniform_bg(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    slice: &PreparedSlice,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(slice.uniform_binding()),
        }],
    })
}

#[inline]
fn alloc_vertex(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    arenas: &mut FrameArenas,
    data: &[u8],
) -> PreparedSlice {
    let (buf, offset, size) = arenas.vertex.alloc(device, queue, data);
    PreparedSlice { buf, offset, size }
}

#[inline]
fn alloc_index(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    arenas: &mut FrameArenas,
    data: &[u8],
) -> PreparedSlice {
    let (buf, offset, size) = arenas.index.alloc(device, queue, data);
    PreparedSlice { buf, offset, size }
}

#[inline]
fn alloc_uniform(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    arenas: &mut FrameArenas,
    data: &[u8],
) -> PreparedSlice {
    let (buf, offset, size) = arenas.uniform.alloc(device, queue, data);
    PreparedSlice { buf, offset, size }
}
