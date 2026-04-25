//! GL renderer infrastructure — tess2 bridge + GL command buffer.
//!
//! This module provides the building blocks for hardware-accelerated rendering:
//!
//! - [`tess2_bridge`] — converts AGG-style polygon contours into GL triangle
//!   meshes using tess2-rust.
//!
//! The higher-level [`GlGfxCtx`] (a drop-in parallel to [`crate::GfxCtx`] for
//! GL targets) and the full [`RenderTarget`] abstraction are planned extensions.
//!
//! # Reference
//!
//! Modelled after the MatterCAD agg-sharp `Graphics2DGpu` / `AARenderTesselator`
//! pipeline: shapes are tessellated to triangle meshes, then uploaded as VBOs
//! and rendered with a simple colour-fill shader.  Anti-aliased edge expansion
//! (one-pixel outward quad + coverage ramp) follows `AARenderTesselator.cs`.

pub mod glyph_cache;
pub mod tess2_bridge;

pub use glyph_cache::GlyphCache;
pub use tess2_bridge::{
    agg_path_to_contours, expand_aa_halo, tessellate_circle, tessellate_fill, tessellate_interior,
    tessellate_path, tessellate_path_aa, tessellate_rect, tessellate_rounded_rect, CachedTess,
};
