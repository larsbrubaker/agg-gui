//! Re-exports that keep `agg_gui::draw_ctx::{FillRule, GlPaint, …}` import
//! paths resolving: the paint definitions live in [`crate::paints`], the
//! `GlPaint` trait in `draw_ctx/gl_paint.rs` and `ImageSampling` in
//! `draw_ctx/image_sampling.rs`.  Pulled in by `draw_ctx.rs`
//! with a glob re-export.

pub use super::gl_paint::GlPaint;
pub use super::image_sampling::ImageSampling;
pub use super::paint_nesting::PaintNesting;
pub use crate::paints::{
    FillRule, GradientSpread, GradientStop, LinearGradientPaint, PatternPaint, RadialGradientPaint,
};
