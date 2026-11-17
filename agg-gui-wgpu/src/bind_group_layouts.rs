//! The bind-group layouts of the 2-D pipelines in `pipelines.rs`: group 0
//! holds a draw's uniforms, group 1 the texture(s) and sampler it samples.
//!
//! Every uniform layout takes a dynamic offset, so the bind groups over the
//! frame's uniform arena are made once per chunk and reused by every frame
//! (`bind_group_cache.rs` says why that matters). Anything binding one of
//! these layouts as group 0 passes the offset of its uniforms with
//! `set_bind_group` (the SSAA blit in `ssaa.rs` passes `0` for its own
//! buffer).

/// Bind-group layout with a single uniform buffer of `size` bytes (the
/// pipeline's uniform struct) at binding 0, visible to both vertex and
/// fragment stages. The offset is dynamic, given with each draw: a bind
/// group made once over a chunk of the frame's uniform arena serves every
/// draw whose uniforms are in that chunk (`bind_group_cache.rs`). A bind
/// group over a buffer of its own binds it with offset `0`.
pub(crate) fn mk_uniform_bgl(
    device: &wgpu::Device,
    label: &str,
    size: u64,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        }],
    })
}

/// Bind-group layout for group 1 with one `texture_2d<f32>` (binding 0) and
/// one filtering sampler (binding 1).
pub(crate) fn mk_tex1_bgl(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Bind-group layout for the LCB pipeline's group 1: two `texture_2d<f32>`
/// (bindings 0 and 1, colour plane + alpha plane) and one sampler (binding 2).
pub(crate) fn mk_tex2_bgl(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}
