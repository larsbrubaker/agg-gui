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
//! (an outward quad per outline edge with a coverage ramp, half a pixel wide by
//! default) follows `HaloAaTesselator.cs`, whose tests are
//! `halo_aa_tesselator_tests.rs`.

pub mod aa_texture_mesh;
pub mod glyph_cache;
#[cfg(test)]
mod halo_aa_tesselator_tests;
pub mod tess2_bridge;

pub use aa_texture_mesh::{tessellate_path_aa_texture, AaTexVertex};
pub use glyph_cache::GlyphCache;
pub use tess2_bridge::{
    agg_path_to_contours, expand_aa_halo, install_tess_panic_logger, tessellate_circle,
    tessellate_fill, tessellate_interior, tessellate_interior_with_rule, tessellate_path,
    tessellate_path_aa, tessellate_rect, tessellate_rounded_rect, CachedTess, AA_HALO_WIDTH,
};
