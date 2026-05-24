//! All `wgpu::RenderPipeline` objects and associated `wgpu::BindGroupLayout`
//! resources created once at startup.
//!
//! `WgpuPipelines` is created in `WgpuGfxCtx::new()` and stored for the
//! lifetime of the context.  Every pipeline, bind-group layout, and shared
//! sampler lives here.
//!
//! Uniform data is NOT stored here — each draw command in `end_frame` creates
//! a small `wgpu::Buffer` via `device.create_buffer_init()` so that multiple
//! draw commands of the same pipeline type in one frame do not overwrite each
//! other's uniforms.

use std::mem::size_of;

use bytemuck::{Pod, Zeroable};

use crate::shaders::{
    AA_SOLID_WGSL, GRADIENT_WGSL, LAYER_WGSL, LCB_WGSL, LCD_WGSL, SOLID_WGSL,
    TEX_DOWNSAMPLE_4X_WGSL, TEX_WGSL,
};

// ---------------------------------------------------------------------------
// Uniform structs (byte layouts must exactly match WGSL structs in shaders.rs)
// ---------------------------------------------------------------------------

/// 32-byte uniform block for the solid and AA-solid pipelines.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct SolidUniforms {
    pub resolution: [f32; 2],
    pub _pad: [f32; 2],
    pub color: [f32; 4],
}
const _: () = assert!(size_of::<SolidUniforms>() == 32);

/// 32-byte uniform block for the textured-quad pipeline. `tint` is
/// a per-draw RGBA multiplier so callers can fade image blits — set
/// `[1.0, 1.0, 1.0, alpha]` for a straight alpha fade, or zero out
/// channels for a quick recolor. The 4×4-box downsample pipeline
/// reuses the same layout and simply doesn't read `tint`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct TexUniforms {
    pub resolution: [f32; 2],
    pub _pad: [f32; 2],
    pub tint: [f32; 4],
}
const _: () = assert!(size_of::<TexUniforms>() == 32);

/// 48-byte uniform block for the layer-composite pipeline.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct LayerUniforms {
    pub resolution: [f32; 2],
    pub alpha: f32,
    pub mask_enabled: u32,
    pub layer_size: [f32; 2],
    pub mask_radius: f32,
    pub _pad0: f32,
    pub mask_rect: [f32; 4],
}
const _: () = assert!(size_of::<LayerUniforms>() == 48);

/// 32-byte uniform block for the LCD subpixel-text pipeline.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct LcdUniforms {
    pub resolution: [f32; 2],
    pub channel: u32,
    pub _pad: u32,
    pub color: [f32; 4],
}
const _: () = assert!(size_of::<LcdUniforms>() == 32);

/// 16-byte uniform block for the LCD backbuffer composite pipeline.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct LcbUniforms {
    pub resolution: [f32; 2],
    pub channel: u32,
    pub _pad: u32,
}
const _: () = assert!(size_of::<LcbUniforms>() == 16);

// ---------------------------------------------------------------------------
// Blend states
// ---------------------------------------------------------------------------

/// Standard 2-D composite: src-alpha / one-minus-src-alpha for colour, with
/// premultiplied-style alpha accumulation so the same pipelines work both
/// against the surface (alpha stays pinned at 1) and against transparent layer
/// textures (alpha builds up correctly so the layer composite has something
/// to alpha-blend against).
///
/// Earlier wired this with `alpha = { Zero, One }` to "preserve framebuffer
/// alpha", which is fine on the surface but leaves layer textures stuck at
/// alpha=0 — the composite then degenerates into pure additive blending and
/// every windowed widget renders as washed-out white.  `One / OMSA` is a
/// fixed-point at 1 on the surface AND accumulates correctly on a layer.
const BLEND_STANDARD: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Premultiplied src-over for layer compositing and LCB backbuffer blits.
const BLEND_PREMUL: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

// ---------------------------------------------------------------------------
// WgpuPipelines
// ---------------------------------------------------------------------------

