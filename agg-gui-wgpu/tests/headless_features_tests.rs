//! A headless device gets the optional features its caller asks for where the
//! adapter offers them, as a window's device does, so a test paints under
//! the same features as the app: [`HeadlessOptions::with_optional_features`].
//! A feature the adapter lacks is left off rather than failing the request.
//!
//! Skipped (passes trivially) when no GPU adapter is available.

use agg_gui_wgpu::{HeadlessGpu, HeadlessOptions};

#[test]
fn optional_features_are_on_the_device_where_the_adapter_offers_them() {
    let wanted = wgpu::Features::DEPTH32FLOAT_STENCIL8 | wgpu::Features::SHADER_F64;
    let options = HeadlessOptions::new().with_optional_features(wanted);
    let gpu = match HeadlessGpu::create_with_options(options) {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    let offered = gpu.wgpu_adapter().features();
    assert_eq!(gpu.device().features() & wanted, wanted & offered);
}

#[test]
fn a_device_asked_for_no_optional_features_has_none_of_them() {
    let gpu = match HeadlessGpu::create() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("SKIP: no GPU adapter ({e})");
            return;
        }
    };
    assert!(!gpu
        .device()
        .features()
        .contains(wgpu::Features::DEPTH32FLOAT_STENCIL8));
}
