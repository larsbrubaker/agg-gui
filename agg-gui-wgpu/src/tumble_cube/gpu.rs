//! GPU resources for [`super::WgpuTumbleCubeRenderer`]: the scene-module
//! pipeline, cube buffers, the three uniform blocks and the six face
//! textures (with CPU-built mip chains, as the C# preloads them mipmapped).
//!
//! Adapted from AtomArtist's `tumble_cube/renderer.rs` (pipeline, mip
//! chain and per-face bind groups), retargeted at MatterCAD's
//! `NodeDesignerScene.wgsl` bind layout and vertex format.

use std::sync::Arc;

use agg_gui::widgets::tumble_cube::{CubeView, FACE_SIZE};
use wgpu::util::DeviceExt;

use super::{cube_vertices, effect_block, lights_block, rows, SCENE_WGSL};

const MIP_COUNT: u32 = 9; // 256 → 1
/// position 3 + normal 3 + uv 2 + edge hints 3 + colour 4 floats.
const VERTEX_STRIDE: u64 = 15 * 4;

pub(crate) struct GpuState {
    pub(crate) format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    transform: wgpu::Buffer,
    lights: wgpu::Buffer,
    effect: wgpu::Buffer,
    textures: Vec<wgpu::Texture>,
    bind_groups: Vec<wgpu::BindGroup>,
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_buffer(device: &wgpu::Device, label: &str, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl GpuState {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tumble cube scene"),
            source: wgpu::ShaderSource::Wgsl(SCENE_WGSL.into()),
        });
        // Only the bindings the two entry points touch; the module's
        // peel / bed bindings stay out, as its header allows.
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tumble cube bgl"),
            entries: &[
                uniform_entry(0),
                uniform_entry(1),
                uniform_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tumble cube pl"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let attrs = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 12,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 24,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 32,
                shader_location: 3,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: 44,
                shader_location: 4,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tumble cube pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("sceneVertexMain"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                }],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                // The C# draws the cube with `forceCullBackFaces: true`.
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("sceneTextureMain"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let (verts, indices) = cube_vertices();
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tumble cube vb"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tumble cube ib"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let transform = uniform_buffer(device, "tumble cube transform", 128);
        let lights = uniform_buffer(device, "tumble cube lights", 128);
        let effect = uniform_buffer(device, "tumble cube effect", 208);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tumble cube sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let mut textures = Vec::with_capacity(6);
        let mut bind_groups = Vec::with_capacity(6);
        for _ in 0..6 {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tumble cube face"),
                size: wgpu::Extent3d {
                    width: FACE_SIZE,
                    height: FACE_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: MIP_COUNT,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Same format agg-gui-wgpu's image blits use, so the cube
                // and 2-D content agree on colour handling.
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tumble cube face bg"),
                layout: &bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: transform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: lights.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: effect.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            }));
            textures.push(texture);
        }
        Self {
            format,
            pipeline,
            vbuf,
            ibuf,
            transform,
            lights,
            effect,
            textures,
            bind_groups,
        }
    }

    pub(crate) fn upload_faces(&self, queue: &wgpu::Queue, faces: &[Arc<Vec<u8>>]) {
        for (texture, pixels) in self.textures.iter().zip(faces) {
            for (level, (w, h, data)) in mip_chain(pixels, FACE_SIZE).iter().enumerate() {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: level as u32,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(*h),
                    },
                    wgpu::Extent3d {
                        width: *w,
                        height: *h,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }

    pub(crate) fn write_uniforms(
        &self,
        queue: &wgpu::Queue,
        view: &CubeView,
        width: u32,
        height: u32,
    ) {
        let mut t = [0.0f32; 32];
        t[..16].copy_from_slice(&rows(view.modelview()));
        t[16..].copy_from_slice(&rows(view.projection()));
        queue.write_buffer(&self.transform, 0, bytemuck::cast_slice(&t));
        queue.write_buffer(&self.lights, 0, bytemuck::cast_slice(&lights_block()));
        queue.write_buffer(
            &self.effect,
            0,
            bytemuck::cast_slice(&effect_block(width, height)),
        );
    }

    pub(crate) fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("tumble cube"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    // Clear to transparent so only the cube composites,
                    // like the C# capture.
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(0.0, 0.0, width as f32, height as f32, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        pass.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        for (face, bg) in self.bind_groups.iter().enumerate() {
            pass.set_bind_group(0, bg, &[]);
            let base = (face * 6) as u32;
            pass.draw_indexed(base..base + 6, 0, 0..1);
        }
    }
}

/// Box-filtered mip chain of a square RGBA8 image, level 0 first.
fn mip_chain(base: &[u8], size: u32) -> Vec<(u32, u32, Vec<u8>)> {
    let mut out = vec![(size, size, base.to_vec())];
    let mut s = size;
    while s > 1 {
        let n = s / 2;
        let prev = &out[out.len() - 1].2;
        let mut next = vec![0u8; (n * n * 4) as usize];
        for y in 0..n {
            for x in 0..n {
                for c in 0..4 {
                    let at = |xx: u32, yy: u32| prev[((yy * s + xx) * 4 + c) as usize] as u32;
                    let sum = at(2 * x, 2 * y)
                        + at(2 * x + 1, 2 * y)
                        + at(2 * x, 2 * y + 1)
                        + at(2 * x + 1, 2 * y + 1);
                    next[((y * n + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        out.push((n, n, next));
        s = n;
    }
    out
}
