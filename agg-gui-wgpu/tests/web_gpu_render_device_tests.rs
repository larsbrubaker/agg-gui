//! agg-sharp `Tests/Agg.Tests/Agg.WebGpuRender/WebGpuRenderDeviceTests.cs`:
//! the device limits agg-gui-wgpu requests (`device_descriptor`, shared by
//! `Gpu::new` and [`HeadlessGpu`]) and enforces.
//!
//! Only `TheDeviceReportsAndEnforcesItsTextureSizeLimit` applies: the rest of
//! the C# class tests its `IRenderDevice` seam over wgpu-native's C API
//! (pass-state exceptions, FFI aborts, canned GL shader keys), which agg-gui
//! does not have — it calls wgpu's Rust API, whose own validation covers them.
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