/// All render pipelines, bind-group layouts, and shared samplers.
///
/// Created once per [`crate::WgpuGfxCtx`] and reused for the lifetime of the
/// context.  The struct is publicly visible so library primitives like
/// [`crate::ssaa::SsaaFramebuffer::blit_to`] can take a reference and reuse
/// the shared 2-D textured-quad pipeline; the fields themselves stay
/// `pub(crate)` so external code can hold a `&WgpuPipelines` but cannot
/// reach in and rebuild individual pipelines.
pub struct WgpuPipelines {
    // ── Solid colour ─────────────────────────────────────────────────────────
    pub solid_pipeline: wgpu::RenderPipeline,
    pub solid_bgl: wgpu::BindGroupLayout,

    // ── AA solid colour (per-vertex alpha from tess2 halo strip) ─────────────
    pub aa_solid_pipeline: wgpu::RenderPipeline,
    pub aa_solid_bgl: wgpu::BindGroupLayout,

    // ── Gradient (linear + radial, SVG spread modes) ──────────────────────────
    pub gradient_pipeline: wgpu::RenderPipeline,
    pub gradient_bgl0: wgpu::BindGroupLayout,
    pub gradient_bgl1: wgpu::BindGroupLayout,

    // ── Textured quad (image blit, Label backbuffer) ──────────────────────────
    pub tex_pipeline: wgpu::RenderPipeline,
    pub tex_bgl0: wgpu::BindGroupLayout,
    pub tex_bgl1: wgpu::BindGroupLayout,

    // ── 4×4-box downsample (SSAA at 4× linear) ─────────────────────────────
    /// Same vertex / bind groups as `tex_pipeline`; fragment shader runs
    /// 4 bilinear taps in a 2×2 quadrant grid for an exact 4×4 box average.
    /// Used by the cube widget at the highest SSAA setting where a single
    /// bilinear minification would drop 12 of 16 source texels per output.
    pub tex_downsample_4x_pipeline: wgpu::RenderPipeline,

    // ── Layer composite (SDF rounded-corner mask in fragment shader) ──────────
    pub layer_pipeline: wgpu::RenderPipeline,
    pub layer_bgl0: wgpu::BindGroupLayout,
    pub layer_bgl1: wgpu::BindGroupLayout,

    // ── LCD subpixel text — three pipelines, one per colour channel ───────────
    pub lcd_r: wgpu::RenderPipeline,
    pub lcd_g: wgpu::RenderPipeline,
    pub lcd_b: wgpu::RenderPipeline,
    pub lcd_bgl0: wgpu::BindGroupLayout,
    pub lcd_bgl1: wgpu::BindGroupLayout,

    // ── LCD backbuffer composite — three pipelines, premultiplied blend ───────
    pub lcb_r: wgpu::RenderPipeline,
    pub lcb_g: wgpu::RenderPipeline,
    pub lcb_b: wgpu::RenderPipeline,
    pub lcb_bgl0: wgpu::BindGroupLayout,
    pub lcb_bgl1: wgpu::BindGroupLayout,

    // ── Shared samplers ───────────────────────────────────────────────────────
    /// Nearest-neighbour — used for LCD mask textures and Label backbuffers.
    pub nearest_sampler: wgpu::Sampler,
    /// Bilinear — used for images, layers, and gradient ramp textures.
    pub linear_sampler: wgpu::Sampler,
}

impl WgpuPipelines {
    /// Construct all pipelines.  Panics if any WGSL shader fails to compile —
    /// a startup crash is preferable to a silent runtime black screen.
    ///
    /// `sample_count` bakes into every pipeline's `MultisampleState`.  Must
    /// match the sample count of every render-pass color attachment those
    /// pipelines target — both the surface MSAA buffer and any layer MSAA
    /// buffers.  `1` disables MSAA.
    pub(crate) fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        // ── Samplers ─────────────────────────────────────────────────────────
        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let linear_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // ── Shader modules ───────────────────────────────────────────────────
        let solid_sm = mk_shader(device, "solid", SOLID_WGSL);
        let aa_solid_sm = mk_shader(device, "aa_solid", AA_SOLID_WGSL);
        let gradient_sm = mk_shader(device, "gradient", GRADIENT_WGSL);
        let tex_sm = mk_shader(device, "tex", TEX_WGSL);
        let layer_sm = mk_shader(device, "layer", LAYER_WGSL);
        let lcd_sm = mk_shader(device, "lcd", LCD_WGSL);
        let lcb_sm = mk_shader(device, "lcb", LCB_WGSL);

