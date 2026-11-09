//! agg-sharp `Tests/Agg.Tests/Agg.WebGpuRender/WebGpuRenderDeviceTests.cs`:
//! the device limits and features agg-gui-wgpu requests (`device_descriptor`,
//! shared by `Gpu::new` and [`HeadlessGpu`]) and enforces: the adapter's
//! texture size, storage-binding and buffer-size limits, and its timestamp
//! queries when it has them.
//!
//! Of the C# class only `TheDeviceReportsAndEnforcesItsTextureSizeLimit`
//! applies; the storage, buffer-size and timestamp tests are agg-gui's own.
//! The rest of the C# class tests its `IRenderDevice` seam over wgpu-native's
//! C API (pass-state exceptions, FFI aborts, canned GL shader keys), which
//! agg-gui does not have — it calls wgpu's Rust API, whose own validation
//! covers them.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use agg_gui_wgpu::{HeadlessGpu, HeadlessTarget};

#[test]
fn the_device_reports_and_enforces_its_texture_size_limit() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let granted = gpu.device().limits().max_texture_dimension_2d;

    // At least the WebGPU default: the device asks for the adapter's own
    // maxTextureDimension2D, which every desktop adapter reports as 16384.
    // Anything below it would mean the request went out malformed.
    assert!(
        granted >= wgpu::Limits::default().max_texture_dimension_2d,
        "granted {granted}"
    );
    assert_eq!(granted, gpu.adapter_limits().max_texture_dimension_2d);

    // Enforced before wgpu sees it: an over-limit target is clamped to the
    // limit (as a shell clamps its surface) instead of becoming an error
    // texture that only fails at the next submit.
    let target = HeadlessTarget::new(gpu, granted + 1, 16);
    assert_eq!(target.size(), (granted, 16));
}

/// A device is granted the adapter's own storage-binding and buffer-size
/// limits, not wgpu's defaults (128 MiB and 256 MiB): a compute pass over a
/// large view (a ray tracer's per-pixel storage buffer, its read-back buffer)
/// needs buffers as big as the card allows, and asking for exactly what the
/// adapter reports can never fail the device request.
#[test]
fn the_device_gets_the_adapters_storage_and_buffer_size_limits() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let (device, adapter) = (gpu.device().limits(), gpu.adapter_limits());
    eprintln!(
        "{}: device max_storage_buffer_binding_size {} (adapter {}), \
         max_buffer_size {} (adapter {})",
        gpu.adapter_info().name,
        device.max_storage_buffer_binding_size,
        adapter.max_storage_buffer_binding_size,
        device.max_buffer_size,
        adapter.max_buffer_size
    );
    assert_eq!(
        device.max_storage_buffer_binding_size,
        adapter.max_storage_buffer_binding_size
    );
    assert_eq!(device.max_buffer_size, adapter.max_buffer_size);
}

/// A device can time its own work on the GPU's clock (timestamp queries)
/// exactly when the adapter offers it, and an adapter without it still
/// yields a device.
#[test]
fn the_device_has_timestamp_queries_exactly_when_the_adapter_does() {
    let gpu = match HeadlessGpu::shared() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let offered = gpu
        .wgpu_adapter()
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY);
    let granted = gpu
        .device()
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY);
    eprintln!(
        "{}: adapter offers TIMESTAMP_QUERY {offered}, device has it {granted}",
        gpu.adapter_info().name
    );
    assert_eq!(granted, offered);
}
