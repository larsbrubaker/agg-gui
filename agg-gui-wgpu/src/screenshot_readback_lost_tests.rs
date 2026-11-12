//! A read-back whose wait finds the device lost after the copy was mapped
//! refuses to read the mapped buffer, which the loss destroyed.
//!
//! wgpu cannot be made to lose a device between a map completing and its
//! read, so the wait handed to `read_texture_rgba_after` does the real wait
//! and then records the loss, as the poll that finds a loss does before it
//! returns (`crate::device_lost_flag`). Without the check the read would
//! go ahead: on a really lost device, into a panic inside wgpu.

use crate::gpu_wait::{wait_for_submission, GpuWaitError};
use crate::layer_text_readback_tests::try_device;
use crate::screenshot_readback::{read_texture_rgba_after, ReadbackError};
use crate::DeviceLostFlag;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn copyable(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot_readback_lost_test"),
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[test]
fn a_loss_found_by_the_wait_refuses_the_mapped_read() {
    let Some((device, queue)) = try_device() else {
        eprintln!("SKIP: no GPU adapter");
        return;
    };
    let texture = copyable(&device);

    // A live device: the same read returns the pixels.
    let live = DeviceLostFlag::default();
    let pixels =
        read_texture_rgba_after(&device, &queue, &texture, FORMAT, Some(live.loss()), || {
            wait_for_submission(&device, None, Some(live.loss()), crate::GPU_WAIT_BUDGET)
        })
        .expect("a live device reads back");
    assert_eq!(pixels.len(), 4 * 4 * 4);

    // The wait maps the copy, then finds the device lost.
    let lost = DeviceLostFlag::default();
    let read =
        read_texture_rgba_after(&device, &queue, &texture, FORMAT, Some(lost.loss()), || {
            let waited = wait_for_submission(&device, None, None, crate::GPU_WAIT_BUDGET);
            lost.record_for_test("Destroyed");
            waited
        });
    assert_eq!(
        read,
        Err(ReadbackError::Wait(GpuWaitError::DeviceLost(
            "Destroyed".to_owned()
        ))),
        "the mapped buffer of a lost device is not read"
    );
}