        // ── Bind group layouts ───────────────────────────────────────────────
        let solid_bgl = mk_uniform_bgl(device, "solid", size_of::<SolidUniforms>() as u64);
        let aa_solid_bgl = mk_uniform_bgl(device, "aa_solid", size_of::<SolidUniforms>() as u64);
        let gradient_bgl0 = mk_uniform_bgl(
            device,
            "gradient0",
            size_of::<crate::gradient::GradientUniforms>() as u64,
        );
        let gradient_bgl1 = mk_tex1_bgl(device, "gradient1");
        let tex_bgl0 = mk_uniform_bgl(device, "tex0", size_of::<TexUniforms>() as u64);
        let tex_bgl1 = mk_tex1_bgl(device, "tex1");
        let layer_bgl0 = mk_uniform_bgl(device, "layer0", size_of::<LayerUniforms>() as u64);
        let layer_bgl1 = mk_tex1_bgl(device, "layer1");
        let lcd_bgl0 = mk_uniform_bgl(device, "lcd0", size_of::<LcdUniforms>() as u64);
        let lcd_bgl1 = mk_tex1_bgl(device, "lcd1");
        let lcb_bgl0 = mk_uniform_bgl(device, "lcb0", size_of::<LcbUniforms>() as u64);
        let lcb_bgl1 = mk_tex2_bgl(device, "lcb1");

        // ── Pipeline layouts ─────────────────────────────────────────────────
        let solid_pl = mk_layout(device, "solid", &[&solid_bgl]);
        let aa_solid_pl = mk_layout(device, "aa_solid", &[&aa_solid_bgl]);
        let gradient_pl = mk_layout(device, "gradient", &[&gradient_bgl0, &gradient_bgl1]);
        let tex_pl = mk_layout(device, "tex", &[&tex_bgl0, &tex_bgl1]);
        let layer_pl = mk_layout(device, "layer", &[&layer_bgl0, &layer_bgl1]);
        let lcd_pl = mk_layout(device, "lcd", &[&lcd_bgl0, &lcd_bgl1]);
        let lcb_pl = mk_layout(device, "lcb", &[&lcb_bgl0, &lcb_bgl1]);

