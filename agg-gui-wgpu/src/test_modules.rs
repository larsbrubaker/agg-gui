//! The crate's in-crate test modules, gathered here so `lib.rs` declares
//! one test module rather than each of them. They are children of this
//! module, so `crate::` paths inside them reach the crate's private items as
//! they did at the root. The two that other test modules use —
//! `layer_text_readback_tests` (the shared device and target helpers) and
//! `wgsl_uniformity` (the shader uniformity analysis) — are re-exported at the
//! crate root, so `crate::layer_text_readback_tests` and
//! `crate::wgsl_uniformity` still name them.

#[path = "clip_path_readback_tests.rs"]
mod clip_path_readback_tests;
#[path = "dead_sweep_tests.rs"]
mod dead_sweep_tests;
#[path = "frame_release_tests.rs"]
mod frame_release_tests;
#[path = "golden/mod.rs"]
mod golden;
#[path = "gpu_comp_op_tests.rs"]
mod gpu_comp_op_tests;
#[cfg(not(target_arch = "wasm32"))]
#[path = "gpu_frame_capture_tests.rs"]
mod gpu_frame_capture_tests;
#[cfg(not(target_arch = "wasm32"))]
#[path = "headless_options_tests.rs"]
mod headless_options_tests;
#[cfg(not(target_arch = "wasm32"))]
#[path = "headless_tests.rs"]
mod headless_tests;
#[path = "image_blit_readback_tests.rs"]
mod image_blit_readback_tests;
#[path = "layer_text_readback_tests.rs"]
pub(crate) mod layer_text_readback_tests;
#[path = "lcd_arc_cache_tests.rs"]
mod lcd_arc_cache_tests;
#[path = "lcd_gpu_composite_tests.rs"]
mod lcd_gpu_composite_tests;
#[path = "lcd_layer_tint_readback_tests.rs"]
mod lcd_layer_tint_readback_tests;
#[path = "lcd_text_scale_readback_tests.rs"]
mod lcd_text_scale_readback_tests;
#[cfg(not(target_arch = "wasm32"))]
#[path = "opacity_headless_tests.rs"]
mod opacity_headless_tests;
#[path = "outline_text_baseline_tests.rs"]
mod outline_text_baseline_tests;
#[path = "radial_gradient_readback_tests.rs"]
mod radial_gradient_readback_tests;
#[path = "root_transform_readback_tests.rs"]
mod root_transform_readback_tests;
#[path = "rotated_text_readback_tests.rs"]
mod rotated_text_readback_tests;
#[path = "save_restore_readback_tests.rs"]
mod save_restore_readback_tests;
#[path = "screenshot_scaled_tests.rs"]
mod screenshot_scaled_tests;
#[path = "wgsl_browser_validation_tests.rs"]
mod wgsl_browser_validation_tests;
#[path = "wgsl_uniformity.rs"]
pub(crate) mod wgsl_uniformity;
