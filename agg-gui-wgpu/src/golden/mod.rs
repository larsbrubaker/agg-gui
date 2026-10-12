//! The ported agg-sharp GoldenImages classes (`Tests/Agg.Tests/Agg.GoldenImages`),
//! one file per C# class, comparing the wgpu backend with the software
//! `GfxCtx` through `harness.rs`.

mod harness;

mod golden_2d_primitive_tests;
mod golden_text_tests;
mod gpu_clear_tests;
mod gpu_fill_rule_tests;
mod gpu_translucent_image_tests;
mod gpu_widget_backbuffer_tests;
mod image_render_transform_tests;
mod text_widget_inset_tests;
mod transparent_text_tests;