        // ── Shared vertex attribute slices ───────────────────────────────────
        // Defined once here so that every VertexBufferLayout below can borrow them
        // without moving them (they outlive all the pipeline create calls below).
        let pos2_attrs = [wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        }];
        let pos2_alpha_attrs = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32,
                offset: 8,
                shader_location: 1,
            },
        ];
        let pos2_uv2_attrs = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 8,
                shader_location: 1,
            },
        ];

        // Convenience closures — return a fresh VertexBufferLayout borrowing the
        // attribute arrays above.  Closures rather than values avoid requiring
        // VertexBufferLayout to be Copy (it only derives Clone).
        let vbl_pos2 = || wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &pos2_attrs,
        };
        let vbl_pos2_alpha = || wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &pos2_alpha_attrs,
        };
        let vbl_pos2_uv2 = || wgpu::VertexBufferLayout {
            array_stride: 16,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &pos2_uv2_attrs,
        };

        // ── Render pipelines ─────────────────────────────────────────────────
        let solid_pipeline = build_pipeline(
            device,
            "solid",
            &solid_pl,
            &solid_sm,
            &solid_sm,
            &[vbl_pos2()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::ALL,
            sample_count,
        );
        let aa_solid_pipeline = build_pipeline(
            device,
            "aa_solid",
            &aa_solid_pl,
            &aa_solid_sm,
            &aa_solid_sm,
            &[vbl_pos2_alpha()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::ALL,
            sample_count,
        );
        let gradient_pipeline = build_pipeline(
            device,
            "gradient",
            &gradient_pl,
            &gradient_sm,
            &gradient_sm,
            &[vbl_pos2_alpha()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::ALL,
            sample_count,
        );
        let tex_pipeline = build_pipeline(
            device,
            "tex",
            &tex_pl,
            &tex_sm,
            &tex_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::ALL,
            sample_count,
        );
        let tex_ds4_sm = mk_shader(device, "tex_downsample_4x", TEX_DOWNSAMPLE_4X_WGSL);
        let tex_downsample_4x_pipeline = build_pipeline(
            device,
            "tex_downsample_4x",
            &tex_pl,
            &tex_ds4_sm,
            &tex_ds4_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::ALL,
            sample_count,
        );
        let layer_pipeline = build_pipeline(
            device,
            "layer",
            &layer_pl,
            &layer_sm,
            &layer_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_PREMUL),
            wgpu::ColorWrites::ALL,
            sample_count,
        );
        let lcd_r = build_pipeline(
            device,
            "lcd_r",
            &lcd_pl,
            &lcd_sm,
            &lcd_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::RED,
            sample_count,
        );
        let lcd_g = build_pipeline(
            device,
            "lcd_g",
            &lcd_pl,
            &lcd_sm,
            &lcd_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::GREEN,
            sample_count,
        );
        let lcd_b = build_pipeline(
            device,
            "lcd_b",
            &lcd_pl,
            &lcd_sm,
            &lcd_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_STANDARD),
            wgpu::ColorWrites::BLUE,
            sample_count,
        );
        let lcb_r = build_pipeline(
            device,
            "lcb_r",
            &lcb_pl,
            &lcb_sm,
            &lcb_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_PREMUL),
            wgpu::ColorWrites::RED,
            sample_count,
        );
        let lcb_g = build_pipeline(
            device,
            "lcb_g",
            &lcb_pl,
            &lcb_sm,
            &lcb_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_PREMUL),
            wgpu::ColorWrites::GREEN,
            sample_count,
        );
        let lcb_b = build_pipeline(
            device,
            "lcb_b",
            &lcb_pl,
            &lcb_sm,
            &lcb_sm,
            &[vbl_pos2_uv2()],
            surface_format,
            Some(BLEND_PREMUL),
            wgpu::ColorWrites::BLUE,
            sample_count,
        );

        Self {
            solid_pipeline,
            solid_bgl,
            aa_solid_pipeline,
            aa_solid_bgl,
            gradient_pipeline,
            gradient_bgl0,
            gradient_bgl1,
            tex_pipeline,
            tex_bgl0,
            tex_bgl1,
            tex_downsample_4x_pipeline,
            layer_pipeline,
            layer_bgl0,
            layer_bgl1,
            lcd_r,
            lcd_g,
            lcd_b,
            lcd_bgl0,
            lcd_bgl1,
            lcb_r,
            lcb_g,
            lcb_b,
            lcb_bgl0,
            lcb_bgl1,
            nearest_sampler,
            linear_sampler,
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn mk_shader(device: &wgpu::Device, label: &str, src: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    })
}

/// Bind-group layout with a single uniform buffer at binding 0, visible to
/// both vertex and fragment stages.
fn mk_uniform_bgl(device: &wgpu::Device, label: &str, size: u64) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(size),
            },
            count: None,
        }],
    })
}

/// Bind-group layout for group 1 with one `texture_2d<f32>` (binding 0) and
/// one filtering sampler (binding 1).
fn mk_tex1_bgl(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
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
fn mk_tex2_bgl(device: &wgpu::Device, label: &str) -> wgpu::BindGroupLayout {
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

fn mk_layout(
    device: &wgpu::Device,
    label: &str,
    bgls: &[&wgpu::BindGroupLayout],
) -> wgpu::PipelineLayout {
    let opt: Vec<Option<&wgpu::BindGroupLayout>> = bgls.iter().copied().map(Some).collect();
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &opt,
        immediate_size: 0,
    })
}

/// Create a `RenderPipeline` from a single shader module (vs and fs are the
/// same module), a single vertex buffer slot, a blend state, and a colour
/// write mask.  All 2D pipelines use `TriangleList` topology with no depth
/// test, no culling, and no MSAA.
fn build_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    vs: &wgpu::ShaderModule,
    fs: &wgpu::ShaderModule,
    vertex_buffers: &[wgpu::VertexBufferLayout<'_>],
    surface_format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    write_mask: wgpu::ColorWrites,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: vs,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: vertex_buffers,
        },
        fragment: Some(wgpu::FragmentState {
            module: fs,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend,
                write_mask,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}
