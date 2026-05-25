//! 1024×1 RGBA8 alpha-step texture for the AA-texture pipeline.
//!
//! Direct port of `Graphics2DGpu::CheckLineImageCache` in agg-sharp.
//! Split out of `lib.rs` to keep that file under the 800-line cap.

/// Build the alpha-step texture and upload its pixels.
///
/// Pixel layout:
/// - Column 0:        `(255, 255, 255, 0)`  — fully transparent
/// - Columns 1..1023: `(255, 255, 255, 255)` — fully opaque
///
/// Sampled LINEAR, the boundary between texel 0 and texel 1 produces a
/// sub-texel α ramp — that's the AA edge. See
/// `agg_gui::gl_renderer::aa_texture_mesh` for the texcoord scheme.
///
/// We only need the α=255 variant (agg-sharp builds 256 of them, one
/// per polygon-alpha value); the shader multiplies in the polygon's
/// colour alpha as a uniform instead.
pub(crate) fn build_aa_step_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> wgpu::Texture {
    const W: u32 = 1024;
    let mut pixels = vec![255u8; (W as usize) * 4];
    // Column 0: zero out the alpha byte (offset 3 in RGBA).
    pixels[3] = 0;

    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aa_step"),
        size: wgpu::Extent3d {
            width: W,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(W * 4),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: W,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    tex
}
