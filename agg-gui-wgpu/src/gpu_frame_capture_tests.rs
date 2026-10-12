//! Port of agg-sharp `Agg.RenderGl.Compat/GpuFrameCaptureTests.cs`'s
//! `ATargetThatCannotBeCopiedFromIsRefusedRatherThanSilentlyBlank`: a capture
//! of a texture created without `COPY_SRC` is refused by
//! `screenshot_readback::read_texture_rgba` rather than copied into a blank
//! image. The class's PNG-writing tests are in `agg-gui-shell`'s
//! `screenshot.rs`, where every capture is written.

use crate::layer_text_readback_tests::try_device;
use crate::screenshot_readback::read_texture_rgba;

fn texture(device: &wgpu::Device, usage: wgpu::TextureUsages) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gpu_frame_capture_test"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage,
        view_formats: &[],
    })
}

#[test]
fn a_target_that_cannot_be_copied_from_is_refused_rather_than_silently_blank() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let no_copy = texture(&device, wgpu::TextureUsages::RENDER_ATTACHMENT);
    assert!(
        read_texture_rgba(&device, &queue, &no_copy, wgpu::TextureFormat::Bgra8Unorm).is_err(),
        "a texture without COPY_SRC is refused"
    );
    // The same target with COPY_SRC reads back.
    let copyable = texture(
        &device,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let pixels = read_texture_rgba(&device, &queue, &copyable, wgpu::TextureFormat::Bgra8Unorm)
        .expect("a copyable texture reads back");
    assert_eq!(pixels.len(), 4 * 4 * 4);
}
